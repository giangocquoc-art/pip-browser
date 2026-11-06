//! Pip Setup & Standalone Uninstaller.
//! Built with modern Fluent acrylic aesthetics, SVG icons, and WebView2 rendering.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod installer_ui;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Icon, Window, WindowAttributes, WindowId};
use wry::WebViewBuilder;

const CREATE_NO_WINDOW: u32 = 0x08000000;

const APP_NAME: &str = "Pip";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const PUBLISHER: &str = "Pip contributors";
const EXE_NAME: &str = "Pip.exe";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Pip";

// Embedded assets
const BROWSER_PAYLOAD: &[u8] = include_bytes!("../assets/evergreen-browser.exe");
const LOGO_BASE64: &str = include_str!("../../app/ui/logo.b64");
const SOURCE_PAYLOAD: &[u8] = include_bytes!("../assets/Pip-source.zip");
const RUNTIME_BOOTSTRAPPER: &[u8] = include_bytes!("../assets/MicrosoftEdgeWebview2Setup.exe");
const NOTICE: &str = include_str!("../../../THIRD_PARTY.md");
const README: &str = include_str!("../../../PIP_README.md");
const LICENSE: &str = include_str!("../../../LICENSE");
const MARKER: &str = "Pip per-user installation v1";
const ICON_RGBA: &[u8] = include_bytes!("../../app/ui/icon_64.rgba");

fn get_install_directory() -> PathBuf {
    if let Ok(local_appdata) = std::env::var("LOCALAPPDATA") {
        PathBuf::from(local_appdata).join("Programs").join("Pip")
    } else if let Ok(userprofile) = std::env::var("USERPROFILE") {
        PathBuf::from(userprofile)
            .join("AppData")
            .join("Local")
            .join("Programs")
            .join("Pip")
    } else {
        panic!("Không xác định được thư mục cài đặt của người dùng")
    }
}

fn get_start_menu_shortcut_path() -> Option<PathBuf> {
    if let Ok(appdata) = std::env::var("APPDATA") {
        Some(
            PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join(format!("{}.lnk", APP_NAME)),
        )
    } else {
        None
    }
}

fn get_desktop_shortcut_path() -> Option<PathBuf> {
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        Some(
            PathBuf::from(userprofile)
                .join("Desktop")
                .join(format!("{}.lnk", APP_NAME)),
        )
    } else {
        None
    }
}

fn create_shortcut(
    target_exe: &Path,
    shortcut_path: &Path,
    icon_path: &Path,
) -> Result<(), String> {
    if let Some(parent) = shortcut_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let ps_script = format!(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath = '{}'; $s.IconLocation = '{},0'; $s.Description = '{}'; $s.Save()",
        shortcut_path.display().to_string().replace('\'', "''"),
        target_exe.display().to_string().replace('\'', "''"),
        icon_path.display().to_string().replace('\'', "''"),
        APP_NAME
    );

    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps_script]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to invoke PowerShell: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn register_uninstall_entry(install_dir: &Path, uninstall_exe: &Path) -> Result<(), String> {
    let uninst_cmd = format!("\"{}\" --uninstall", uninstall_exe.display());
    let target_exe = install_dir.join(EXE_NAME);
    let display_icon = target_exe.display().to_string();

    let ps_script = format!(
        "New-Item -Path 'HKCU:\\{}' -Force | Out-Null; \
         Set-ItemProperty -Path 'HKCU:\\{}' -Name 'DisplayName' -Value '{}'; \
         Set-ItemProperty -Path 'HKCU:\\{}' -Name 'DisplayVersion' -Value '{}'; \
         Set-ItemProperty -Path 'HKCU:\\{}' -Name 'Publisher' -Value '{}'; \
         Set-ItemProperty -Path 'HKCU:\\{}' -Name 'DisplayIcon' -Value '{}'; \
         Set-ItemProperty -Path 'HKCU:\\{}' -Name 'UninstallString' -Value '{}'; \
         Set-ItemProperty -Path 'HKCU:\\{}' -Name 'InstallLocation' -Value '{}'; \
         Set-ItemProperty -Path 'HKCU:\\{}' -Name 'NoModify' -Value 1 -Type DWord; \
         Set-ItemProperty -Path 'HKCU:\\{}' -Name 'NoRepair' -Value 1 -Type DWord",
        UNINSTALL_KEY,
        UNINSTALL_KEY,
        APP_NAME,
        UNINSTALL_KEY,
        APP_VERSION,
        UNINSTALL_KEY,
        PUBLISHER,
        UNINSTALL_KEY,
        display_icon.replace('\'', "''"),
        UNINSTALL_KEY,
        uninst_cmd.replace('\'', "''"),
        UNINSTALL_KEY,
        install_dir.display().to_string().replace('\'', "''"),
        UNINSTALL_KEY,
        UNINSTALL_KEY
    );

    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps_script]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
}

