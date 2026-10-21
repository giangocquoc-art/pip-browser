//! Win32 keyboard accelerator hook for WebView2.
//! Intercepts browser shortcuts (Ctrl+T, Ctrl+W, Ctrl+L, F12, etc.) at the COM layer
//! before child webview HWNDs consume them.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(target_os = "windows")]
use windows::core::Interface;
use winit::event_loop::EventLoopProxy;
use wry::WebView;

#[cfg(any(target_os = "windows", test))]
fn find_navigation_shortcut(
    ctrl: bool,
    shift: bool,
    alt: bool,
    virtual_key: u32,
) -> Option<&'static str> {
    if alt {
        return None;
    }

    match (ctrl, shift, virtual_key) {
        (true, false, 0x47) | (false, false, 0x72) => Some("FindNext"),
        (true, true, 0x47) | (false, true, 0x72) => Some("FindPrevious"),
        _ => None,
    }
}

#[cfg(any(target_os = "windows", test))]
fn devtools_shortcut(ctrl: bool, shift: bool, alt: bool, virtual_key: u32) -> Option<&'static str> {
    if !ctrl || !shift || alt {
        return None;
    }
    match virtual_key {
        0x49 => Some("Ctrl+Shift+I"),
        0x4A => Some("Ctrl+Shift+J"),
        _ => None,
    }
}

#[cfg(any(target_os = "windows", test))]
fn unmodified_browser_key(ctrl: bool, alt: bool, virtual_key: u32) -> Option<&'static str> {
    if ctrl || alt {
        return None;
    }
    match virtual_key {
        0x7A => Some("F11"),
        0x7B => Some("F12"),
        0x74 => Some("F5"),
        0x1B => Some("Escape"),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
pub struct ActivePermission {
    window: winit::window::WindowId,
    tab: evergreen_core::tabs::TabId,
    pub args:
        webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2PermissionRequestedEventArgs,
    pub deferral: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Deferral,
}

#[cfg(target_os = "windows")]
static NEXT_PERMISSION_ID: AtomicU64 = AtomicU64::new(1);
#[cfg(target_os = "windows")]
thread_local! {
    static PERMISSION_STORE: std::cell::RefCell<HashMap<u64,ActivePermission>> = std::cell::RefCell::new(HashMap::new());
}
#[cfg(target_os = "windows")]
pub fn resolve_permission(permission_id: u64, allow: bool) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PERMISSION_STATE_ALLOW, COREWEBVIEW2_PERMISSION_STATE_DENY,
    };
    let active = PERMISSION_STORE.with(|s| s.borrow_mut().remove(&permission_id));
    if let Some(active) = active {
        unsafe {
            let _ = active.args.SetState(if allow {
                COREWEBVIEW2_PERMISSION_STATE_ALLOW
            } else {
                COREWEBVIEW2_PERMISSION_STATE_DENY
            });
            let _ = active.deferral.Complete();
        }
    }
}
pub fn cancel_permissions(
    window: winit::window::WindowId,
    tab: Option<evergreen_core::tabs::TabId>,
    keep: Option<u64>,
) {
    let ids = PERMISSION_STORE.with(|store| {
        store
            .borrow()
            .iter()
            .filter(|(id, p)| {
                p.window == window && tab.is_none_or(|tab| p.tab == tab) && Some(**id) != keep
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>()
    });
    for id in ids {
        resolve_permission(id, false);
    }
}
#[cfg(not(target_os = "windows"))]
pub fn resolve_permission(_permission_id: u64, _allow: bool) {}

#[cfg(target_os = "windows")]
pub fn wake_webview(wv: &WebView) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_3;
    use windows::core::Interface;
    use wry::WebViewExtWindows;

    let controller = wv.controller();
    let _ = unsafe { controller.NotifyParentWindowPositionChanged() };
    if let Ok(core) = unsafe { controller.CoreWebView2() } {
        if let Ok(core3) = core.cast::<ICoreWebView2_3>() {
            let mut is_suspended = windows::core::BOOL(0);
            let suspended =
                unsafe { core3.IsSuspended(&mut is_suspended) }.is_ok() && is_suspended.as_bool();
            if suspended {
                // Resume automatically restores the NORMAL memory target. Setting it
                // first is ignored by WebView2 while suspended, so avoid that extra COM call.
                let _ = unsafe { core3.Resume() };
            } else if let Ok(core19) =
                core.cast::<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_19>()
            {
                // Tabs hidden without entering suspension were marked LOW by hide_webview.
                let _ = unsafe {
                    core19.SetMemoryUsageTargetLevel(
                        webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
                    )
                };
            }
        } else if let Ok(core19) =
            core.cast::<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_19>()
        {
            // This runtime can request a memory target but cannot expose suspension.
            let _ = unsafe {
                core19.SetMemoryUsageTargetLevel(
                    webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
                )
            };
        }
    }
}

pub fn hide_webview(wv: &WebView) {
    if wv.set_visible(false).is_err() {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        use webview2_com::Microsoft::Web::WebView2::Win32::*;
        use windows::core::Interface;
        use wry::WebViewExtWindows;
        if let Ok(core) = unsafe { wv.controller().CoreWebView2() } {
            if let Ok(core19) = core.cast::<ICoreWebView2_19>() {
                // Native cache reduction, without suspending calls, downloads or unsaved pages.
                let _ = unsafe {
                    core19.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW)
                };
            }
        }
    }
}

