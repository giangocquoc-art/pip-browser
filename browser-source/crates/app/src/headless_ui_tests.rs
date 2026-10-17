//! Exercise the real app/IPC/WebView2 flow with every native window hidden.
use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};
use winit::{event_loop::ControlFlow, platform::windows::EventLoopBuilderExtWindows};

struct Check {
    app: BrowserApp,
    main: Option<WindowId>,
    step: u8,
    pending: bool,
    next: Instant,
    deadline: Instant,
    url: String,
    error: Option<String>,
    complete: bool,
    draft_tab: Option<TabId>,
    restored_window: Option<WindowId>,
}
impl Check {
    fn query(&mut self, window: WindowId, chrome: bool, script: &str) {
        let win = &self.app.windows[&window];
        let view = if chrome {
            win.chrome_webview.as_ref()
        } else {
            win.tab_manager
                .active_tab_id()
                .and_then(|id| win.tabs.get(&id))
        }
        .unwrap();
        let proxy = self.app.proxy.clone();
        self.pending = true;
        view.evaluate_script_with_callback(script, move |value| {
            let _ = proxy.send_event(BrowserEvent::CommandDone(
                "__headless_check".into(),
                true,
                value,
            ));
        })
        .unwrap();
    }
    fn tick(&mut self, events: &ActiveEventLoop) -> Result<(), String> {
        let id = self.main.ok_or("No main window")?;
        if self
            .app
            .windows
            .values()
            .any(|w| w.window.is_visible() == Some(true))
        {
            return Err("A test window became visible".into());
        }
        if self.app.windows.values().any(|w| w.window.is_decorated()) {
            return Err("The native title bar is still separate from the Pip tab strip".into());
        }
        match self.step {
            0=>self.query(id,true,"(()=>{const okay=document.querySelector('.tab-strip').offsetHeight===40&&document.querySelector('.nav-bar').offsetHeight===48&&document.querySelectorAll('.window-controls .window-control').length===3;const button=document.getElementById('profileButton');button.click();const menu=document.getElementById('profilePopover');const menuOkay=!menu.hidden&&document.querySelectorAll('#profileList .profile-choice').length===1&&document.getElementById('profileCurrentName').textContent==='Cá nhân';showProfileCreate();document.getElementById('profileCreateName').value='Kiểm thử';document.getElementById('profileCreateForm').requestSubmit();document.getElementById('googleDnsButton').click();return okay&&menuOkay&&menu.hidden})()"),
            1=>{
                if !self.app.settings.google_dns{return Err("DNS toolbar did not save".into());}
                self.query(id,true,"(()=>{const okay=document.getElementById('googleDnsButton').title.includes('mở lại Pip');document.getElementById('googleDnsButton').click();postIpc('OpenSettings');return okay})()");
            }
            2=>self.query(id,false,"(()=>{const input=document.getElementById('askWhereToSaveToggle'),clear=document.getElementById('clearSiteDataButton');if(!input||!clear||!clear.textContent.includes('Xóa dữ liệu website')||!clear.getAttribute('onclick').includes('ClearSiteData'))return false;const before=input.checked;input.click();return before&&!input.checked&&typeof window.__pipSend==='function'})()"),
            3=>{
                if self.app.settings.downloads.ask_where_to_save {return Err("Settings checkbox did not reach native storage".into());}
                let disk=evergreen_core::settings::Settings::load_from_path(&get_settings_path()).map_err(|e|e.to_string())?;
                if disk.downloads.ask_where_to_save {return Err("Nested settings were not persisted".into());}
                self.app.handle_navigate(id,&self.url);self.step+=1;
            }
            4=>self.query(id,false,"(()=>{if(document.title!=='Pip native test')return false;localStorage.setItem('pip-secret','normal');document.cookie='pip_secret=normal; SameSite=Lax';return typeof window.__pipSend==='undefined'})()"),
            5=>self.query(id,true,"(()=>{document.getElementById('bookmarkButton').click();postIpc('OpenLibrary',{view:'bookmarks'});return true})()"),
            6=>self.query(id,false,"(()=>{const portable=document.getElementById('importBookmarksButton')?.dataset.action==='ImportBookmarks'&&document.getElementById('exportBookmarksButton')?.dataset.action==='ExportBookmarks';const check=kind=>{const input=document.getElementById(kind+'Search'),list=document.getElementById(kind+'List'),empty=document.getElementById(kind+'NoResults');if(!input||!list.textContent.includes('Pip native test'))return false;input.value='pip native';input.dispatchEvent(new Event('input'));const matched=list.querySelectorAll('.library-row').length===1;input.value='not found';input.dispatchEvent(new Event('input'));const emptyState=empty.hidden===false&&list.querySelectorAll('.library-row').length===0;input.value='';input.dispatchEvent(new Event('input'));return matched&&emptyState&&list.querySelectorAll('.library-row').length===1};const bookmarks=check('bookmarks');document.querySelector('.rail-link[data-view=history]').click();const history=check('history');const row=[...document.querySelectorAll('#historyList .library-row')].find(item=>item.textContent.includes('Pip native test'));row?.querySelector('button')?.click();return portable&&bookmarks&&history&&!!row})()"),
            7=>{
                let path=self.app.library_path(id).ok_or("Library path missing")?;
                let library=evergreen_core::library::Library::load(&path).map_err(|e|e.to_string())?;
                if library.history.iter().any(|page|page.url==self.url){return Err("Removing a history entry did not persist".into());}
                self.query(id,true,"(()=>{postIpc('OpenPrivateWindow');return true})()");
            }
            8=>{
                let private=self.app.windows.values().find(|w|w.private).ok_or("Private window missing")?.id;
                self.app.handle_navigate(private,&(self.url.clone()+"private"));self.step+=1;
            }
            9=>{
                let private=self.app.windows.values().find(|w|w.private).ok_or("Private window missing")?.id;
                self.query(private,false,"document.title==='Pip native test'&&localStorage.getItem('pip-secret')===null&&!document.cookie.includes('pip_secret')&&typeof window.__pipSend==='undefined'");
            }
            10=>{
                let path=self.app.library_path(id).unwrap();let library=evergreen_core::library::Library::load(&path).map_err(|e|e.to_string())?;
                if library.history.iter().any(|p|p.url.ends_with("/private")){return Err("Private history leaked".into());}
                self.app.handle_navigate(id,&(self.url.clone()+"download"));self.step+=1;
            }
            11=>{
                let path=self.app.settings.downloads.default_folder.join("native-test.txt");
                if std::fs::read(path).ok().as_deref()!=Some(b"Pip download test\n") {return Err("First download missing or incomplete".into());}
                self.app.handle_navigate(id,&(self.url.clone()+"download"));self.step+=1;
            }
            12=>{
                let win=&self.app.windows[&id];
                if win.downloads.len()!=2 || !win.downloads.values().all(|d|d["state"]=="Completed"){return Err("Downloads did not keep distinct completed IDs".into());}
                if !self.app.settings.downloads.default_folder.join("native-test (1).txt").is_file(){return Err("An existing download was overwritten".into());}
                self.query(id,true,"(()=>{document.getElementById('profileButton').click();const names=[...document.querySelectorAll('#profileList .profile-choice-name')].map(n=>n.textContent);const okay=names.includes('Kiểm thử');document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape'}));return okay})()");
            }
            13=>{
                let profile=self.app.windows.values().find(|w|!w.private&&w.profile_id!="Default").ok_or("New profile missing")?.id;
                self.app.handle_navigate(profile,&self.url);self.step+=1;
            }
            14=>{
                let profile=self.app.windows.values().find(|w|!w.private&&w.profile_id!="Default").unwrap().id;
                self.query(profile,false,"(()=>{if(document.title!=='Pip native test')return null;const s={title:document.title,url:location.href,storage:localStorage.getItem('pip-secret'),cookie:document.cookie};return s.title==='Pip native test'&&s.storage===null&&!s.cookie.includes('pip_secret')?true:JSON.stringify(s)})()");
            }
            15=>self.query(id,true,"(()=>{const button=document.getElementById('profileButton');button.click();const names=[...document.querySelectorAll('#profileList .profile-choice-name')].map(n=>n.textContent);return !document.getElementById('profilePopover').hidden&&names.includes('Cá nhân')&&names.includes('Kiểm thử')})()"),
            16=>{
                let win=&self.app.windows[&id];
                if !win.profile_menu_open {return Err("Profile popup did not expand the native toolbar WebView".into());}
                let chrome=win.chrome_webview.as_ref().ok_or("Toolbar WebView missing")?;
                if !webview_is_top(chrome) {return Err("Profile popup toolbar WebView is covered by page WebView".into());}
                self.query(id,true,"(()=>{document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape'}));return document.getElementById('profilePopover').hidden})()")
            }
            17=>{
                let win=&self.app.windows[&id];
                if win.profile_menu_open {return Err("Profile popup did not close".into());}
                let active=win.tab_manager.active_tab_id().and_then(|tab|win.tabs.get(&tab)).ok_or("Active page WebView missing")?;
                if !webview_is_top(active) {return Err("Active page WebView was not restored above the toolbar after popup close".into());}
                self.app.handle_create_tab(id,Some(self.url.clone()+"tabs"));
                self.draft_tab=self.app.windows[&id].tab_manager.active_tab_id();
                self.step+=1;
            }
            18=>self.query(id,false,"document.title==='Pip native test' ? (()=>{document.getElementById('draft').value='Nội dung chưa gửi';window.pipDraftKept=true;return true})() : null"),
            19=>{
                for index in 0..11 {
                    self.app.handle_create_tab(id,Some(format!("{}tabs?{index}",self.url)));
                }
                self.step+=1;
            }
            20=>{
                use webview2_com::Microsoft::Web::WebView2::Win32::*;
                use windows::core::Interface;
                use wry::WebViewExtWindows;
                let win=self.app.windows.get_mut(&id).unwrap();
                for (tab,view) in &win.tabs {
                    let active=Some(*tab)==win.tab_manager.active_tab_id();
                    let mut visible=windows::core::BOOL(0);
                    unsafe {view.controller().IsVisible(&mut visible)}.map_err(|e|e.to_string())?;
                    if visible.as_bool()!=active {return Err("Inactive tab visibility was not preserved".into());}
                    let core=unsafe {view.controller().CoreWebView2()}.map_err(|e|e.to_string())?;
                    if let Ok(core19)=core.cast::<ICoreWebView2_19>() {
                        let mut level=COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL;
                        unsafe {core19.MemoryUsageTargetLevel(&mut level)}.map_err(|e|e.to_string())?;
                        let expected=if active {COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL}else{COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW};
                        if level!=expected {return Err("Tab memory target did not match its visibility".into());}
                    }
                }
                win.handle_switch_tab(self.draft_tab.unwrap(),&self.app.cert_cache);
                self.step+=1;
            }
            21=>self.query(id,false,"window.pipDraftKept===true&&document.getElementById('draft').value==='Nội dung chưa gửi'"),
            22=>{
                self.app.save_session(id);
                let restored=self.app.create_profile_window(events,Some("evergreen://newtab".into()),"Default".into(),false).ok_or("Restore test window missing")?;
                self.restored_window=Some(restored);
                self.app.restore_session(restored);
                self.step+=1;
            }
            23=>{
                let restored=self.restored_window.unwrap();
                let win=&self.app.windows[&restored];
                if win.tab_manager.tabs().len()!=self.app.windows[&id].tab_manager.tabs().len()||win.tabs.len()!=1 {
                    return Err("Session restore eagerly loaded background tabs".into());
                }
                let last=win.tab_manager.tabs().last().unwrap().id;
                if !self.app.ensure_tab_view(restored,last){return Err("Deferred tab could not load".into());}
                self.app.windows.get_mut(&restored).unwrap().handle_switch_tab(last,&self.app.cert_cache);
                self.step+=1;
            }
            24=>{
                let restored=self.restored_window.unwrap();
                if self.app.windows[&restored].tabs.len()!=2{return Err("Selecting a deferred tab created unrelated views".into());}
                self.query(restored,false,"document.title==='Pip native test' ? document.getElementById('draft').value==='' : null");
            }
            25=>{
                self.app.windows.remove(&self.restored_window.unwrap());
                self.query(id,false,"window.pipDraftKept===true&&document.getElementById('draft').value==='Nội dung chưa gửi'");
            }
            26=>{
                self.app.handle_create_tab(id,Some("evergreen://newtab".into()));
                self.step+=1;
            }
            27=>self.query(id,false,"document.getElementById('homeSearch') ? (()=>{const hero=document.querySelector('.hero').getBoundingClientRect(),search=document.querySelector('.search').getBoundingClientRect();return search.height>=66&&search.width>=600&&Math.abs((search.left+search.width/2)-(hero.left+hero.width/2))<2&&getComputedStyle(document.querySelector('.hero')).backgroundImage.includes('data:image/webp')&&document.querySelector('.guide summary').textContent.includes('hướng dẫn')})() : null"),
            28=>{
                self.app.handle_open_settings(id);
                self.step+=1;
            }
            29=>self.query(id,false,"document.getElementById('clearCacheButton') ? (()=>{document.getElementById('clearCacheButton').click();return true})() : null"),
            30=>self.query(id,false,"document.getElementById('statusText').textContent.startsWith('Đã xóa bộ nhớ đệm') ? true : null"),
            31=>{
                self.app.handle_navigate(id,&self.url.clone());
                self.step+=1;
            }
            32=>self.query(id,false,"document.title==='Pip native test' ? (()=>{window.pipBeforeHardReload=true;return localStorage.getItem('pip-secret')==='normal'&&document.cookie.includes('pip_secret=normal')})() : null"),
            33=>{
                self.app.handle_navigate(id,&(self.url.clone()+"history-same-title"));
                self.step+=1;
            }
            34=>self.query(id,false,"location.pathname==='/history-same-title'&&document.readyState==='complete' ? (()=>{window.pipBeforeHardReload=true;return true})() : null"),
            35=>{
                let library=evergreen_core::library::Library::load(&self.app.library_path(id).unwrap()).map_err(|e|e.to_string())?;
                if !library.history.iter().any(|page|page.url.ends_with("/history-same-title")) {return Err("Navigation with an unchanged page title was missing from history".into());}
                let view=self.app.windows[&id].tab_manager.active_tab_id().and_then(|tab|self.app.windows[&id].tabs.get(&tab)).unwrap();
                accelerator::reload_without_cache(view);
                self.step+=1;
            }
            36=>self.query(id,false,"document.title==='Pip native test'&&typeof window.pipBeforeHardReload==='undefined' ? true : null"),
            37=>{
                self.app.user_event(events,BrowserEvent::Ipc(id,UiToHostMessage::OpenFindInPage));
                self.step+=1;
            }
            38=>self.query(id,true,"getComputedStyle(document.getElementById('findBar')).display==='flex'&&document.activeElement===document.getElementById('findInput')"),
            39=>{
                self.app.settings.features.enable_downloads_manager=false;
                self.app.windows.get_mut(&id).unwrap().close_sidebar();
                self.app.user_event(events,BrowserEvent::Shortcut(id,"Ctrl+J".into()));
                if self.app.windows[&id].is_sidebar_open { return Err("Disabled downloads manager still opened from shortcut".into()); }
                self.complete=true;events.exit();
            }
            _=>{self.complete=true;events.exit();}
        }
        Ok(())
    }
}
impl ApplicationHandler<BrowserEvent> for Check {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        self.app.resumed(events);
        self.main = self.app.windows.keys().next().copied();
        self.next = Instant::now() + Duration::from_secs(1);
    }
    fn window_event(&mut self, events: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.app.window_event(events, id, event);
    }
    fn user_event(&mut self, events: &ActiveEventLoop, event: BrowserEvent) {
        match event {
            BrowserEvent::CommandDone(name, _, reply) if name == "__headless_check" => {
                self.pending = false;
                if reply == "null" {
                    self.next = Instant::now() + Duration::from_millis(500);
                    return;
                }
                if reply != "true" {
                    self.error = Some(format!("UI step {} returned {reply}", self.step));
                    events.exit();
                    return;
                }
                self.step += 1;
                self.next = Instant::now() + Duration::from_millis(400);
            }
            event => self.app.user_event(events, event),
        }
    }
    fn about_to_wait(&mut self, events: &ActiveEventLoop) {
        if Instant::now() > self.deadline {
            self.error = Some(format!("Timeout at step {}", self.step));
            events.exit();
            return;
        }
        if !self.pending && Instant::now() >= self.next {
            if let Err(error) = self.tick(events) {
                self.error = Some(error);
                events.exit();
                return;
            }
            self.next = Instant::now() + Duration::from_millis(500);
        }
        events.set_control_flow(ControlFlow::WaitUntil(self.next.min(self.deadline)));
    }
}