fn unregister_uninstall_entry() {
    let ps_script = format!(
        "Remove-Item -Path 'HKCU:\\{}' -Recurse -Force -ErrorAction SilentlyContinue",
        UNINSTALL_KEY
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps_script]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let _ = cmd.output();
}

#[derive(Debug)]
enum InstallerEvent {
    Progress { pct: u32, text: String },
    InstallComplete,
    UninstallComplete,
    Failed(String),
    Exit,
}

struct InstallerApp {
    uninstalled: bool,
    is_uninstall: bool,
    proxy: EventLoopProxy<InstallerEvent>,
    window: Option<Arc<Window>>,
    webview: Option<wry::WebView>,
    is_installing: Arc<AtomicBool>,
}

impl ApplicationHandler<InstallerEvent> for InstallerApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = WindowAttributes::default()
            .with_title(if self.is_uninstall {
                "Gỡ cài đặt Pip"
            } else {
                "Cài đặt Pip"
            })
            .with_inner_size(LogicalSize::new(580.0, 500.0))
            .with_resizable(false)
            .with_theme(Some(winit::window::Theme::Light));

        let attrs = if let Ok(icon) = Icon::from_rgba(ICON_RGBA.to_vec(), 64, 64) {
            attrs.with_window_icon(Some(icon))
        } else {
            attrs
        };

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("Failed to create installer window: {}", e);
                event_loop.exit();
                return;
            }
        };

        #[cfg(target_os = "windows")]
        {
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            #[link(name = "dwmapi")]
            unsafe extern "system" {
                fn DwmSetWindowAttribute(
                    hwnd: isize,
                    attribute: u32,
                    value: *const std::ffi::c_void,
                    size: u32,
                ) -> i32;
            }
            if let Ok(handle) = window.window_handle() {
                if let RawWindowHandle::Win32(handle) = handle.as_raw() {
                    let hwnd = handle.hwnd.get();
                    let light_caption: i32 = 0;
                    let caption_color: u32 = 0x00e5f0ff; // RGB(255, 240, 229)
                    let caption_text: u32 = 0x002e2017; // RGB(23, 32, 46)
                    unsafe {
                        let _ = DwmSetWindowAttribute(
                            hwnd,
                            20,
                            &light_caption as *const _ as _,
                            std::mem::size_of_val(&light_caption) as u32,
                        );
                        let _ = DwmSetWindowAttribute(
                            hwnd,
                            35,
                            &caption_color as *const _ as _,
                            std::mem::size_of_val(&caption_color) as u32,
                        );
                        let _ = DwmSetWindowAttribute(
                            hwnd,
                            36,
                            &caption_text as *const _ as _,
                            std::mem::size_of_val(&caption_text) as u32,
                        );
                    }
                }
            }
        }

        // Center on screen
        if let Some(monitor) = window
            .primary_monitor()
            .or_else(|| window.available_monitors().next())
        {
            let m_size = monitor.size();
            let scale = monitor.scale_factor();
            let w_phys = (580.0 * scale) as u32;
            let h_phys = (500.0 * scale) as u32;
            let x = (m_size.width.saturating_sub(w_phys)) / 2;
            let y = (m_size.height.saturating_sub(h_phys)) / 2;
            window.set_outer_position(LogicalPosition::new(x as f64 / scale, y as f64 / scale));
        }

        let html = installer_ui::get_installer_html(LOGO_BASE64, self.is_uninstall);

        let proxy = self.proxy.clone();
        let is_installing = self.is_installing.clone();
        let is_uninstall_mode = self.is_uninstall;

        let webview_builder = WebViewBuilder::new()
            .with_background_color((255, 250, 246, 255))
            .with_navigation_handler(|url| {
                url == "about:blank" || url.starts_with("data:text/html")
            })
            .with_html(html)
            .with_ipc_handler(move |req: wry::http::Request<String>| {
                let msg = req.body();
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(msg) {
                    let action = val["action"].as_str().unwrap_or("");
                    match action {
                        "cancel" => {
                            let _ = proxy.send_event(InstallerEvent::Exit);
                        }
                        "accept"
                            if !is_uninstall_mode
                                && !is_installing.swap(true, Ordering::SeqCst) =>
                        {
                            let create_desktop = val["desktop"].as_bool().unwrap_or(true);
                            let create_start_menu = val["start_menu"].as_bool().unwrap_or(true);
                            let worker_proxy = proxy.clone();
                            std::thread::spawn(move || {
                                run_installer_worker(
                                    create_desktop,
                                    create_start_menu,
                                    worker_proxy,
                                );
                            });
                        }
                        "finish" => {
                            let launch = val["launch"].as_bool().unwrap_or(true);
                            if launch {
                                let install_dir = get_install_directory();
                                let target_exe = install_dir.join(EXE_NAME);
                                if target_exe.exists() {
                                    let _ = Command::new(&target_exe).spawn();
                                }
                            }
                            let _ = proxy.send_event(InstallerEvent::Exit);
                        }
                        "start_uninstall"
                            if is_uninstall_mode && !is_installing.swap(true, Ordering::SeqCst) =>
                        {
                            let remove_data = val["remove_data"].as_bool().unwrap_or(false);
                            let worker_proxy = proxy.clone();
                            std::thread::spawn(move || {
                                run_uninstaller_worker(remove_data, worker_proxy);
                            });
                        }
                        _ => {}
                    }
                }
            });

        match webview_builder.build(&*window) {
            Ok(wv) => {
                if self.is_uninstall {
                    let _ = wv.evaluate_script(
                        "if (window.initUninstallMode) { window.initUninstallMode(); }",
                    );
                }
                self.webview = Some(wv);
                self.window = Some(window);
            }
            Err(e) => {
                eprintln!("Failed to initialize WebView2 installer shell: {}", e);
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: InstallerEvent) {
        match event {
            InstallerEvent::Progress { pct, text } => {
                if let Some(wv) = &self.webview {
                    let script = format!("window.updateProgress({}, {:?});", pct, text);
                    let _ = wv.evaluate_script(&script);
                }
            }
            InstallerEvent::InstallComplete => {
                self.is_installing.store(false, Ordering::SeqCst);
                if let Some(wv) = &self.webview {
                    let _ = wv.evaluate_script("window.installComplete();");
                }
            }
            InstallerEvent::UninstallComplete => {
                self.uninstalled = true;
                self.is_installing.store(false, Ordering::SeqCst);
                if let Some(wv) = &self.webview {
                    let _ = wv.evaluate_script("window.uninstallComplete();");
                }
            }
            InstallerEvent::Failed(message) => {
                self.is_installing.store(false, Ordering::SeqCst);
                if let Some(view) = &self.webview {
                    let message = serde_json::to_string(&message).unwrap();
                    let _ = view.evaluate_script(&format!("window.installFailed({message});"));
                }
            }
            InstallerEvent::Exit => {
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::CloseRequested = event {
            if !self.is_installing.load(Ordering::SeqCst) {
                event_loop.exit();
            }
        }
    }
}

fn checked_install_directory() -> Result<PathBuf, String> {
    let directory = get_install_directory();
    if !directory.is_absolute() || directory.file_name().and_then(|s| s.to_str()) != Some("Pip") {
        return Err("Đường dẫn cài đặt Pip không hợp lệ.".into());
    }
    if let Ok(metadata) = std::fs::symlink_metadata(&directory) {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Không cài đặt hoặc xóa qua liên kết thư mục.".into());
        }
    }
    Ok(directory)
}

fn install(desktop: bool, start_menu: bool, register: bool) -> Result<(), String> {
    let directory = checked_install_directory()?;
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let target = directory.join(EXE_NAME);
    // Opening a running executable for writing fails on Windows; never kill sessions.
    if target.exists() {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(&target)
            .map_err(|_| "Pip đang chạy. Hãy đóng Pip rồi chạy lại bộ cài.".to_string())?;
    }
    let staging = directory.join("Pip.exe.new");
    std::fs::write(&staging, BROWSER_PAYLOAD).map_err(|e| e.to_string())?;
    std::fs::rename(staging, &target).map_err(|e| e.to_string())?;
    for (name, bytes) in [
        ("LICENSE", LICENSE.as_bytes()),
        ("THIRD_PARTY.md", NOTICE.as_bytes()),
        ("HUONG_DAN.md", README.as_bytes()),
        ("Pip-source.zip", SOURCE_PAYLOAD),
        (".pip-installed", MARKER.as_bytes()),
    ] {
        std::fs::write(directory.join(name), bytes).map_err(|e| e.to_string())?;
    }
    let uninstaller = directory.join("uninstall.exe");
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    if current != uninstaller {
        std::fs::copy(current, &uninstaller).map_err(|e| e.to_string())?;
    }
    if desktop {
        if let Some(path) = get_desktop_shortcut_path() {
            create_shortcut(&target, &path, &target)?;
        }
    }
    if start_menu {
        if let Some(path) = get_start_menu_shortcut_path() {
            create_shortcut(&target, &path, &target)?;
        }
    }
    if register {
        register_uninstall_entry(&directory, &uninstaller)?;
    }
    Ok(())
}

fn uninstall(remove_data: bool, register: bool) -> Result<(), String> {
    let directory = checked_install_directory()?;
    if std::fs::read_to_string(directory.join(".pip-installed"))
        .ok()
        .as_deref()
        != Some(MARKER)
    {
        return Err("Không tìm thấy bản cài Pip hợp lệ tại thư mục này.".into());
    }
    let target = directory.join(EXE_NAME);
    if target.exists() {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(&target)
            .map_err(|_| "Pip đang chạy. Hãy đóng các cửa sổ Pip rồi thử gỡ lại.".to_string())?;
        std::fs::remove_file(target).map_err(|e| e.to_string())?;
    }
    for name in [
        "LICENSE",
        "THIRD_PARTY.md",
        "HUONG_DAN.md",
        "Pip-source.zip",
    ] {
        let path = directory.join(name);
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    for path in [get_desktop_shortcut_path(), get_start_menu_shortcut_path()]
        .into_iter()
        .flatten()
    {
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    if register {
        unregister_uninstall_entry();
    }
    if remove_data {
        let data = evergreen_core::env::resolve_data_directory(&directory, &[]);
        let expected = PathBuf::from(std::env::var("APPDATA").map_err(|e| e.to_string())?)
            .join("evergreen-browser");
        if !data.is_absolute() || data != expected {
            return Err("Không xóa dữ liệu ở đường dẫn ngoài hồ sơ Pip.".into());
        }
        if data.exists() {
            std::fs::remove_dir_all(data).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn run_installer_worker(desktop: bool, start_menu: bool, proxy: EventLoopProxy<InstallerEvent>) {
    let _ = proxy.send_event(InstallerEvent::Progress {
        pct: 30,
        text: "Đang cài Pip và tạo lối tắt...".into(),
    });
    match install(desktop, start_menu, true) {
        Ok(()) => {
            let _ = proxy.send_event(InstallerEvent::InstallComplete);
        }
        Err(error) => {
            let _ = proxy.send_event(InstallerEvent::Failed(error));
        }
    }
}
fn run_uninstaller_worker(remove_data: bool, proxy: EventLoopProxy<InstallerEvent>) {
    match uninstall(remove_data, true) {
        Ok(()) => {
            let _ = proxy.send_event(InstallerEvent::UninstallComplete);
        }
        Err(error) => {
            let _ = proxy.send_event(InstallerEvent::Failed(error));
        }
    }
}

// Move only the known uninstaller to OS temp, then clean the now empty install
// folder after the parent exits. No recursive deletion of the install directory.
fn schedule_cleanup() -> Result<(), String> {
    let directory = checked_install_directory()?;
    if directory.join(EXE_NAME).exists() {
        return Ok(());
    }
    let pid = std::process::id();
    let helper = std::env::temp_dir().join(format!("Pip-uninstall-{pid}.exe"));
    std::fs::copy(std::env::current_exe().map_err(|e| e.to_string())?, &helper)
        .map_err(|e| e.to_string())?;
    Command::new(helper)
        .args(["--cleanup", &pid.to_string()])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn cleanup(parent: u32) -> Result<(), String> {
    use windows::Win32::{
        Foundation::CloseHandle,
        System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
    };
    if let Ok(handle) = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, parent) } {
        let result = unsafe { WaitForSingleObject(handle, 60000) };
        unsafe {
            let _ = CloseHandle(handle);
        }
        if result.0 != 0 {
            return Err("Uninstaller is still running".into());
        }
    }
    let directory = checked_install_directory()?;
    if std::fs::read_to_string(directory.join(".pip-installed"))
        .ok()
        .as_deref()
        != Some(MARKER)
    {
        return Err("Invalid Pip installation marker".into());
    }
    if directory.join(EXE_NAME).exists() {
        return Err("Pip is still installed".into());
    }
    let _ = std::fs::remove_file(directory.join("uninstall.exe"));
    let _ = std::fs::remove_file(directory.join(".pip-installed"));
    let _ = std::fs::remove_dir(directory); // only succeeds if empty
    Ok(())
}

fn ensure_runtime(interactive: bool) -> Result<(), String> {
    if evergreen_core::env::detect_webview2_runtime().is_some() {
        return Ok(());
    }
    if !interactive {
        return Err("Cần cài WebView2 trước khi cài Pip bằng chế độ silent.".into());
    }
    use windows::{
        core::w,
        Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONINFORMATION, MB_YESNO},
    };
    let answer = unsafe {
        MessageBoxW(None,w!("Pip cần Microsoft WebView2 để mở trang web.\n\nCài WebView2 từ Microsoft bây giờ? Cần kết nối Internet."),w!("Cài đặt Pip"),MB_YESNO|MB_ICONINFORMATION)
    };
    if answer != IDYES {
        return Err("Chưa cài WebView2.".into());
    }
    let directory = std::env::temp_dir().join(format!("Pip-runtime-{}", std::process::id()));
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let bootstrapper = directory.join("MicrosoftEdgeWebview2Setup.exe");
    std::fs::write(&bootstrapper, RUNTIME_BOOTSTRAPPER).map_err(|e| e.to_string())?;
    let status = Command::new(&bootstrapper)
        .args(["/silent", "/install"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&bootstrapper);
    let _ = std::fs::remove_dir(&directory);
    if !status.success() || evergreen_core::env::detect_webview2_runtime().is_none() {
        return Err("Không cài được WebView2. Kiểm tra mạng rồi chạy lại bộ cài Pip.".into());
    }
    Ok(())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let cur_exe_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    let is_uninstall =
        args.iter().any(|a| a == "--uninstall") || cur_exe_name.contains("uninstall");

    if let Some(index) = args.iter().position(|a| a == "--cleanup") {
        let parent = args
            .get(index + 1)
            .ok_or("missing parent process")?
            .parse::<u32>()?;
        cleanup(parent)?;
        return Ok(());
    }
    if evergreen_core::env::is_process_elevated() {
        return Err(
            "Hãy chạy bộ cài Pip bằng tài khoản thường, không chọn Run as administrator.".into(),
        );
    }
    if args.iter().any(|a| a == "--silent") {
        let register = !args.iter().any(|a| a == "--no-register");
        if is_uninstall {
            uninstall(args.iter().any(|a| a == "--remove-data"), register)?;
        } else {
            ensure_runtime(false)?;
            let shortcuts = !args.iter().any(|a| a == "--no-shortcuts");
            install(shortcuts, shortcuts, register)?;
        }
        if is_uninstall {
            schedule_cleanup()?;
        }
        return Ok(());
    }
    ensure_runtime(true)?;

    let event_loop = EventLoop::<InstallerEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    let mut app = InstallerApp {
        uninstalled: false,
        is_uninstall,
        proxy,
        window: None,
        webview: None,
        is_installing: Arc::new(AtomicBool::new(false)),
    };

    event_loop.run_app(&mut app)?;
    if app.uninstalled {
        let _ = schedule_cleanup();
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("Pip Setup: {error}");
        if !std::env::args().any(|a| a == "--silent" || a == "--cleanup") {
            use windows::{
                core::{w, HSTRING},
                Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK},
            };
            unsafe {
                let _ = MessageBoxW(
                    None,
                    &HSTRING::from(error.to_string()),
                    w!("Cài đặt Pip"),
                    MB_OK | MB_ICONERROR,
                );
            }
        }
        std::process::exit(1);
    }
}