pub fn reload_without_cache(wv: &WebView) {
    use wry::WebViewExtWindows;
    unsafe {
        if let Ok(core) = wv.controller().CoreWebView2() {
            let _ = core.CallDevToolsProtocolMethod(
                windows::core::w!("Page.reload"),
                windows::core::w!("{\"ignoreCache\":true}"),
                None,
            );
        }
    }
}

/// Open WebView2's native print experience instead of calling page-controlled `window.print()`.
#[cfg(target_os = "windows")]
pub fn print_page(wv: &WebView) -> Result<(), String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_16, COREWEBVIEW2_PRINT_DIALOG_KIND_BROWSER,
    };
    use wry::WebViewExtWindows;

    let core = unsafe { wv.controller().CoreWebView2() }.map_err(|e| e.to_string())?;
    let printable = core.cast::<ICoreWebView2_16>().map_err(|e| e.to_string())?;
    unsafe { printable.ShowPrintUI(COREWEBVIEW2_PRINT_DIALOG_KIND_BROWSER) }
        .map_err(|e| e.to_string())
}

/// Save the active document with WebView2's built-in Save As flow and resource handling.
#[cfg(target_os = "windows")]
pub fn save_page(wv: &WebView) -> Result<(), String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_25;
    use wry::WebViewExtWindows;

    let core = unsafe { wv.controller().CoreWebView2() }.map_err(|e| e.to_string())?;
    let save_as = core
        .cast::<ICoreWebView2_25>()
        .map_err(|e| format!("Lưu trang cần WebView2 Runtime mới hơn: {e}"))?;
    let callback =
        webview2_com::ShowSaveAsUICompletedHandler::create(Box::new(move |result, outcome| {
            if let Err(error) = result {
                eprintln!("Pip Save As failed: {error}");
            } else if outcome.0 != 0 {
                eprintln!("Pip Save As completed with WebView2 result {}", outcome.0);
            }
            Ok(())
        }));
    unsafe { save_as.ShowSaveAsUI(&callback) }.map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
pub fn suspend_webview(
    wv: &WebView,
    window_id: winit::window::WindowId,
    tab_id: evergreen_core::tabs::TabId,
    proxy: EventLoopProxy<crate::BrowserEvent>,
) -> Result<(), String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_3;
    use windows::core::Interface;
    use wry::WebViewExtWindows;

    let mut visible = windows::core::BOOL(0);
    unsafe { wv.controller().IsVisible(&mut visible) }.map_err(|error| error.to_string())?;
    if visible.as_bool() {
        return Err("tab is still visible".into());
    }
    let core = unsafe { wv.controller().CoreWebView2() }.map_err(|error| error.to_string())?;
    let core3 = core
        .cast::<ICoreWebView2_3>()
        .map_err(|error| error.to_string())?;
    let callback =
        webview2_com::TrySuspendCompletedHandler::create(Box::new(move |result, suspended| {
            let did_suspend = result.is_ok() && suspended;
            let _ = proxy.send_event(crate::BrowserEvent::TabSuspendResult(
                window_id,
                tab_id,
                did_suspend,
            ));
            Ok(())
        }));
    unsafe { core3.TrySuspend(&callback) }.map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
pub fn focus_webview(wv: &WebView) {
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC;
    use wry::WebViewExtWindows;

    let controller = wv.controller();
    let _ = unsafe { controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) };
}

#[cfg(not(target_os = "windows"))]
pub fn wake_webview(_wv: &WebView) {}

#[cfg(not(target_os = "windows"))]
pub fn focus_webview(_wv: &WebView) {}

