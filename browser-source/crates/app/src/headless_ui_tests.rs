//! Exercise the real app/IPC/WebView2 flow with every native window hidden.
use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
    extension_folder: Option<std::path::PathBuf>,
    extension_id: Option<String>,
    extension_install_done: bool,
    extension_removing: bool,
    extension_remove_done: bool,
    blocked_before_worker_test: u64,
    redirected_before_test: u64,
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
            0=>{
                let extra=self.app.create_profile_window(events,None,"Default".into(),true).ok_or("Could not create a temporary private window")?;
                self.app.user_event(events,BrowserEvent::Shortcut(extra,"Ctrl+Shift+W".into()));
                if self.app.windows.contains_key(&extra)||!self.app.windows.contains_key(&id){return Err("Ctrl+Shift+W did not close only the selected window".into());}
                self.app.user_event(events,BrowserEvent::Shortcut(id,"Ctrl+Shift+M".into()));
                self.query(id,true,"(()=>{const okay=document.querySelector('.tab-strip').offsetHeight===40&&document.querySelector('.nav-bar').offsetHeight===48&&document.querySelectorAll('.window-controls .window-control').length===3;const button=document.getElementById('profileButton'),menu=document.getElementById('profilePopover');const menuOkay=!menu.hidden&&button.getAttribute('aria-expanded')==='true'&&document.querySelectorAll('#profileList .profile-choice').length===1&&document.getElementById('profileCurrentName').textContent==='Cá nhân';showProfileCreate();document.getElementById('profileCreateName').value='Kiểm thử';document.getElementById('profileCreateForm').requestSubmit();document.getElementById('googleDnsButton').click();return okay&&menuOkay&&menu.hidden})()");
            }
            1=>{
                if !self.app.settings.google_dns{return Err("DNS toolbar did not save".into());}
                self.query(id,true,"(()=>{const button=document.getElementById('googleDnsButton');const saved=button.title.includes('áp dụng')&&button.querySelector('span').textContent==='Áp dụng';window.__renderGoogleDns(true,true,false,null);const checking=button.title.includes('đang kiểm tra')&&button.querySelector('span').textContent==='G DNS';window.__renderGoogleDns(true,true,false,true);const verified=button.title.includes('đã xác minh');window.__renderGoogleDns(true,true,false,false);const routeFailure=button.dataset.failed==='true'&&button.dataset.pending==='false'&&button.title.includes('chưa kết nối');window.__renderGoogleDns(true,false,true,null);const startupFailure=button.dataset.failed==='true'&&button.dataset.pending==='false'&&button.title.includes('không khởi động được')&&button.querySelector('span').textContent==='Cài đặt';window.__renderGoogleDns(true,false,false,null);postIpc('OpenSettings');return saved&&checking&&verified&&routeFailure&&startupFailure})()");
            }
            2=>self.query(id,false,"(()=>{const dnsRow=document.getElementById('googleDnsRestartRow'),hint=document.getElementById('googleDnsHint'),dns=document.getElementById('googleDnsToggle'),input=document.getElementById('askWhereToSaveToggle'),clear=document.getElementById('clearSiteDataButton'),clearTypes=['clearDataHistory','clearDataSite','clearDataCache','clearDataPasswords','clearDataAutofill','clearDataDownloads'];if(!dnsRow||dnsRow.hidden||getComputedStyle(dnsRow).display==='none'||!hint.textContent.includes('tuyến chưa đổi')||!dns||!input||!clear||!clear.textContent.includes('Xóa dữ liệu website')||!clear.getAttribute('onclick').includes('ClearSiteData')||clearTypes.some(id=>!document.getElementById(id))||typeof clearSelectedBrowsingData!=='function'||!document.getElementById('clearSelectedBrowsingDataButton').getAttribute('onclick').includes('clearSelectedBrowsingData'))return false;window.__renderGoogleDns(true,false,true,null);const failure=hint.textContent.includes('không khởi động được')&&!dnsRow.hidden;window.__renderGoogleDns(true,false,false,null);dns.checked=false;dns.onchange();const before=input.checked;input.click();for(const id of clearTypes)document.getElementById(id).checked=false;clearSelectedBrowsingData();const emptySelection=document.getElementById('statusText').textContent.includes('ít nhất một loại');window.confirm=()=>true;document.getElementById('clearDataCache').checked=true;clearSelectedBrowsingData();return failure&&before&&!input.checked&&emptySelection&&typeof window.__pipSend==='function'})()"),
            3=>{
                if self.app.settings.downloads.ask_where_to_save {return Err("Settings checkbox did not reach native storage".into());}
                if self.app.settings.google_dns {return Err("Google DNS setting did not return to its original state".into());}
                let disk=evergreen_core::settings::Settings::load_from_path(&get_settings_path()).map_err(|e|e.to_string())?;
                if disk.downloads.ask_where_to_save {return Err("Nested settings were not persisted".into());}
                if disk.google_dns {return Err("Google DNS setting was not persisted".into());}
                self.query(id,false,"document.getElementById('statusText')?.textContent.includes('Đã xóa dữ liệu đã chọn') ? true : null");
            }
            4=>self.query(id,false,"(()=>{if(document.title!=='Pip native test')return false;localStorage.setItem('pip-secret','normal');document.cookie='pip_secret=normal; SameSite=Lax';return typeof window.__pipSend==='undefined'})()"),
            5=>self.query(id,true,"(()=>{document.getElementById('bookmarkButton').click();postIpc('OpenLibrary',{view:'bookmarks'});return true})()"),
            6=>self.query(id,false,"(()=>{const portable=document.getElementById('importBookmarksButton')?.dataset.action==='ImportBookmarks'&&document.getElementById('exportBookmarksButton')?.dataset.action==='ExportBookmarks';const check=kind=>{const input=document.getElementById(kind+'Search'),list=document.getElementById(kind+'List'),empty=document.getElementById(kind+'NoResults');if(!input||!list.textContent.includes('Pip native test'))return false;input.value='pip native';input.dispatchEvent(new Event('input'));const matched=list.querySelectorAll('.library-row').length===1;input.value='not found';input.dispatchEvent(new Event('input'));const emptyState=empty.hidden===false&&list.querySelectorAll('.library-row').length===0;input.value='';input.dispatchEvent(new Event('input'));return matched&&emptyState&&list.querySelectorAll('.library-row').length===1};const bookmarks=check('bookmarks');document.querySelector('.rail-link[data-view=history]').click();const history=check('history');const row=[...document.querySelectorAll('#historyList .library-row')].find(item=>item.textContent.includes('Pip native test'));row?.querySelector('button')?.click();return portable&&bookmarks&&history&&!!row?true:JSON.stringify({portable,bookmarks,history,row:!!row,bookmarkText:document.querySelector('#bookmarksList')?.textContent,historyText:document.querySelector('#historyList')?.textContent})})()"),
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
                let restored=load_download_history("Default");
                if restored.len()!=2||!restored.values().all(|entry|entry["state"]=="Completed"&&entry["path"].as_str().is_some_and(|path|std::path::Path::new(path).is_file())){return Err("Completed downloads were not safely persisted for the profile".into());}
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
                let active_tab=win.tab_manager.active_tab_id().ok_or("No active tab for Keep Awake IPC test")?;
                self.app.user_event(events,BrowserEvent::Ipc(id,UiToHostMessage::ToggleKeepAwakeTab));
                if !self.app.windows[&id].tab_manager.get_tab_by_id(active_tab).is_some_and(|tab|tab.keep_awake){return Err("Keep Awake menu action did not protect the active tab".into());}
                self.app.user_event(events,BrowserEvent::Ipc(id,UiToHostMessage::ToggleKeepAwakeTab));
                if self.app.windows[&id].tab_manager.get_tab_by_id(active_tab).is_some_and(|tab|tab.keep_awake){return Err("Keep Awake menu action did not toggle off".into());}
                self.app.settings.performance.sleep_after_secs=1;
                let draft=self.draft_tab.unwrap();
                if self.app.windows.get_mut(&id).unwrap().tab_manager.toggle_keep_awake(draft)!=Some(true){return Err("Could not enable Keep Awake for the idle-tab test".into());}
                self.app.suspend_idle_tabs(BrowserApp::now_secs());
                if self.app.windows[&id].tab_manager.get_tab_by_id(draft).is_some_and(|tab|tab.status==evergreen_core::tabs::TabStatus::Suspended){return Err("Keep Awake tab was suspended".into());}
                if self.app.windows.get_mut(&id).unwrap().tab_manager.toggle_keep_awake(draft)!=Some(false){return Err("Could not disable Keep Awake for the idle-tab test".into());}
                self.app.suspend_idle_tabs(BrowserApp::now_secs());
                self.step=64;
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
                use webview2_com::Microsoft::Web::WebView2::Win32::{
                    ICoreWebView2_13, ICoreWebView2Profile4,
                    COREWEBVIEW2_PERMISSION_KIND_CAMERA, COREWEBVIEW2_PERMISSION_STATE_ALLOW,
                };
                use windows::core::{Interface,PCWSTR};
                use wry::WebViewExtWindows;
                let view=self.app.windows[&id].tab_manager.active_tab_id().and_then(|tab|self.app.windows[&id].tabs.get(&tab)).unwrap();
                let profile=unsafe{view.controller().CoreWebView2().and_then(|core|core.cast::<ICoreWebView2_13>()).and_then(|core|core.Profile()).and_then(|profile|profile.cast::<ICoreWebView2Profile4>())}.map_err(|error|error.to_string())?;
                let permission_origin=self.url.trim_end_matches('/');
                let origin_wide=permission_origin.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
                let proxy=self.app.proxy.clone();
                let callback=webview2_com::SetPermissionStateCompletedHandler::create(Box::new(move|result|{let _=proxy.send_event(BrowserEvent::CommandDone("__headless_permission_seed".into(),result.is_ok(),String::new()));Ok(())}));
                unsafe{profile.SetPermissionState(COREWEBVIEW2_PERMISSION_KIND_CAMERA,PCWSTR(origin_wide.as_ptr()),COREWEBVIEW2_PERMISSION_STATE_ALLOW,&callback)}.map_err(|error|error.to_string())?;
                self.app.handle_open_settings(id);
                self.step+=1;
            }
            29=>{
                let origin=serde_json::to_string(self.url.trim_end_matches('/')).unwrap();
                self.query(id,false,&format!("(()=>{{const list=document.getElementById('sitePermissionsList'),render=window.__renderSitePermissions;if(!list||!render)return null;const actual=[...list.querySelectorAll('.site-permission-item')].find(row=>row.textContent.includes({origin}));if(!actual)return null;if(!actual.textContent.includes('Camera')||!actual.textContent.includes('Được phép'))return false;render([{{origin:'<img src=x onerror=alert(1)>',kind_name:'Camera',state_name:'Được phép',kind_id:2}}]);const safe=!list.querySelector('img')&&list.textContent.includes('<img src=x onerror=alert(1)>');render([{{origin:{origin},kind_name:'Camera',state_name:'Được phép',kind_id:2}}]);const row=list.querySelector('.site-permission-item'),button=row?.querySelector('button');if(!safe||!button)return false;window.confirm=()=>true;button.click();return true}})()"));
            }
            30=>self.query(id,false,&format!("(()=>{{const status=document.getElementById('statusText'),list=document.getElementById('sitePermissionsList');if(!status||!status.textContent.startsWith('Đã đặt lại quyền website')||list?.textContent.includes('{}'))return null;document.getElementById('clearCacheButton').click();return true}})()",self.url.trim_end_matches('/'))),
            31=>self.query(id,false,"document.getElementById('statusText').textContent.startsWith('Đã xóa bộ nhớ đệm') ? true : null"),
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
                let tab_id=self.app.windows[&id].tab_manager.active_tab_id().unwrap();
                self.app.user_event(events,BrowserEvent::Ipc(id,UiToHostMessage::SetTabGroup{tab_id,name:"Research".into()}));
                self.query(id,true,"(()=>{const tab=document.getElementById('tab-'+activeTabId);return !!tab&&tab.classList.contains('grouped')&&tab.style.getPropertyValue('--tab-group-color')!==''&&tab.getAttribute('aria-label').includes('nhóm Research')&&typeof tab.oncontextmenu==='function'})()");
            }
            40=>{
                let tab_id=self.app.windows[&id].tab_manager.active_tab_id().unwrap();
                if self.app.windows[&id].tab_manager.get_tab_by_id(tab_id).and_then(|tab|tab.group.as_ref()).map(|group|group.name.as_str())!=Some("Research") {return Err("Tab group assignment did not reach native tab state".into());}
                self.app.user_event(events,BrowserEvent::Ipc(id,UiToHostMessage::TranslatePage));
                self.step+=1;
            }
            41=>self.query(id,false,"document.getElementById('translationTarget')?.textContent==='VI:Hello from Pip' ? true : null"),
            42=>{
                self.app.user_event(events,BrowserEvent::Ipc(id,UiToHostMessage::TranslatePage));
                self.query(id,false,"document.getElementById('translationTarget')?.textContent==='Hello from Pip' ? true : null");
            }
            43=>{
                self.app.settings.features.enable_downloads_manager=false;
                self.app.windows.get_mut(&id).unwrap().close_sidebar();
                self.app.user_event(events,BrowserEvent::Shortcut(id,"Ctrl+J".into()));
                if self.app.windows[&id].is_sidebar_open { return Err("Disabled downloads manager still opened from shortcut".into()); }
                self.app.user_event(events,BrowserEvent::Shortcut(id,"Ctrl+Shift+Delete".into()));
                self.query(id,false,"document.getElementById('clearDataHistory')&&document.activeElement===document.getElementById('clearDataHistory') ? true : null");
            }
            44=>{
                let folder=std::env::temp_dir().join(format!("pip-native-extension-{}",std::process::id()));
                std::fs::create_dir_all(&folder).map_err(|error|error.to_string())?;
                std::fs::write(folder.join("manifest.json"),r#"{"manifest_version":3,"name":"Pip native extension test","version":"1.0","content_scripts":[{"matches":["http://127.0.0.1/*"],"js":["content.js"],"run_at":"document_idle"}]}"#).map_err(|error|error.to_string())?;
                std::fs::write(folder.join("content.js"),"document.documentElement.setAttribute('data-pip-extension-probe','loaded');").map_err(|error|error.to_string())?;
                self.extension_folder=Some(folder.clone());
                self.app.install_browser_extension(id,folder);
                self.step=45;
            }
            45=>{
                if !self.extension_install_done{return Ok(())}
                self.app.handle_navigate(id,&(self.url.clone()+"extension-check"));
                self.step=46;
            }
            46=>self.query(id,false,"document.documentElement.getAttribute('data-pip-extension-probe')==='loaded' ? true : null"),
            47=>{
                let Some(extension_id)=self.extension_id.clone() else {self.app.request_browser_extensions(id);return Ok(())};
                self.extension_removing=true;
                self.app.change_browser_extension(id,extension_id,None);
                self.step=48;
            }
            48=>{
                if !self.extension_remove_done{return Ok(())}
                self.app.handle_navigate(id,&(self.url.clone()+"extension-removed"));
                self.step=49;
            }
            49=>self.query(id,false,"document.documentElement.getAttribute('data-pip-extension-probe')===null ? true : null"),
            50=>{
                let page_url=format!("{}extension-removed",self.url);
                let script=adblock::procedural_test_script(&page_url);
                let query=format!("(()=>{{document.body.innerHTML='<div class=\"ad\">Casino bonus</div><div class=\"content\">Article</div>';const count=eval({});const ad=!!document.querySelector('.ad'),content=!!document.querySelector('.content');return count===1&&!ad&&content?true:JSON.stringify({{count,href:location.href,ad,content}})}})()",serde_json::to_string(&script).unwrap());
                self.query(id,false,&query);
            }
            51=>{
                self.blocked_before_worker_test=self.app.adblock.borrow().blocked;
                self.app.handle_navigate(id,&(self.url.clone()+"worker-test"));
                self.step=52;
            }
            52=>self.query(id,false,"document.body.dataset.workerTest==='done'&&document.body.dataset.sharedWorkerTest==='done' ? true : null"),
            53=>{
                if self.app.adblock.borrow().blocked<self.blocked_before_worker_test+2{return Err("Service/shared-worker request bypassed WebView2 ad filtering".into())}
                self.redirected_before_test=self.app.adblock.borrow().redirected;
                self.app.handle_navigate(id,&(self.url.clone()+"redirect-test"));
                self.step=54;
            }
            54=>self.query(id,false,"document.body.dataset.redirectTest==='failed' ? false : document.body.dataset.redirectTest==='loaded' ? true : null"),
            55=>{
                if self.app.adblock.borrow().redirected<=self.redirected_before_test{return Err("uBlock redirect resource did not reach the WebView2 response".into())}
                self.step+=1;
            }
            56=>{
                let id=self.main.unwrap();
                let path=self.app.passwords_path(id).ok_or("Password vault path missing")?;
                let mut entries=Vec::new();
                passwords::upsert(&mut entries,&self.url,"native-one","test-secret-one").map_err(|e|e.to_string())?;
                passwords::upsert(&mut entries,&self.url,"native-two","test-secret-two").map_err(|e|e.to_string())?;
                passwords::save(&path,&entries)?;
                self.app.handle_navigate(id,&(self.url.clone()+"password-test"));
                self.step=57;
            }
            57=>self.query(id,false,"document.querySelector('#username')&&document.querySelector('#password')&&document.querySelector('#username').value===''&&document.querySelector('#password').value==='' ? true : null"),
            58=>{
                self.app.user_event(events,BrowserEvent::Shortcut(id,"Alt+Shift+P".into()));
                self.step=59;
            }
            59=>self.query(id,false,"document.querySelector('#username').value===''&&document.querySelector('#password').value===''"),
            60=>{
                let path=self.app.passwords_path(id).ok_or("Password vault path missing")?;
                let mut entries=passwords::load(&path)?;
                entries.retain(|entry|entry.username!="native-two");
                passwords::save(&path,&entries)?;
                self.app.user_event(events,BrowserEvent::Shortcut(id,"Alt+Shift+P".into()));
                self.step=61;
            }
            61=>self.query(id,false,"document.querySelector('#username').value==='native-one'&&document.querySelector('#password').value==='test-secret-one'"),
            62=>{
                use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2_16,ICoreWebView2_25};
                use windows::core::Interface;
                use wry::WebViewExtWindows;
                let view=self.app.windows[&id].tab_manager.active_tab_id().and_then(|tab|self.app.windows[&id].tabs.get(&tab)).ok_or("Active WebView missing for print/save capability check")?;
                let core=unsafe{view.controller().CoreWebView2()}.map_err(|error|error.to_string())?;
                core.cast::<ICoreWebView2_16>().map_err(|error|format!("WebView2 print API unavailable: {error}"))?;
                core.cast::<ICoreWebView2_25>().map_err(|error|format!("WebView2 Save As API unavailable: {error}"))?;
                self.app.handle_create_tab(id,Some("evergreen://settings".into()));
                self.step=63;
            }
            63=>self.query(id,false,"(()=>{const button=document.getElementById('checkAppUpdatesButton'),status=document.getElementById('appUpdateStatus'),link=document.getElementById('openAppReleaseButton'),filters=document.getElementById('updateAdblockFiltersButton'),sleep=document.getElementById('backgroundThrottlingToggle'),delay=document.getElementById('sleepAfterSelect');if(!button||!status||!link||!filters||!sleep||!delay||!sleep.checked||delay.value!=='300'||typeof window.__renderAppUpdate!=='function'||typeof window.__renderAdblockFilterUpdate!=='function')return false;window.__renderAppUpdate({currentVersion:'0.1.9',latestVersion:'0.1.10',updateAvailable:true,releaseUrl:'https://github.com/giangocquoc-art/pip-browser/releases/tag/v0.1.10'});const offered=!link.hidden&&link.href==='https://github.com/giangocquoc-art/pip-browser/releases/tag/v0.1.10'&&status.textContent.includes('v0.1.10');window.__renderAppUpdate({currentVersion:'0.1.12',latestVersion:'0.1.9',updateAvailable:false,releaseUrl:'https://github.com/giangocquoc-art/pip-browser/releases/tag/v0.1.9'});window.__renderAdblockFilterUpdate({listCount:4,ruleCount:1200});const filterDone=document.getElementById('adblockUpdateStatus').textContent.includes('1.200');return offered&&link.hidden&&status.textContent.includes('mới hơn')&&filterDone})()"),
            64=>{
                let tab_id=self.draft_tab.unwrap();
                if self.app.windows[&id].tab_manager.get_tab_by_id(tab_id).map(|tab|tab.status)!=Some(evergreen_core::tabs::TabStatus::Suspended){return Ok(())}
                self.query(id,true,&format!("(()=>{{const tab=document.getElementById('tab-{}');return tab?.getAttribute('aria-label')?.includes('tab đang ngủ; chọn để tiếp tục')&&tab.title.includes('Tab đang ngủ')&&tab.classList.contains('sleeping')?true:null}})()",tab_id.0));
            }
            65=>{
                let tab_id=self.draft_tab.unwrap();
                self.app.settings.performance.sleep_after_secs=300;
                self.app.windows.get_mut(&id).unwrap().handle_switch_tab(tab_id,&self.app.cert_cache);
                self.step=21;
            }
            66=>{
                let chrome=self.app.windows[&id].chrome_webview.as_ref().ok_or("Chrome WebView missing for omnibox suggestion test")?;
                chrome.evaluate_script("const input=document.getElementById('omnibox');input.value='Pip native';input.focus()").map_err(|error|error.to_string())?;
                self.app.user_event(events,BrowserEvent::Ipc(id,UiToHostMessage::SuggestOmnibox{query:"Pip native".into()}));
                self.step=68;
            }
            68=>self.query(id,true,"(()=>{const input=document.getElementById('omnibox'),list=document.getElementById('omniboxSuggestions'),home=document.getElementById('btnHome');if(!input||!list||!home||home.getAttribute('aria-label')!=='Trang chủ'||!home.title.includes('Alt+Home')||typeof window.__renderOmniboxSuggestions!=='function')return false;const count=list.querySelectorAll('.omnibox-suggestion').length;const actual=count===8&&list.textContent.includes('Pip native test')&&input.getAttribute('aria-expanded')==='true';const before={count,text:list.textContent,expanded:input.getAttribute('aria-expanded'),focused:document.activeElement===input};window.__renderOmniboxSuggestions('Pip native',[{url:'https://safe.example/',title:'<img src=x onerror=alert(1)>',bookmarked:true}]);const safe=list.querySelectorAll('img').length===0&&list.textContent.includes('<img src=x onerror=alert(1)>');input.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true,cancelable:true}));const selected=input.getAttribute('aria-activedescendant')==='omnibox-suggestion-0'&&list.querySelector('[aria-selected=true]')!==null;input.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));const closed=list.hidden&&input.getAttribute('aria-expanded')==='false';if(!actual||!safe||!selected||!closed)return JSON.stringify({actual,before,safe,selected,closed});home.click();return true})()"),
            69=>{
                if self.app.windows[&id].tab_manager.active_tab().map(|tab|tab.url.as_str())!=Some("evergreen://newtab"){return Ok(())}
                self.query(id,false,"(()=>{const list=document.getElementById('homeShortcuts'),add=list?.querySelector('.shortcut-add');if(!list||!add||list.querySelectorAll('.shortcut-item').length!==4)return null;const entries=['https://custom.example/','Lối tắt thử'];window.prompt=()=>entries.shift();add.click();return true})()");
            }
            70=>{
                let library=evergreen_core::library::Library::load(&self.app.library_path(id).unwrap()).map_err(|error|error.to_string())?;
                if !library.home_shortcuts().iter().any(|page|page.url=="https://custom.example/"){return Err("New-tab shortcut was not saved to the profile library".into());}
                self.query(id,false,"(()=>{const list=document.getElementById('homeShortcuts'),remove=[...list.querySelectorAll('button')].find(button=>button.getAttribute('aria-label')==='Xóa lối tắt Lối tắt thử');if(!remove)return false;window.confirm=()=>true;remove.click();return true})()");
            }
            71=>{
                let library=evergreen_core::library::Library::load(&self.app.library_path(id).unwrap()).map_err(|error|error.to_string())?;
                if library.home_shortcuts().len()!=4||library.home_shortcuts().iter().any(|page|page.url=="https://custom.example/"){return Err("Removing a new-tab shortcut did not persist cleanly".into());}
                if self.app.windows[&id].tab_manager.active_tab().map(|tab|tab.url.as_str())!=Some("evergreen://newtab"){return Ok(())}
                self.app.user_event(events,BrowserEvent::Shortcut(id,"Alt+Home".into()));
                self.step=72;
            }
            72=>{
                if self.app.windows[&id].tab_manager.active_tab().map(|tab|tab.url.as_str())!=Some("evergreen://newtab"){return Ok(())}
                self.query(id,false,"document.getElementById('homeView')&&document.getElementById('homeSearch') ? true : null");
            }
            73=>{
                self.app.handle_navigate(id, &self.url);
                self.step=74;
            }
            74=>{
                if self.app.windows[&id].tab_manager.active_tab().map(|tab|tab.url.as_str())!=Some(self.url.as_str()){return Ok(())}
                if !sidebar_ui::SIDEBAR_HTML.contains("sendAction('RepairPlayback')")||!sidebar_ui::SIDEBAR_HTML.contains("Sửa lỗi phát video") {return Err("The Pip menu does not expose the confirmed video repair action".into())}
                self.app.user_event(events,BrowserEvent::Ipc(id,UiToHostMessage::RepairPlayback));
                if self.app.adblock.borrow().enabled(&self.url)!=Some(false){return Err("Video repair did not persist a site-only ad-blocking exception".into())}
                self.step=75;
            }
            75=>{
                if let Some(folder)=self.extension_folder.take(){let _=std::fs::remove_dir_all(folder);}
                self.complete=true;
                events.exit();
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
                if self.step == 31 {
                    let id = self.main.unwrap();
                    self.app.handle_navigate(id, &self.url.clone());
                }
                if self.step == 3 {
                    let id = self.main.unwrap();
                    self.app.handle_navigate(id, &self.url.clone());
                }
                if self.step == 43 {
                    self.step = 44;
                    self.next = Instant::now() + Duration::from_millis(400);
                    return;
                }
                if self.step == 63 {
                    self.step = 66;
                    self.next = Instant::now() + Duration::from_millis(400);
                    return;
                }
                if self.step == 68 {
                    self.step = 69;
                    self.next = Instant::now() + Duration::from_millis(400);
                    return;
                }
                self.step += 1;
                self.next = Instant::now() + Duration::from_millis(400);
            }
            event => {
                if let BrowserEvent::BrowserExtensionsLoaded(_, Ok(extensions)) = &event {
                    self.extension_id = extensions
                        .iter()
                        .find(|extension| extension.name == "Pip native extension test")
                        .map(|extension| extension.id.clone());
                }
                if let BrowserEvent::BrowserExtensionOperationDone(_, result) = &event {
                    if result.is_ok() {
                        if self.extension_removing {
                            self.extension_remove_done = true;
                        } else {
                            self.extension_install_done = true;
                        }
                    }
                }
                self.app.user_event(events, event)
            }
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
    let run_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stage =
        std::env::temp_dir().join(format!("Pip-headless-ui-{}-{run_id}", std::process::id()));
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
            } else if path == "/worker-test" {
                (
                    "Content-Type: text/html\r\n",
                    "<!doctype html><title>Pip native test</title><body><script>navigator.serviceWorker.addEventListener('message',event=>{document.body.dataset.workerTest=event.data;if(event.data==='done'){const shared=new SharedWorker('/shared-worker.js');shared.port.onmessage=message=>document.body.dataset.sharedWorkerTest=message.data;shared.port.start();shared.port.postMessage('probe')}});navigator.serviceWorker.register('/sw.js').then(async()=>{await navigator.serviceWorker.ready;const send=()=>navigator.serviceWorker.controller&&navigator.serviceWorker.controller.postMessage('probe');if(navigator.serviceWorker.controller)send();else navigator.serviceWorker.addEventListener('controllerchange',send,{once:true})}).catch(()=>document.body.dataset.workerTest='failed')</script></body>",
                )
            } else if path == "/sw.js" {
                (
                    "Content-Type: application/javascript\r\nService-Worker-Allowed: /\r\n",
                    "self.addEventListener('activate',event=>event.waitUntil(self.clients.claim()));self.addEventListener('message',event=>{if(event.data==='probe')event.waitUntil(fetch('https://ad.doubleclick.net/pip-worker-probe.js').catch(()=>null).then(()=>event.source.postMessage('done')))})",
                )
            } else if path == "/shared-worker.js" {
                (
                    "Content-Type: application/javascript\r\n",
                    "onconnect=event=>{const port=event.ports[0];port.onmessage=message=>{if(message.data==='probe')fetch('https://ad.doubleclick.net/pip-shared-worker-probe.js').catch(()=>null).then(()=>port.postMessage('done'))};port.start()}",
                )
            } else if path == "/redirect-test" {
                (
                    "Content-Type: text/html\r\n",
                    "<!doctype html><title>Pip native test</title><body><script>const probe=document.createElement('script');probe.src='https://go.oclasrv.com/apu.php';probe.onload=()=>document.body.dataset.redirectTest='loaded';probe.onerror=()=>document.body.dataset.redirectTest='failed';document.body.append(probe)</script></body>",
                )
            } else if path == "/password-test" {
                (
                    "Content-Type: text/html\r\n",
                    "<!doctype html><title>Pip native password test</title><form><input id='username' name='username' autocomplete='username'><input id='password' name='password' type='password' autocomplete='current-password'></form>",
                )
            } else {
                (
                    "Content-Type: text/html\r\n",
                    "<!doctype html><title>Pip native test</title><p id='translationTarget'>Hello from Pip</p><p>Normal page</p><input id='draft' aria-label='Draft'><script>window.LanguageDetector={create:async()=>({detect:async()=>[{detectedLanguage:'en'}],destroy(){}})};window.Translator={availability:async()=> 'available',create:async()=>({translate:async text=>'VI:'+text,destroy(){}})};</script>",
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
        extension_folder: None,
        extension_id: None,
        extension_install_done: false,
        extension_removing: false,
        extension_remove_done: false,
        blocked_before_worker_test: 0,
        redirected_before_test: 0,
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
    std::fs::write(&artifact,serde_json::to_vec_pretty(&serde_json::json!({"status":"passed","all_windows_hidden":true,"checks":["chrome dimensions","DNS toolbar/persistence","nested settings/IPC","bookmarks/history","native local bookmark/history omnibox suggestions through IPC, keyboard navigation, safe text rendering, and Alt+Home","profile-local quick-link add/remove through the native IPC with persistence","InPrivate storage/history","distinct downloads/no overwrite","profile cookie/storage isolation","profile-scoped persistent download history","quick profile popup and create flow","native toolbar WebView stays above page while open and restores page order on close","12 local page tabs preserve unsent form and JS state while switching","exactly one tab view visible; hidden views request LOW memory and active NORMAL when supported","idle inactive WebView2 tab suspends after configured delay, exposes an accessible sleeping state, and preserves unsent form after waking","session restore creates one WebView regardless of background tab count; selecting deferred tab creates only that view","large centered search over mascot background and quick guide","native disk cache cleanup callback succeeds and preserves cookies/localStorage","completed download COM handles released on window close","history records navigation when page title is unchanged","native Page.reload ignores cache; document reload verified","Find menu opens and focuses search","Tab group action syncs to native state and draws a visible group indicator","Page translation invokes local Translator API and restores original text","disabled downloads manager stays closed","unpacked WebView2 extension installs, content script runs, and removal stops it","procedural cosmetic ad filter removes matching ad nodes in real WebView2","service-worker and shared-worker requests pass through Brave network filtering","uBlock noop redirect resource reaches a native WebView2 response","Alt+Shift+P leaves multiple saved logins untouched and fills the only exact-origin match","toolbar Home button and Alt+Home navigate the current tab to Pip's start page","version checker offers only a newer Pip release on the official GitHub repository"]})).unwrap()).unwrap();
    let mut report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&artifact).unwrap()).unwrap();
    report["checks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(
            "Keep Awake menu action toggles and protects a background tab from WebView2 suspension"
        ));
    std::fs::write(&artifact, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(stage);
    println!("PASS: hidden native app: profiles, DNS, password picker, release checker, keep-awake tabs, tab groups, translation, extensions, ad filtering, settings, downloads, session restore, and cache cleanup.");
}