pub fn run() {
    let stage = std::env::temp_dir().join(format!("Pip-headless-ui-{}", std::process::id()));
    let old_appdata = std::env::var_os("APPDATA");
    let old_webview = std::env::var_os("WEBVIEW2_USER_DATA_FOLDER");
    std::env::set_var("APPDATA", stage.join("roaming"));
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", stage.join("webview"));
    let mut settings = evergreen_core::settings::Settings::default();
    settings.downloads.default_folder = stage.join("downloads");
    settings.save_to_path(&get_settings_path()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://127.0.0.1:{}/",
        listener.local_addr().unwrap().port()
    );
    std::thread::spawn(move || {
        for mut socket in listener.incoming().flatten() {
            let _ = socket.set_read_timeout(Some(Duration::from_secs(2)));
            let mut bytes = [0; 8192];
            let Ok(n) = socket.read(&mut bytes) else {
                continue;
            };
            let request = String::from_utf8_lossy(&bytes[..n]);
            let path = request.split_whitespace().nth(1).unwrap_or("");
            let (extra, body) = if path == "/download" {
                (
                    "Content-Disposition: attachment; filename=\"native-test.txt\"\r\n",
                    "Pip download test\n",
                )
            } else {
                (
                    "Content-Type: text/html\r\n",
                    "<!doctype html><title>Pip native test</title><p>Normal page</p><input id='draft' aria-label='Draft'>",
                )
            };
            let _=socket.write_all(format!("HTTP/1.1 200 OK\r\n{extra}Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes());
        }
    });
    let events = EventLoop::<BrowserEvent>::with_user_event()
        .with_any_thread(false)
        .build()
        .unwrap();
    let mut app = BrowserApp::new(events.create_proxy(), "Native verification".into());
    app.background_check = true;
    let mut check = Check {
        app,
        main: None,
        step: 0,
        pending: false,
        next: Instant::now(),
        deadline: Instant::now() + Duration::from_secs(90),
        url,
        error: None,
        complete: false,
        draft_tab: None,
        restored_window: None,
    };
    events.run_app(&mut check).unwrap();
    check.app.windows.clear();
    assert_eq!(
        downloads::retained_downloads(),
        0,
        "Closed windows retained completed download handles"
    );
    if let Some(value) = old_appdata {
        std::env::set_var("APPDATA", value);
    } else {
        std::env::remove_var("APPDATA");
    }
    if let Some(value) = old_webview {
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", value);
    } else {
        std::env::remove_var("WEBVIEW2_USER_DATA_FOLDER");
    }
    assert!(check.complete, "{:?}", check.error);
    let artifact = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/native-ui-verification.json");
    std::fs::write(artifact,serde_json::to_vec_pretty(&serde_json::json!({"status":"passed","all_windows_hidden":true,"checks":["chrome dimensions","DNS toolbar/persistence","nested settings/IPC","bookmarks/history","InPrivate storage/history","distinct downloads/no overwrite","profile cookie/storage isolation","quick profile popup and create flow","native toolbar WebView stays above page while open and restores page order on close","12 local page tabs preserve unsent form and JS state while switching","exactly one tab view visible; hidden views request LOW memory and active NORMAL when supported","session restore creates one WebView regardless of background tab count; selecting deferred tab creates only that view","large centered search over mascot background and quick guide","native disk cache cleanup callback succeeds and preserves cookies/localStorage","completed download COM handles released on window close","history records navigation when page title is unchanged","native Page.reload ignores cache; document reload verified","Find menu opens and focuses search; disabled downloads manager stays closed"]})).unwrap()).unwrap();
    println!("PASS: hidden native app: profiles, DNS, settings, downloads/cleanup, 12-tab state preservation, lazy restore, centered mascot home, and cache cleanup without logout.");
}