#[cfg(target_os = "windows")]
pub fn attach_accelerator_keys(
    webview: &WebView,
    window_id: winit::window::WindowId,
    proxy: EventLoopProxy<crate::BrowserEvent>,
) {
    use webview2_com::AcceleratorKeyPressedEventHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_MENU, VK_SHIFT,
    };
    use wry::WebViewExtWindows;

    let controller = webview.controller().clone();

    let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_sender, args| {
        if let Some(args) = args {
            let mut kind =
                webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_KEY_EVENT_KIND(0);
            if unsafe { args.KeyEventKind(&mut kind) }.is_err() {
                return Ok(());
            }

            if kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                || kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
            {
                let mut vkey = 0;
                if unsafe { args.VirtualKey(&mut vkey) }.is_ok() {
                    let ctrl = unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) } & -32768 != 0;
                    let alt = unsafe { GetAsyncKeyState(VK_MENU.0 as i32) } & -32768 != 0;
                    let shift = unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) } & -32768 != 0;

                    let mut handled = false;
                    let mut shortcut = None;

                    if let Some(find_shortcut) = find_navigation_shortcut(ctrl, shift, alt, vkey) {
                        shortcut = Some(find_shortcut);
                        handled = true;
                    } else if let Some(devtools) = devtools_shortcut(ctrl, shift, alt, vkey) {
                        shortcut = Some(devtools);
                        handled = true;
                    } else if ctrl && !alt {
                        match vkey {
                            0x2E if shift => {
                                // Open the browser data-clear controls.
                                shortcut = Some("Ctrl+Shift+Delete");
                                handled = true;
                            }
                            0x4D if shift => {
                                // Chrome-style profile switcher.
                                shortcut = Some("Ctrl+Shift+M");
                                handled = true;
                            }
                            0x41 if shift => {
                                shortcut = Some("Ctrl+Shift+A");
                                handled = true;
                            }
                            0x54 => {
                                // 'T'
                                if shift {
                                    shortcut = Some("Ctrl+Shift+T");
                                } else {
                                    shortcut = Some("Ctrl+T");
                                }
                                handled = true;
                            }
                            0x57 => {
                                // 'W'
                                shortcut = Some(if shift { "Ctrl+Shift+W" } else { "Ctrl+W" });
                                handled = true;
                            }
                            0x4C => {
                                // 'L'
                                shortcut = Some("Ctrl+L");
                                handled = true;
                            }
                            0x45 => {
                                // 'E'
                                shortcut = Some("Ctrl+E");
                                handled = true;
                            }
                            0x4B if shift => {
                                // Open Pip's lightweight tab switcher.
                                shortcut = Some("Ctrl+Shift+K");
                                handled = true;
                            }
                            0x4B => {
                                // 'K'
                                shortcut = Some("Ctrl+K");
                                handled = true;
                            }
                            0x46 => {
                                // 'F'
                                shortcut = Some("Ctrl+F");
                                handled = true;
                            }
                            0x4A => {
                                // 'J'
                                shortcut = Some("Ctrl+J");
                                handled = true;
                            }
                            0x48 => {
                                // 'H'
                                shortcut = Some("Ctrl+H");
                                handled = true;
                            }
                            0x4F if shift => {
                                // 'O' opens the bookmarks manager.
                                shortcut = Some("Ctrl+Shift+O");
                                handled = true;
                            }
                            0x44 if !shift => {
                                // 'D'
                                shortcut = Some("Ctrl+D");
                                handled = true;
                            }
                            0x50 => {
                                // 'P'
                                shortcut = Some("Ctrl+P");
                                handled = true;
                            }
                            0x53 => {
                                // 'S' opens WebView2's native Save As flow.
                                shortcut = Some("Ctrl+S");
                                handled = true;
                            }
                            0x52 => {
                                // 'R'
                                if shift {
                                    shortcut = Some("Ctrl+Shift+R");
                                } else {
                                    shortcut = Some("Ctrl+R");
                                }
                                handled = true;
                            }
                            0x4E => {
                                // 'N'
                                if shift {
                                    shortcut = Some("Ctrl+Shift+N");
                                } else {
                                    shortcut = Some("Ctrl+N");
                                }
                                handled = true;
                            }
                            0x09 => {
                                // Tab
                                if shift {
                                    shortcut = Some("Ctrl+Shift+Tab");
                                } else {
                                    shortcut = Some("Ctrl+Tab");
                                }
                                handled = true;
                            }
                            0x21 => {
                                // Page Up: previous tab, as in Chrome.
                                shortcut = Some("Ctrl+PageUp");
                                handled = true;
                            }
                            0x22 => {
                                // Page Down: next tab, as in Chrome.
                                shortcut = Some("Ctrl+PageDown");
                                handled = true;
                            }
                            0x31 => {
                                shortcut = Some("Ctrl+1");
                                handled = true;
                            }
                            0x32 => {
                                shortcut = Some("Ctrl+2");
                                handled = true;
                            }
                            0x33 => {
                                shortcut = Some("Ctrl+3");
                                handled = true;
                            }
                            0x34 => {
                                shortcut = Some("Ctrl+4");
                                handled = true;
                            }
                            0x35 => {
                                shortcut = Some("Ctrl+5");
                                handled = true;
                            }
                            0x36 => {
                                shortcut = Some("Ctrl+6");
                                handled = true;
                            }
                            0x37 => {
                                shortcut = Some("Ctrl+7");
                                handled = true;
                            }
                            0x38 => {
                                shortcut = Some("Ctrl+8");
                                handled = true;
                            }
                            0x39 => {
                                shortcut = Some("Ctrl+9");
                                handled = true;
                            }
                            0xBB | 0x6B => {
                                // '+' or Numpad '+'
                                shortcut = Some("Ctrl+Plus");
                                handled = true;
                            }
                            0xBD | 0x6D => {
                                // '-' or Numpad '-'
                                shortcut = Some("Ctrl+Minus");
                                handled = true;
                            }
                            0x30 | 0x60 => {
                                // '0' or Numpad '0'
                                shortcut = Some("Ctrl+Zero");
                                handled = true;
                            }
                            _ => {}
                        }
                    } else if alt && !ctrl {
                        if shift && vkey == 0x50 {
                            // Fill the only saved Pip password for this exact website.
                            shortcut = Some("Alt+Shift+P");
                            handled = true;
                        } else if !shift {
                            match vkey {
                                0x25 => {
                                    // Left Arrow
                                    shortcut = Some("Alt+Left");
                                    handled = true;
                                }
                                0x27 => {
                                    // Right Arrow
                                    shortcut = Some("Alt+Right");
                                    handled = true;
                                }
                                0x24 => {
                                    // Home
                                    shortcut = Some("Alt+Home");
                                    handled = true;
                                }
                                _ => {}
                            }
                        }
                    } else if let Some(browser_key) = unmodified_browser_key(ctrl, alt, vkey) {
                        shortcut = Some(browser_key);
                        handled = true;
                    }

                    if handled {
                        let _ = unsafe { args.SetHandled(true) };
                        if let Some(s) = shortcut {
                            let _ = proxy.send_event(crate::BrowserEvent::Shortcut(
                                window_id,
                                s.to_string(),
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }));

    let mut token = Default::default();
    let _ = unsafe { controller.add_AcceleratorKeyPressed(&handler, &mut token) };
}