struct GoogleDnsWebViewCheck {
    app: BrowserApp,
    main: Option<WindowId>,
    proxy_count_before_navigation: Option<u64>,
    pending: bool,
    next: Instant,
    deadline: Instant,
    error: Option<String>,
    complete: bool,
    phase: &'static str,
    last_reply: Option<String>,
    last_navigation: Option<String>,
}

impl ApplicationHandler<BrowserEvent> for GoogleDnsWebViewCheck {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        self.app.resumed(events);
        self.main = self.app.windows.keys().next().copied();
        self.phase = "resumed";
        if let Some(view) = self.main.and_then(|id| {
            self.app.windows.get(&id).and_then(|window| {
                window
                    .tab_manager
                    .active_tab_id()
                    .and_then(|tab| window.tabs.get(&tab))
            })
        }) {
            use wry::WebViewExtWindows;
            if let Ok(core) = unsafe { view.controller().CoreWebView2() } {
                let proxy = self.app.proxy.clone();
                let handler = webview2_com::NavigationCompletedEventHandler::create(Box::new(
                    move |_, args| {
                        if let Some(args) = args {
                            let mut success = windows::core::BOOL(0);
                            let mut status = webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_WEB_ERROR_STATUS::default();
                            unsafe {
                                let _ = args.IsSuccess(&mut success);
                                let _ = args.WebErrorStatus(&mut status);
                            }
                            let report =
                                format!("success={}, web_error={}", success.as_bool(), status.0);
                            let _ = proxy.send_event(BrowserEvent::CommandDone(
                                "__google_dns_navigation_completed".into(),
                                true,
                                report,
                            ));
                        }
                        Ok(())
                    },
                ));
                let mut token = 0;
                let _ = unsafe { core.add_NavigationCompleted(&handler, &mut token) };
            }
        }
        self.next = Instant::now() + Duration::from_millis(250);
    }

    fn window_event(&mut self, events: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.app.window_event(events, id, event);
    }

    fn user_event(&mut self, events: &ActiveEventLoop, event: BrowserEvent) {
        match event {
            BrowserEvent::CommandDone(name, _, report)
                if name == "__google_dns_navigation_completed" =>
            {
                self.last_navigation = Some(report);
            }
            BrowserEvent::CommandDone(name, _, reply) if name == "__google_dns_webview_check" => {
                self.pending = false;
                self.last_reply = Some(reply.clone());
                if reply == "null" {
                    self.next = Instant::now() + Duration::from_millis(500);
                    return;
                }
                if reply != "true" {
                    self.next = Instant::now() + Duration::from_millis(500);
                    return;
                }
                let current = self
                    .app
                    .doh_proxy
                    .as_ref()
                    .map(doh_proxy::DohProxy::accepted_connections)
                    .unwrap_or(0);
                let before = self.proxy_count_before_navigation.unwrap_or(current);
                if current <= before {
                    self.error = Some(
                        "WebView2 loaded Google without connecting to Pip's local DoH proxy".into(),
                    );
                    events.exit();
                    return;
                }
                let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../target/google-dns-webview-verification.json");
                let report = serde_json::json!({
                    "status": "passed",
                    "destination": "https://www.google.com/",
                    "proxy_connections": current - before,
                    "windows_hidden": true
                });
                if let Err(error) =
                    std::fs::write(evidence, serde_json::to_vec_pretty(&report).unwrap())
                {
                    self.error = Some(format!(
                        "Could not write DNS verification evidence: {error}"
                    ));
                    events.exit();
                    return;
                }
                self.complete = true;
                events.exit();
            }
            event => self.app.user_event(events, event),
        }
    }

    fn about_to_wait(&mut self, events: &ActiveEventLoop) {
        if Instant::now() > self.deadline {
            let active_url = self
                .main
                .and_then(|id| self.app.windows.get(&id))
                .and_then(|window| window.tab_manager.active_tab())
                .map(|tab| tab.url.clone());
            let proxy_count = self
                .app
                .doh_proxy
                .as_ref()
                .map(doh_proxy::DohProxy::accepted_connections);
            self.error = Some(format!(
                "Timed out waiting for the Google DNS WebView route (phase={}, url={active_url:?}, proxy={proxy_count:?}, baseline={:?}, last_reply={:?}, navigation={:?}, pending={})",
                self.phase, self.proxy_count_before_navigation, self.last_reply, self.last_navigation, self.pending
            ));
            events.exit();
            return;
        }
        if self.pending || Instant::now() < self.next {
            events.set_control_flow(ControlFlow::WaitUntil(self.next.min(self.deadline)));
            return;
        }
        let Some(window) = self.main else {
            self.next = Instant::now() + Duration::from_millis(100);
            events.set_control_flow(ControlFlow::WaitUntil(self.next));
            return;
        };
        if self.proxy_count_before_navigation.is_none() {
            self.proxy_count_before_navigation = self
                .app
                .doh_proxy
                .as_ref()
                .map(doh_proxy::DohProxy::accepted_connections);
            self.app.handle_navigate(window, "https://www.google.com/");
            self.phase = "navigated to Google";
            self.next = Instant::now() + Duration::from_millis(250);
            events.set_control_flow(ControlFlow::WaitUntil(self.next.min(self.deadline)));
            return;
        }
        if self
            .app
            .windows
            .values()
            .any(|window| window.window.is_visible() == Some(true))
        {
            self.error = Some("Google DNS verification displayed a window".into());
            events.exit();
            return;
        }
        let Some(view) = self.app.windows[&window]
            .tab_manager
            .active_tab_id()
            .and_then(|id| self.app.windows[&window].tabs.get(&id))
        else {
            self.next = Instant::now() + Duration::from_millis(100);
            events.set_control_flow(ControlFlow::WaitUntil(self.next));
            return;
        };
        let proxy = self.app.proxy.clone();
        self.pending = true;
        if let Err(error) = view.evaluate_script_with_callback(
            "location.hostname.endsWith('google.com')&&document.readyState==='complete'&&!!document.body&&document.body.innerText.length>0",
            move |reply| {
                let _ = proxy.send_event(BrowserEvent::CommandDone(
                    "__google_dns_webview_check".into(),
                    true,
                    reply,
                ));
            },
        ) {
            self.pending = false;
            self.error = Some(format!("Could not inspect the Google DNS WebView: {error}"));
            events.exit();
            return;
        }
        self.next = Instant::now() + Duration::from_millis(500);
        events.set_control_flow(ControlFlow::WaitUntil(self.next.min(self.deadline)));
    }
}

