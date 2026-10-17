//! Session downloads on the WebView2 UI thread; no unsafe COM Send/Sync wrappers.
use crate::BrowserEvent;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use webview2_com::{
    BytesReceivedChangedEventHandler, DownloadStartingEventHandler,
    Microsoft::Web::WebView2::Win32::*, StateChangedEventHandler,
};
use windows::core::{Interface, HSTRING};
use winit::{event_loop::EventLoopProxy, window::WindowId};
use wry::WebViewExtWindows;

struct Pending {
    window: WindowId,
    args: ICoreWebView2DownloadStartingEventArgs,
    deferral: ICoreWebView2Deferral,
}
struct Download {
    window: WindowId,
    operation: ICoreWebView2DownloadOperation,
    path: PathBuf,
}
thread_local! {
    static PENDING:RefCell<HashMap<u64,Pending>>=RefCell::new(HashMap::new());
    static ACTIVE:RefCell<HashMap<u64,Download>>=RefCell::new(HashMap::new());
}
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn attach(
    view: &wry::WebView,
    window: WindowId,
    proxy: EventLoopProxy<BrowserEvent>,
) -> windows::core::Result<()> {
    let core = unsafe {
        view.controller()
            .CoreWebView2()?
            .cast::<ICoreWebView2_4>()?
    };
    let handler = DownloadStartingEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let pending = Pending {
                window,
                deferral: unsafe { args.GetDeferral()? },
                args,
            };
            PENDING.with(|store| store.borrow_mut().insert(id, pending));
            if proxy
                .send_event(BrowserEvent::DownloadRequested(window, id))
                .is_err()
            {
                cancel_pending(id);
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe {
        core.add_DownloadStarting(&handler, &mut token)?;
    }
    Ok(())
}
fn cancel_pending(id: u64) {
    if let Some(p) = PENDING.with(|s| s.borrow_mut().remove(&id)) {
        unsafe {
            let _ = p.args.SetCancel(true);
            let _ = p.deferral.Complete();
        }
    }
}
pub fn start(
    window: WindowId,
    id: u64,
    proxy: EventLoopProxy<BrowserEvent>,
    settings: &evergreen_core::settings::Settings,
) {
    let Some(pending) = PENDING.with(|s| s.borrow_mut().remove(&id)) else {
        return;
    };
    let result = (|| -> windows::core::Result<()> {
        let operation = unsafe { pending.args.DownloadOperation()? };
        let mut raw = windows::core::PWSTR::null();
        unsafe {
            operation.ResultFilePath(&mut raw)?;
        }
        let suggested = PathBuf::from(webview2_com::take_pwstr(raw));
        let filename = suggested
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("download");
        let chosen = if settings.downloads.ask_where_to_save {
            crate::accelerator::show_save_file_dialog(filename, "Lưu tệp tải xuống")
        } else {
            // Preserve existing files: choose a free destination instead of overwriting.
            let folder = &settings.downloads.default_folder;
            std::fs::create_dir_all(folder).map_err(|e| {
                windows::core::Error::new(
                    windows::core::HRESULT(0x80004005u32 as i32),
                    e.to_string(),
                )
            })?;
            let mut path = folder.join(filename);
            for n in 1..10000 {
                if !path.exists() {
                    break;
                }
                path = folder.join(format!(
                    "{} ({n}){}",
                    suggested
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("download"),
                    suggested
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| format!(".{s}"))
                        .unwrap_or_default()
                ));
            }
            if path.exists() {
                None
            } else {
                Some(path)
            }
        };
        let Some(path) = chosen else {
            unsafe {
                pending.args.SetCancel(true)?;
            }
            return Ok(());
        };
        unsafe {
            pending
                .args
                .SetResultFilePath(&HSTRING::from(path.to_string_lossy().as_ref()))?;
            pending.args.SetHandled(true)?;
        }
        ACTIVE.with(|s| {
            s.borrow_mut().insert(
                id,
                Download {
                    window,
                    operation: operation.clone(),
                    path,
                },
            )
        });
        let bytes_proxy = proxy.clone();
        let bytes = BytesReceivedChangedEventHandler::create(Box::new(move |_, _| {
            report(id, &bytes_proxy);
            Ok(())
        }));
        let state_proxy = proxy.clone();
        let state = StateChangedEventHandler::create(Box::new(move |_, _| {
            report(id, &state_proxy);
            Ok(())
        }));
        let mut token = 0;
        unsafe {
            operation.add_BytesReceivedChanged(&bytes, &mut token)?;
            operation.add_StateChanged(&state, &mut token)?;
        }
        report(id, &proxy);
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("Pip download: {error}");
        unsafe {
            let _ = pending.args.SetCancel(true);
        }
    }
    unsafe {
        let _ = pending.deferral.Complete();
    }
}
fn report(id: u64, proxy: &EventLoopProxy<BrowserEvent>) {
    let event = ACTIVE.with(|store| {
        let store = store.borrow();
        let d = store.get(&id)?;
        let mut received = 0;
        let mut total = 0;
        let mut state = COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS;
        unsafe {
            d.operation.BytesReceived(&mut received).ok()?;
            d.operation.TotalBytesToReceive(&mut total).ok()?;
            d.operation.State(&mut state).ok()?;
        }
        Some(BrowserEvent::DownloadProgress {
            window_id: d.window,
            download_id: id,
            filename: d.path.file_name()?.to_string_lossy().into(),
            received_bytes: received,
            total_bytes: total,
            state: if state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
                "Completed"
            } else if state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED {
                "Interrupted"
            } else {
                "InProgress"
            }
            .into(),
        })
    });
    if let Some(event) = event {
        let _ = proxy.send_event(event);
    }
}
pub fn cancel(window: WindowId, id: u64) {
    // Drop the RefCell borrow before COM callbacks can reenter.
    let operation = ACTIVE.with(|s| {
        s.borrow()
            .get(&id)
            .filter(|d| d.window == window)
            .map(|d| d.operation.clone())
    });
    if let Some(op) = operation {
        unsafe {
            let _ = op.Cancel();
        }
    }
}
pub fn open(window: WindowId, id: u64, folder: bool) {
    let entry = ACTIVE.with(|s| {
        s.borrow()
            .get(&id)
            .filter(|d| d.window == window)
            .map(|d| (d.path.clone(), d.operation.clone()))
    });
    let path = entry.and_then(|(path, op)| {
        let mut state = COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS;
        unsafe {
            op.State(&mut state).ok()?;
        }
        if state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
            Some(path)
        } else {
            None
        }
    });
    if let Some(path) = path.filter(|p| p.is_file()) {
        if folder {
            let _ = std::process::Command::new("explorer.exe")
                .arg(format!("/select,{}", path.display()))
                .spawn();
        } else {
            unsafe {
                let _ = windows::Win32::UI::Shell::ShellExecuteW(
                    None,
                    windows::core::w!("open"),
                    &HSTRING::from(path.to_string_lossy().as_ref()),
                    None,
                    None,
                    windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                );
            }
        }
    }
}
pub fn clear_finished(window: WindowId) {
    let operations = ACTIVE.with(|s| {
        s.borrow()
            .iter()
            .filter(|(_, d)| d.window == window)
            .map(|(id, d)| (*id, d.operation.clone()))
            .collect::<Vec<_>>()
    });
    let completed = operations
        .into_iter()
        .filter_map(|(id, op)| {
            let mut state = COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS;
            unsafe {
                op.State(&mut state).ok()?;
            }
            if state != COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS {
                Some(id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    ACTIVE.with(|s| {
        let mut s = s.borrow_mut();
        for id in completed {
            s.remove(&id);
        }
    });
}

/// Release completed download handles and resolve outstanding prompts when a window closes.
/// In-progress transfers keep their native operation until their completion event.
pub fn close_window(window: WindowId) {
    let pending = PENDING.with(|s| {
        s.borrow()
            .iter()
            .filter(|(_, p)| p.window == window)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>()
    });
    for id in pending {
        cancel_pending(id);
    }
    clear_finished(window);
}

#[cfg(debug_assertions)]
pub fn retained_downloads() -> usize {
    ACTIVE.with(|s| s.borrow().len())
}