#[cfg(target_os = "windows")]
pub fn attach_navigation_events(
    webview: &WebView,
    window_id: winit::window::WindowId,
    tab_id: evergreen_core::tabs::TabId,
    proxy: EventLoopProxy<crate::BrowserEvent>,
    allowed_cert_hosts: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    blocker: std::rc::Rc<std::cell::RefCell<crate::adblock::Blocker>>,
    popup_opener_url: Option<String>,
) {
    use webview2_com::{
        DocumentTitleChangedEventHandler, HistoryChangedEventHandler,
        IsDocumentPlayingAudioChangedEventHandler, NavigationCompletedEventHandler,
        NavigationStartingEventHandler, SourceChangedEventHandler,
    };
    use wry::WebViewExtWindows;

    // WebView2 raises the top-level NavigationStarting event for the document but
    // iframe navigations have their own event source. Subscribe to every frame,
    // including nested frames, so a redirect inside an ad iframe cannot evade the
    // same narrow cross-site destination checks used for top-level redirects.
    fn attach_frame_ad_navigation_filter(
        frame: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Frame,
        top_level: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
        blocker: std::rc::Rc<std::cell::RefCell<crate::adblock::Blocker>>,
        popup_navigation_context: std::rc::Rc<
            std::cell::RefCell<crate::adblock::PopupNavigationContext>,
        >,
    ) {
        use webview2_com::{
            FrameChildFrameCreatedEventHandler, FrameNavigationStartingEventHandler,
            Microsoft::Web::WebView2::Win32::{ICoreWebView2Frame2, ICoreWebView2Frame7},
        };

        if let Ok(frame2) = frame.cast::<ICoreWebView2Frame2>() {
            let navigation_blocker = blocker.clone();
            let source_core = top_level.clone();
            let popup_context_for_frame = popup_navigation_context.clone();
            let navigation_handler =
                FrameNavigationStartingEventHandler::create(Box::new(move |_sender, args| {
                    let Some(args) = args else {
                        return Ok(());
                    };
                    let mut raw_uri = windows::core::PWSTR::null();
                    if unsafe { args.Uri(&mut raw_uri) }.is_err() || raw_uri.is_null() {
                        return Ok(());
                    }
                    let target = webview2_com::take_pwstr(raw_uri);
                    let mut raw_source = windows::core::PWSTR::null();
                    let source = if unsafe { source_core.Source(&mut raw_source) }.is_ok()
                        && !raw_source.is_null()
                    {
                        webview2_com::take_pwstr(raw_source)
                    } else {
                        String::new()
                    };
                    let popup_opener = popup_context_for_frame.borrow().opener_for_source(&source);
                    let blocked = if let Some(popup_opener) = popup_opener.as_deref() {
                        navigation_blocker
                            .borrow()
                            .should_block_frame_navigation_with_popup_context(
                                &target,
                                &source,
                                Some(popup_opener),
                            )
                    } else {
                        navigation_blocker
                            .borrow()
                            .should_block_frame_navigation(&target, &source)
                    };
                    if blocked {
                        let _ = unsafe { args.SetCancel(true) };
                        navigation_blocker.borrow_mut().blocked += 1;
                    }
                    Ok(())
                }));
            let mut navigation_token = Default::default();
            if let Err(error) =
                unsafe { frame2.add_NavigationStarting(&navigation_handler, &mut navigation_token) }
            {
                eprintln!("Could not attach iframe ad navigation filter: {error}");
            }
        }

        // Frame7 is available on current WebView2 runtimes. Recursively observe
        // descendants where supported; older runtimes still get the frame's own
        // NavigationStarting protection above.
        if let Ok(frame7) = frame.cast::<ICoreWebView2Frame7>() {
            let child_blocker = blocker;
            let child_source = top_level;
            let child_popup_context = popup_navigation_context;
            let child_handler =
                FrameChildFrameCreatedEventHandler::create(Box::new(move |_sender, args| {
                    if let Some(child) = args.and_then(|args| unsafe { args.Frame() }.ok()) {
                        attach_frame_ad_navigation_filter(
                            child,
                            child_source.clone(),
                            child_blocker.clone(),
                            child_popup_context.clone(),
                        );
                    }
                    Ok(())
                }));
            let mut child_token = Default::default();
            if let Err(error) = unsafe { frame7.add_FrameCreated(&child_handler, &mut child_token) }
            {
                eprintln!("Could not attach nested iframe ad navigation filter: {error}");
            }
        }
    }

    if let Ok(core) = unsafe { webview.controller().CoreWebView2() } {
        let popup_navigation_context = std::rc::Rc::new(std::cell::RefCell::new(
            crate::adblock::PopupNavigationContext::from_opener(popup_opener_url),
        ));
        if popup_navigation_context.borrow().is_pending() {
            let completion_context = popup_navigation_context.clone();
            let navigation_completed =
                NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
                    let Some(args) = args else {
                        return Ok(());
                    };
                    let mut success = windows::core::BOOL(0);
                    if unsafe { args.IsSuccess(&mut success) }.is_err() || !success.as_bool() {
                        return Ok(());
                    }
                    let Some(sender) = sender else {
                        return Ok(());
                    };
                    let mut raw_source = windows::core::PWSTR::null();
                    if unsafe { sender.Source(&mut raw_source) }.is_err() || raw_source.is_null() {
                        return Ok(());
                    }
                    let source = webview2_com::take_pwstr(raw_source);
                    completion_context
                        .borrow_mut()
                        .navigation_committed(&source);
                    Ok(())
                }));
            let mut completion_token = Default::default();
            let _ = unsafe {
                core.add_NavigationCompleted(&navigation_completed, &mut completion_token)
            };
        }

        if let Ok(core8) =
            core.cast::<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_8>()
        {
            let proxy_audio = proxy.clone();
            let audio_handler = IsDocumentPlayingAudioChangedEventHandler::create(Box::new(
                move |sender, _args| {
                    if let Some(sender) = sender {
                        if let Ok(core8) = sender.cast::<
                            webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_8,
                        >() {
                            let mut playing = windows::core::BOOL(0);
                            if unsafe { core8.IsDocumentPlayingAudio(&mut playing) }.is_ok() {
                                let _ = proxy_audio.send_event(
                                    crate::BrowserEvent::TabAudioChanged(
                                        window_id,
                                        tab_id,
                                        playing.as_bool(),
                                    ),
                                );
                            }
                        }
                    }
                    Ok(())
                },
            ));
            let mut audio_token = Default::default();
            let _ = unsafe {
                core8.add_IsDocumentPlayingAudioChanged(&audio_handler, &mut audio_token)
            };
        }
        // 1. History Changed
        let proxy_history = proxy.clone();
        let history_handler = HistoryChangedEventHandler::create(Box::new(move |sender, _args| {
            let Some(core_history) = sender else {
                return Ok(());
            };
            let mut can_back = windows::core::BOOL(0);
            let mut can_forward = windows::core::BOOL(0);
            unsafe {
                let _ = core_history.CanGoBack(&mut can_back);
                let _ = core_history.CanGoForward(&mut can_forward);
            }
            let mut uri_pwstr = windows::core::PWSTR::null();
            if unsafe { core_history.Source(&mut uri_pwstr) }.is_ok() && !uri_pwstr.is_null() {
                let uri_str = webview2_com::take_pwstr(uri_pwstr);
                if !uri_str.is_empty()
                    && !uri_str.starts_with("data:text/html")
                    && uri_str != "about:blank"
                    && uri_str != "aboutblank"
                {
                    let _ = proxy_history.send_event(crate::BrowserEvent::TabNavigated(
                        window_id, tab_id, uri_str,
                    ));
                }
            }
            let mut title_pwstr = windows::core::PWSTR::null();
            if unsafe { core_history.DocumentTitle(&mut title_pwstr) }.is_ok()
                && !title_pwstr.is_null()
            {
                let title_str = webview2_com::take_pwstr(title_pwstr);
                if !title_str.is_empty() {
                    let _ = proxy_history.send_event(crate::BrowserEvent::TabTitleChanged(
                        window_id, tab_id, title_str,
                    ));
                }
            }
            let _ = proxy_history.send_event(crate::BrowserEvent::HistoryChanged(
                window_id,
                tab_id,
                can_back.as_bool(),
                can_forward.as_bool(),
            ));
            Ok(())
        }));
        let mut token_h = Default::default();
        let _ = unsafe { core.add_HistoryChanged(&history_handler, &mut token_h) };

        // 2. Source Changed (instantaneous URI updates on redirect, SPA pushState, back/forward)
        let proxy_source = proxy.clone();
        let source_handler = SourceChangedEventHandler::create(Box::new(move |sender, _args| {
            let Some(core_source) = sender else {
                return Ok(());
            };
            let mut uri_pwstr = windows::core::PWSTR::null();
            if unsafe { core_source.Source(&mut uri_pwstr) }.is_ok() && !uri_pwstr.is_null() {
                let uri_str = webview2_com::take_pwstr(uri_pwstr);
                if !uri_str.is_empty()
                    && !uri_str.starts_with("data:text/html")
                    && uri_str != "about:blank"
                    && uri_str != "aboutblank"
                {
                    let _ = proxy_source.send_event(crate::BrowserEvent::TabNavigated(
                        window_id, tab_id, uri_str,
                    ));
                }
            }
            let mut title_pwstr = windows::core::PWSTR::null();
            if unsafe { core_source.DocumentTitle(&mut title_pwstr) }.is_ok()
                && !title_pwstr.is_null()
            {
                let title_str = webview2_com::take_pwstr(title_pwstr);
                if !title_str.is_empty() {
                    let _ = proxy_source.send_event(crate::BrowserEvent::TabTitleChanged(
                        window_id, tab_id, title_str,
                    ));
                }
            }
            // Also update history state on source change
            let mut can_back = windows::core::BOOL(0);
            let mut can_forward = windows::core::BOOL(0);
            unsafe {
                let _ = core_source.CanGoBack(&mut can_back);
                let _ = core_source.CanGoForward(&mut can_forward);
            }
            let _ = proxy_source.send_event(crate::BrowserEvent::HistoryChanged(
                window_id,
                tab_id,
                can_back.as_bool(),
                can_forward.as_bool(),
            ));
            Ok(())
        }));
        let mut token_s = Default::default();
        let _ = unsafe { core.add_SourceChanged(&source_handler, &mut token_s) };

        // Popup filters also need to catch scripts that replace the current tab with an
        // advertising destination instead of requesting a new window.
        let navigation_blocker = blocker.clone();
        let popup_context_for_navigation = popup_navigation_context.clone();
        let proxy_blocked_navigation = proxy.clone();
        let navigation_handler =
            NavigationStartingEventHandler::create(Box::new(move |sender, args| {
                if let Some(args) = args {
                    let mut uri = windows::core::PWSTR::null();
                    if unsafe { args.Uri(&mut uri) }.is_ok() && !uri.is_null() {
                        let target = webview2_com::take_pwstr(uri);
                        let source = sender
                            .and_then(|sender| {
                                let mut raw = windows::core::PWSTR::null();
                                unsafe { sender.Source(&mut raw) }.ok()?;
                                (!raw.is_null()).then(|| webview2_com::take_pwstr(raw))
                            })
                            .unwrap_or_default();
                        let popup_opener_url = popup_context_for_navigation
                            .borrow()
                            .opener_for_source(&source);
                        let explicit_navigation = navigation_blocker
                            .borrow_mut()
                            .consume_explicit_navigation_ticket(tab_id.0, &target, &source);
                        let blocked = if explicit_navigation {
                            false
                        } else if let Some(popup_opener) = popup_opener_url.as_deref() {
                            navigation_blocker
                                .borrow()
                                .should_block_navigation_with_popup_context(
                                    &target,
                                    &source,
                                    Some(popup_opener),
                                )
                        } else {
                            navigation_blocker
                                .borrow()
                                .should_block_navigation(&target, &source)
                        };
                        if blocked {
                            let _ = unsafe { args.SetCancel(true) };
                            navigation_blocker.borrow_mut().blocked += 1;
                            let _ = proxy_blocked_navigation.send_event(
                                crate::BrowserEvent::TabNavigated(window_id, tab_id, source),
                            );
                        }
                    }
                }
                Ok(())
            }));
        let mut token_n = Default::default();
        if let Err(error) =
            unsafe { core.add_NavigationStarting(&navigation_handler, &mut token_n) }
        {
            eprintln!("Could not attach current-tab ad navigation filter: {error}");
        }

        if let Ok(core4) =
            core.cast::<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_4>()
        {
            let frame_blocker = blocker.clone();
            let frame_source = core.clone();
            let frame_popup_context = popup_navigation_context.clone();
            let frame_handler =
                webview2_com::FrameCreatedEventHandler::create(Box::new(move |_sender, args| {
                    if let Some(frame) = args.and_then(|args| unsafe { args.Frame() }.ok()) {
                        attach_frame_ad_navigation_filter(
                            frame,
                            frame_source.clone(),
                            frame_blocker.clone(),
                            frame_popup_context.clone(),
                        );
                    }
                    Ok(())
                }));
            let mut frame_token = Default::default();
            if let Err(error) = unsafe { core4.add_FrameCreated(&frame_handler, &mut frame_token) }
            {
                eprintln!("Could not attach iframe ad navigation filter: {error}");
            }
        }

        // 3. Document Title Changed (instantaneous title updates)
        let proxy_title = proxy.clone();
        let title_handler =
            DocumentTitleChangedEventHandler::create(Box::new(move |sender, _args| {
                let Some(core_title) = sender else {
                    return Ok(());
                };
                let mut title_pwstr = windows::core::PWSTR::null();
                if unsafe { core_title.DocumentTitle(&mut title_pwstr) }.is_ok()
                    && !title_pwstr.is_null()
                {
                    let title_str = webview2_com::take_pwstr(title_pwstr);
                    if !title_str.is_empty() {
                        let _ = proxy_title.send_event(crate::BrowserEvent::TabTitleChanged(
                            window_id, tab_id, title_str,
                        ));
                    }
                }
                Ok(())
            }));
        let mut token_t = Default::default();
        let _ = unsafe { core.add_DocumentTitleChanged(&title_handler, &mut token_t) };

        // 4. New Window Requested (target="_blank" link clicks, e.g. Garry Tan YC profile)
        let proxy_new_win = proxy.clone();
        let popup_blocker = blocker.clone();
        let popup_context_for_new_window = popup_navigation_context.clone();
        let new_window_handler =
            webview2_com::NewWindowRequestedEventHandler::create(Box::new(move |sender, args| {
                if let Some(args) = args {
                    let mut uri_pwstr = windows::core::PWSTR::null();
                    let uri_result = unsafe { args.Uri(&mut uri_pwstr) };
                    let uri_str = if uri_result.is_ok() && !uri_pwstr.is_null() {
                        webview2_com::take_pwstr(uri_pwstr)
                    } else {
                        String::new()
                    };
                    // An unhandled NewWindowRequested with no NewWindow lets WebView2
                    // create an unmanaged popup. Treat an empty target as about:blank
                    // so normal user-initiated blank windows still become Pip tabs and
                    // non-user-initiated ones pass through the existing popup blocker.
                    let uri_str = if uri_str.is_empty() {
                        "about:blank".to_string()
                    } else {
                        uri_str
                    };
                    let source = sender
                        .and_then(|sender| {
                            let mut raw = windows::core::PWSTR::null();
                            unsafe { sender.Source(&mut raw) }.ok()?;
                            (!raw.is_null()).then(|| webview2_com::take_pwstr(raw))
                        })
                        .unwrap_or_default();
                    let popup_opener_url = popup_context_for_new_window
                        .borrow()
                        .opener_for_source(&source);
                    let mut user_initiated = windows::core::BOOL(0);
                    let _ = unsafe { args.IsUserInitiated(&mut user_initiated) };
                    let blocked = popup_blocker
                        .borrow()
                        .should_block_popup_with_popup_context(
                            &uri_str,
                            &source,
                            popup_opener_url.as_deref(),
                            user_initiated.as_bool(),
                        );
                    if blocked {
                        let _ = unsafe { args.SetHandled(true) };
                        popup_blocker.borrow_mut().blocked += 1;
                        return Ok(());
                    }
                    let _ = unsafe { args.SetHandled(true) };
                    if uri_str == "about:blank"
                        && source.parse::<wry::http::Uri>().ok().is_some_and(|uri| {
                            matches!(uri.scheme_str(), Some("http" | "https"))
                                && uri.host().is_some()
                        })
                    {
                        let _ = proxy_new_win.send_event(crate::BrowserEvent::PopupTabRequested {
                            window_id,
                            url: uri_str,
                            opener_url: source,
                        });
                    } else {
                        let _ = proxy_new_win.send_event(crate::BrowserEvent::Ipc(
                            window_id,
                            evergreen_core::ipc::UiToHostMessage::CreateTab { url: Some(uri_str) },
                        ));
                    }
                }
                Ok(())
            }));
        let mut token_nw = Default::default();
        let _ = unsafe { core.add_NewWindowRequested(&new_window_handler, &mut token_nw) };

        // 5. Site Permission Requested
        let proxy_perm = proxy.clone();
        let perm_handler = webview2_com::PermissionRequestedEventHandler::create(Box::new(
            move |_sender, args| {
                if let Some(args) = args {
                    let deferral = unsafe { args.GetDeferral() }.ok();
                    let perm_id = NEXT_PERMISSION_ID.fetch_add(1, Ordering::SeqCst);

                    let mut uri_pwstr = windows::core::PWSTR::null();
                    let _ = unsafe { args.Uri(&mut uri_pwstr) };
                    let uri_str = if !uri_pwstr.is_null() {
                        webview2_com::take_pwstr(uri_pwstr)
                    } else {
                        String::new()
                    };
                    let mut kind =
                        webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PERMISSION_KIND(
                            0,
                        );
                    let _ = unsafe { args.PermissionKind(&mut kind) };
                    let kind_str = match kind.0 {
                        1 => "Microphone",
                        2 => "Camera",
                        3 => "Geolocation",
                        4 => "Notifications",
                        5 => "Other Sensors",
                        6 => "Clipboard Read",
                        _ => "Site Permission",
                    };

                    if let Some(d) = deferral {
                        PERMISSION_STORE.with(|store| {
                            store.borrow_mut().insert(
                                perm_id,
                                ActivePermission {
                                    window: window_id,
                                    tab: tab_id,
                                    args: args.clone(),
                                    deferral: d,
                                },
                            )
                        });
                    }

                    let host = crate::extract_host(&uri_str).unwrap_or_else(|| uri_str.clone());
                    let _ = proxy_perm.send_event(crate::BrowserEvent::PermissionPrompt {
                        window_id,
                        permission_id: perm_id,
                        origin: host,
                        permission_kind: kind_str.to_string(),
                    });
                }
                Ok(())
            },
        ));
        let mut token_p = Default::default();
        let _ = unsafe { core.add_PermissionRequested(&perm_handler, &mut token_p) };

        // Process Failed Handler (detect GPU/renderer reset on sleep/standby)
        let proxy_fail = proxy.clone();
        let process_failed_handler = webview2_com::ProcessFailedEventHandler::create(Box::new(
            move |_sender, args| {
                if let Some(args) = args {
                    let mut kind = webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PROCESS_FAILED_KIND(0);
                    let _ = unsafe { args.ProcessFailedKind(&mut kind) };
                    eprintln!(
                        "[PROCESS FAILED] WebView2 process failed kind: {:?}",
                        kind.0
                    );
                    let _ = proxy_fail
                        .send_event(crate::BrowserEvent::TabCrashed(window_id, tab_id, kind.0));
                }
                Ok(())
            },
        ));
        let mut token_pf = Default::default();
        let _ = unsafe { core.add_ProcessFailed(&process_failed_handler, &mut token_pf) };

        // 6. Strict TLS Warning Interstitial (ServerCertificateErrorDetected)
        if let Ok(core14) =
            core.cast::<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_14>()
        {
            let proxy_cert = proxy.clone();
            let cert_allowed = allowed_cert_hosts.clone();
            let cert_handler = webview2_com::ServerCertificateErrorDetectedEventHandler::create(
                Box::new(move |_sender, args| {
                    if let Some(args) = args {
                        let mut error_status = webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_WEB_ERROR_STATUS(0);
                        let _ = unsafe { args.ErrorStatus(&mut error_status) };
                        let mut uri_pwstr = windows::core::PWSTR::null();
                        let _ = unsafe { args.RequestUri(&mut uri_pwstr) };
                        let uri_str = if !uri_pwstr.is_null() {
                            webview2_com::take_pwstr(uri_pwstr)
                        } else {
                            String::new()
                        };

                        let host = crate::extract_host(&uri_str).unwrap_or_else(|| uri_str.clone());
                        if cert_allowed
                            .lock()
                            .map(|set| set.contains(&host))
                            .unwrap_or(false)
                        {
                            eprintln!(
                                "[TLS BYPASS] Allowing user-whitelisted certificate for: {}",
                                host
                            );
                            let _ = unsafe {
                                args.SetAction(webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_ALWAYS_ALLOW)
                            };
                            return Ok(());
                        }

                        // Otherwise cancel the untrusted request and show interstitial
                        let _ = unsafe {
                            args.SetAction(webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_CANCEL)
                        };
                        let _ =
                            proxy_cert.send_event(crate::BrowserEvent::ServerCertificateError {
                                window_id,
                                tab_id,
                                request_uri: uri_str,
                                error_status: error_status.0,
                            });
                    }
                    Ok(())
                }),
            );
            let mut token_cert = Default::default();
            let _ = unsafe {
                core14.add_ServerCertificateErrorDetected(&cert_handler, &mut token_cert)
            };
        }
    }
}