pub fn run_google_dns_webview() {
    let run_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stage = std::env::temp_dir().join(format!(
        "Pip-google-dns-check-{}-{run_id}",
        std::process::id()
    ));
    let old_appdata = std::env::var_os("APPDATA");
    let old_webview = std::env::var_os("WEBVIEW2_USER_DATA_FOLDER");
    std::env::set_var("APPDATA", stage.join("roaming"));
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", stage.join("webview"));
    let settings = evergreen_core::settings::Settings {
        google_dns: true,
        ..Default::default()
    };
    settings.save_to_path(&get_settings_path()).unwrap();

    let events = EventLoop::<BrowserEvent>::with_user_event()
        .with_any_thread(false)
        .build()
        .unwrap();
    let mut app = BrowserApp::new(events.create_proxy(), "Google DNS verification".into());
    app.background_check = true;
    app.google_dns_health_check_started = true;
    let mut check = GoogleDnsWebViewCheck {
        app,
        main: None,
        proxy_count_before_navigation: None,
        pending: false,
        next: Instant::now() + Duration::from_secs(1),
        deadline: Instant::now() + Duration::from_secs(30),
        error: None,
        complete: false,
        phase: "created",
        last_reply: None,
        last_navigation: None,
    };
    events.run_app(&mut check).unwrap();
    check.app.windows.clear();
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
    let _ = std::fs::remove_dir_all(stage);
    assert!(check.complete, "{:?}", check.error);
    println!("PASS: WebView2 loaded Google's test response and opened a connection to Pip's local Google DNS proxy.");
}