#[cfg(target_os = "windows")]
pub fn show_save_file_dialog(
    default_filename: &str,
    dialog_title: &str,
) -> Option<std::path::PathBuf> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    use windows::Win32::UI::Shell::{FileSaveDialog, IFileSaveDialog, SIGDN_FILESYSPATH};

    unsafe {
        let dialog: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_ALL).ok()?;
        let wname: Vec<u16> = default_filename
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let _ = dialog.SetFileName(PCWSTR(wname.as_ptr()));
        let title: Vec<u16> = dialog_title
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let _ = dialog.SetTitle(PCWSTR(title.as_ptr()));

        if dialog.Show(None).is_ok() {
            if let Ok(item) = dialog.GetResult() {
                if let Ok(path_pwstr) = item.GetDisplayName(SIGDN_FILESYSPATH) {
                    if !path_pwstr.is_null() {
                        let path_str = webview2_com::take_pwstr(path_pwstr);
                        return Some(std::path::PathBuf::from(path_str));
                    }
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
pub fn attach_accelerator_keys(
    _webview: &WebView,
    _window_id: winit::window::WindowId,
    _proxy: EventLoopProxy<crate::BrowserEvent>,
) {
}

#[cfg(not(target_os = "windows"))]
pub fn attach_navigation_events(
    _webview: &WebView,
    _window_id: winit::window::WindowId,
    _tab_id: evergreen_core::tabs::TabId,
    _proxy: EventLoopProxy<crate::BrowserEvent>,
    _allowed_cert_hosts: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    _blocker: std::rc::Rc<std::cell::RefCell<crate::adblock::Blocker>>,
    _popup_opener_url: Option<String>,
) {
}

#[cfg(test)]
mod find_navigation_shortcut_tests {
    use super::{devtools_shortcut, find_navigation_shortcut, unmodified_browser_key};

    #[test]
    fn f11_is_forwarded_as_an_unmodified_browser_shortcut() {
        assert_eq!(unmodified_browser_key(false, false, 0x7A), Some("F11"));
        assert_eq!(unmodified_browser_key(true, false, 0x7A), None);
        assert_eq!(unmodified_browser_key(false, true, 0x7A), None);
    }

    #[test]
    fn ctrl_g_and_f3_select_next_or_previous_find_match() {
        assert_eq!(
            find_navigation_shortcut(true, false, false, 0x47),
            Some("FindNext")
        );
        assert_eq!(
            find_navigation_shortcut(true, true, false, 0x47),
            Some("FindPrevious")
        );
        assert_eq!(
            find_navigation_shortcut(false, false, false, 0x72),
            Some("FindNext")
        );
        assert_eq!(
            find_navigation_shortcut(false, true, false, 0x72),
            Some("FindPrevious")
        );
    }

    #[test]
    fn unrelated_modifier_combinations_do_not_trigger_find_navigation() {
        assert_eq!(find_navigation_shortcut(true, false, true, 0x47), None);
        assert_eq!(find_navigation_shortcut(false, false, true, 0x72), None);
        assert_eq!(find_navigation_shortcut(true, false, false, 0x46), None);
    }

    #[test]
    fn chrome_devtools_shortcuts_include_control_shift_i_and_j() {
        assert_eq!(
            devtools_shortcut(true, true, false, 0x49),
            Some("Ctrl+Shift+I")
        );
        assert_eq!(
            devtools_shortcut(true, true, false, 0x4A),
            Some("Ctrl+Shift+J")
        );
        assert_eq!(devtools_shortcut(true, false, false, 0x49), None);
        assert_eq!(devtools_shortcut(true, true, true, 0x49), None);
    }
}
