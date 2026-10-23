//! Exercise the real app/IPC/WebView2 flow with every native window hidden.
use super::*;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use winit::{event_loop::ControlFlow, platform::windows::EventLoopBuilderExtWindows};

fn webview_native_state(
    view: &WebView,
) -> Result<(windows::Win32::Foundation::RECT, bool), String> {
    use windows::core::BOOL;
    use wry::WebViewExtWindows;

    let controller = view.controller();
    let mut bounds = windows::Win32::Foundation::RECT::default();
    let mut visible = BOOL(0);
    unsafe {
        controller
            .Bounds(&mut bounds)
            .map_err(|error| error.to_string())?;
        controller
            .IsVisible(&mut visible)
            .map_err(|error| error.to_string())?;
    }
    Ok((bounds, visible.as_bool()))
}

fn same_rect(
    left: windows::Win32::Foundation::RECT,
    right: windows::Win32::Foundation::RECT,
) -> bool {
    left.left == right.left
        && left.top == right.top
        && left.right == right.right
        && left.bottom == right.bottom
}

fn password_autofill_flags(view: &WebView) -> Result<(bool, bool), String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2Profile6, ICoreWebView2_13};
    use windows::core::{Interface, BOOL};
    use wry::WebViewExtWindows;

    let profile = unsafe { view.controller().CoreWebView2() }
        .map_err(|error| error.to_string())?
        .cast::<ICoreWebView2_13>()
        .map_err(|error| error.to_string())?;
    let profile = unsafe { profile.Profile() }
        .map_err(|error| error.to_string())?
        .cast::<ICoreWebView2Profile6>()
        .map_err(|error| error.to_string())?;
    let mut password_autosave = BOOL(0);
    let mut general_autofill = BOOL(0);
    unsafe {
        profile
            .IsPasswordAutosaveEnabled(&mut password_autosave)
            .map_err(|error| error.to_string())?;
        profile
            .IsGeneralAutofillEnabled(&mut general_autofill)
            .map_err(|error| error.to_string())?;
    }
    Ok((password_autosave.as_bool(), general_autofill.as_bool()))
}

fn profile_private_mode(view: &WebView) -> Result<bool, String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_13;
    use windows::core::{Interface, BOOL};
    use wry::WebViewExtWindows;

    let profile = unsafe { view.controller().CoreWebView2() }
        .map_err(|error| error.to_string())?
        .cast::<ICoreWebView2_13>()
        .map_err(|error| error.to_string())?;
    let profile = unsafe { profile.Profile() }.map_err(|error| error.to_string())?;
    let mut is_private = BOOL(0);
    unsafe {
        profile
            .IsInPrivateModeEnabled(&mut is_private)
            .map_err(|error| error.to_string())?;
    }
    Ok(is_private.as_bool())
}

struct Check {
    app: BrowserApp,
    main: Option<WindowId>,
    step: u8,
    pending: bool,
    next: Instant,
    deadline: Instant,
    url: String,
    worker_url: String,
    error: Option<String>,
    complete: bool,
    observed_step: Option<u8>,
    step_started: Instant,
    pending_started: Option<Instant>,
    draft_tab: Option<TabId>,
    discard_safe_tab: Option<TabId>,
    discard_dirty_tab: Option<TabId>,
    restored_window: Option<WindowId>,
    session_test_window: Option<WindowId>,
    session_restore_first: Option<WindowId>,
    session_restore_second: Option<WindowId>,
    fullscreen_normal_bounds: Option<(
        windows::Win32::Foundation::RECT,
        windows::Win32::Foundation::RECT,
        windows::Win32::Foundation::RECT,
    )>,
    extension_folder: Option<std::path::PathBuf>,
    extension_id: Option<String>,
    extension_install_done: bool,
    extension_removing: bool,
    extension_remove_done: bool,
    blocked_before_worker_test: u64,
    blocked_before_popup_test: u64,
    popup_tab_count_before: usize,
    popup_window_count_before: usize,
    redirected_before_test: u64,
    private_permission_check_phase: u8,
    privacy_test_tab: Option<TabId>,
    removed_history_visit: Option<(String, i64)>,
}
impl Check {
    fn send_escape(&mut self, window: WindowId) -> Result<(), String> {
        self.app
            .windows
            .get_mut(&window)
            .ok_or("Escape test window missing")?
            .handle_escape();
        Ok(())
    }

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
        self.pending_started = Some(Instant::now());
        view.evaluate_script_with_callback(script, move |value| {
            let _ = proxy.send_event(BrowserEvent::CommandDone(
                "__headless_check".into(),
                true,
                value,
            ));
        })
        .unwrap();
    }
    fn query_tab(&mut self, window: WindowId, tab_id: TabId, script: &str) {
        let view = self.app.windows[&window].tabs.get(&tab_id).unwrap();
        let proxy = self.app.proxy.clone();
        self.pending = true;
        self.pending_started = Some(Instant::now());
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
        let visible_windows = self
            .app
            .windows
            .iter()
            .filter(|(id, window)| {
                let fullscreen_fixture = (118..=131).contains(&self.step)
                    && self.session_restore_first == Some(**id);
                window.window.is_visible() == Some(true) && !fullscreen_fixture
            })
            .map(|(id, window)| {
                format!(
                    "{id:?} (profile={}, slot={}, fullscreen={}, restored_first={}, restored_second={})",
                    window.profile_id,
                    window.session_slot,
                    window.is_fullscreen,
                    Some(*id) == self.session_restore_first,
                    Some(*id) == self.session_restore_second
                )
            })
            .collect::<Vec<_>>();
        if !visible_windows.is_empty() {
            return Err(format!(
                "A test window became visible at verifier step {}: {}",
                self.step,
                visible_windows.join(", ")
            ));
        }
        if self.app.windows.values().any(|w| w.window.is_decorated()) {
            return Err("The native title bar is still separate from the Pip tab strip".into());
        }
        match self.step {
            0=>{
                let extra=self.app.create_profile_window(events,None,"Default".into(),true,None).ok_or("Could not create a temporary private window")?;
                self.app.user_event(events,BrowserEvent::Shortcut(extra,"Ctrl+Shift+W".into()));
                if self.app.windows.contains_key(&extra)||!self.app.windows.contains_key(&id){return Err("Ctrl+Shift+W did not close only the selected window".into());}
                self.app.user_event(events,BrowserEvent::Shortcut(id,"Ctrl+Shift+M".into()));
                self.query(id,true,"(()=>{const okay=document.querySelector('.tab-strip').offsetHeight===40&&document.querySelector('.nav-bar').offsetHeight===48&&document.querySelectorAll('.window-controls .window-control').length===3;const button=document.getElementById('profileButton'),menu=document.getElementById('profilePopover');const menuOkay=!menu.hidden&&button.getAttribute('aria-expanded')==='true'&&document.querySelectorAll('#profileList .profile-choice').length===1&&document.getElementById('profileCurrentName').textContent==='Cá nhân';showProfileCreate();document.getElementById('profileCreateName').value='Kiểm thử';document.getElementById('profileCreateForm').requestSubmit();document.getElementById('googleDnsButton').click();return okay&&menuOkay&&menu.hidden})()");
            }
            1=>{
                if !self.app.settings.google_dns{return Err("DNS toolbar did not save".into());}
                self.query(id,true,"(()=>{const button=document.getElementById('googleDnsButton');const saved=button.title.includes('áp dụng')&&button.querySelector('span').textContent==='Áp dụng';window.__renderGoogleDns(true,true,false,null);const checking=button.title.includes('đang kiểm tra')&&button.querySelector('span').textContent==='G DNS';window.__renderGoogleDns(true,true,false,true);const verified=button.title.includes('đã xác minh');window.__renderGoogleDns(true,true,false,false);const routeFailure=button.dataset.failed==='true'&&button.dataset.pending==='false'&&button.title.includes('chưa kết nối');window.__renderGoogleDns(true,false,true,null);const startupFailure=button.dataset.failed==='true'&&button.dataset.pending==='false'&&button.title.includes('không khởi động được')&&button.querySelector('span').textContent==='Cài đặt';window.__renderGoogleDns(false,false,true,null);const pendingOff=button.dataset.pending==='true'&&button.querySelector('span').textContent==='Áp dụng'&&button.title.includes('chưa áp dụng');window.__renderGoogleDns(true,false,false,null);postIpc('OpenSettings');return saved&&checking&&verified&&routeFailure&&startupFailure&&pendingOff})()");
            }
            2 => self.query(
                id,
                false,
                r#"(()=> {
                    if (window.__pipDnsProbeTestsStarted) {
                        const completed = window.__pipDnsProbeTestsResult;
                        return completed ? (completed.pass ? true : JSON.stringify(completed)) : null;
                    }

                    const dnsRow=document.getElementById('googleDnsRestartRow');
                    const hint=document.getElementById('googleDnsHint');
                    const dns=document.getElementById('googleDnsToggle');
                    const input=document.getElementById('askWhereToSaveToggle');
                    const clear=document.getElementById('clearSiteDataButton');
                    const clearTypes=['clearDataHistory','clearDataSite','clearDataCache','clearDataPasswords','clearDataAutofill','clearDataDownloads'];
                    const details={dnsRow:!!dnsRow,rowHidden:dnsRow?.hidden,rowDisplay:dnsRow?getComputedStyle(dnsRow).display:null,hint:hint?.textContent,dnsToggle:!!dns,input:!!input,clear:!!clear,clearText:clear?.textContent,clearOnclick:clear?.getAttribute('onclick'),missingClearTypes:clearTypes.filter(id=>!document.getElementById(id)),clearFn:typeof clearSelectedBrowsingData,clearButtonOnclick:document.getElementById('clearSelectedBrowsingDataButton')?.getAttribute('onclick')};

                    if (!dnsRow || dnsRow.hidden || getComputedStyle(dnsRow).display==='none' ||
                        !hint.textContent.includes('proxy chưa được áp dụng') || !dns || !input || !clear ||
                        !clear.textContent.includes('Xóa dữ liệu website') ||
                        !clear.getAttribute('onclick').includes('ClearSiteData') ||
                        clearTypes.some(id=>!document.getElementById(id)) ||
                        typeof clearSelectedBrowsingData!=='function' ||
                        !document.getElementById('clearSelectedBrowsingDataButton').getAttribute('onclick').includes('clearSelectedBrowsingData')) {
                        return JSON.stringify(details);
                    }

                    window.__renderGoogleDns(true,false,true,null);
                    const failure=hint.textContent.includes('không khởi động được')&&dnsRow.hidden;
                    window.__renderGoogleDns(true,false,false,null);
                    dns.checked=false;
                    dns.onchange();
                    const before=input.checked;
                    input.click();
                    for(const id of clearTypes)document.getElementById(id).checked=false;
                    clearSelectedBrowsingData();
                    const emptySelection=document.getElementById('statusText').textContent.includes('ít nhất một loại');
                    window.confirm=()=>true;
                    document.getElementById('clearDataCache').checked=true;
                    clearSelectedBrowsingData();
                    const settingsPass=failure&&before&&!input.checked&&emptySelection&&typeof window.__pipSend==='function';
                    if (!settingsPass) {
                        return JSON.stringify({failure,before,inputAfter:input.checked,emptySelection,pipSend:typeof window.__pipSend});
                    }
                    if (typeof window.__pipGoogleDnsWebViewProbe !== 'function') {
                        return JSON.stringify({probeMissing:true});
                    }

                    window.__pipDnsProbeTestsStarted=true;
                    (async()=>{
                        let request;
                        const resolved=await window.__pipGoogleDnsWebViewProbe((url,init)=>{
                            request={
                                url,
                                method:init.method,
                                mode:init.mode,
                                credentials:init.credentials,
                                referrer:init.referrer,
                                referrerPolicy:init.referrerPolicy,
                                cache:init.cache,
                                hasSignal:!!init.signal
                            };
                            return Promise.resolve({type:'opaque',status:0});
                        },100);
                        const rejected=await window.__pipGoogleDnsWebViewProbe(
                            ()=>Promise.reject(new TypeError('simulated network failure')),
                            100
                        );
                        const timeout=await window.__pipGoogleDnsWebViewProbe(
                            ()=>new Promise(()=>{}),
                            20
                        );
                        window.__pipDnsProbeTestsResult={
                            pass:resolved.ok===true&&resolved.kind==='response'&&
                                rejected.ok===false&&rejected.kind==='network'&&
                                timeout.ok===false&&timeout.kind==='timeout'&&
                                request.url==='https://www.youtube.com/generate_204'&&
                                request.method==='GET'&&request.mode==='no-cors'&&
                                request.credentials==='omit'&&request.referrer===''&&
                                request.referrerPolicy==='no-referrer'&&request.cache==='no-store'&&
                                request.hasSignal,
                            resolved,rejected,timeout,request
                        };
                    })().catch(error=>{
                        window.__pipDnsProbeTestsResult={pass:false,error:String(error)};
                    });
                    return null;
                })()"#,
            ),
            3=>{
                if self.app.settings.downloads.ask_where_to_save {return Err("Settings checkbox did not reach native storage".into());}
                if self.app.settings.google_dns {return Err("Google DNS setting did not return to its original state".into());}
                let disk=evergreen_core::settings::Settings::load_from_path(&get_settings_path()).map_err(|e|e.to_string())?;
                if disk.downloads.ask_where_to_save {return Err("Nested settings were not persisted".into());}
                if disk.google_dns {return Err("Google DNS setting was not persisted".into());}
                self.query(id,false,"document.getElementById('statusText')?.textContent.includes('Đã xóa dữ liệu đã chọn') ? true : null");
            }
            4=>{
                self.privacy_test_tab=self.app.windows[&id].tab_manager.active_tab_id();
                self.query(id,false,"(()=>{if(document.title!=='Pip native test')return null;localStorage.setItem('pip-secret','normal');document.cookie='pip_secret=normal; SameSite=Lax';if(!window.__privacySeedStarted){window.__privacySeedStarted=true;window.__privacySeedReady=false;window.__privacySeedError=null;(async()=>{await new Promise((resolve,reject)=>{const request=indexedDB.open('pip-privacy-isolation',1);request.onupgradeneeded=()=>request.result.createObjectStore('marker');request.onerror=()=>reject(request.error);request.onsuccess=()=>{const db=request.result,tx=db.transaction('marker','readwrite');tx.objectStore('marker').put('normal','context');tx.oncomplete=()=>{db.close();resolve()};tx.onerror=()=>reject(tx.error)}});const cache=await caches.open('pip-privacy-isolation');await cache.put('/pip-privacy-marker',new Response('normal'));await navigator.serviceWorker.register('/privacy-isolation-sw.js');await navigator.serviceWorker.ready;window.__privacySeedReady=true})().catch(error=>window.__privacySeedError=String(error))}if(window.__privacySeedError)return JSON.stringify({seedError:window.__privacySeedError});if(!window.__privacySeedReady)return null;return typeof window.__pipSend==='undefined'?true:JSON.stringify({pipSend:typeof window.__pipSend})})()")
            }
            5=>{
                let path=self.app.library_path(id).ok_or("Library path missing")?;
                let library=evergreen_core::library::Library::load(&path).map_err(|e|e.to_string())?;
                self.removed_history_visit=library.history_visits.iter().filter(|visit|visit.url==self.url).max_by_key(|visit|visit.visited_at_ms).map(|visit|(visit.url.clone(),visit.visited_at_ms));
                if self.removed_history_visit.is_none(){return Err("History fixture visit missing before removal".into());}
                self.query(id,true,"(()=>{document.getElementById('bookmarkButton').click();postIpc('OpenLibrary',{view:'bookmarks'});return true})()")
            },
            6=>self.query(id,false,"(()=>{const portable=document.getElementById('importBookmarksButton')?.dataset.action==='ImportBookmarks'&&document.getElementById('exportBookmarksButton')?.dataset.action==='ExportBookmarks';const check=kind=>{const input=document.getElementById(kind+'Search'),list=document.getElementById(kind+'List'),empty=document.getElementById(kind+'NoResults');if(!input||!list.textContent.includes('Pip native test'))return false;input.value='pip native';input.dispatchEvent(new Event('input'));const matchCount=list.querySelectorAll('.library-row').length;const matched=kind==='bookmarks'?matchCount===1:matchCount>=1;input.value='not found';input.dispatchEvent(new Event('input'));const emptyState=empty.hidden===false&&list.querySelectorAll('.library-row').length===0;input.value='';input.dispatchEvent(new Event('input'));const finalCount=list.querySelectorAll('.library-row').length;return matched&&emptyState&&(kind==='bookmarks'?finalCount===1:finalCount>=1)};const bookmarks=check('bookmarks');document.querySelector('.rail-link[data-view=history]').click();const history=check('history');const row=[...document.querySelectorAll('#historyList .library-row')].find(item=>item.textContent.includes('Pip native test'));row?.querySelector('button')?.click();return portable&&bookmarks&&history&&!!row?true:JSON.stringify({portable,bookmarks,history,row:!!row,bookmarkText:document.querySelector('#bookmarksList')?.textContent,historyText:document.querySelector('#historyList')?.textContent})})()"),
            7=>{
                let path=self.app.library_path(id).ok_or("Library path missing")?;
                let library=evergreen_core::library::Library::load(&path).map_err(|e|e.to_string())?;
                let (removed_url,removed_at_ms)=self.removed_history_visit.as_ref().ok_or("History fixture visit was not recorded before removal")?;
                if library.history_visits.iter().any(|visit|&visit.url==removed_url&&visit.visited_at_ms==*removed_at_ms){return Err("Removing a history entry did not persist".into());}
                self.query(id,true,"(()=>{postIpc('OpenPrivateWindow');return true})()");
            }
            8=>{
                let private=self.app.windows.values().find(|w|w.private).ok_or("Private window missing")?.id;
                self.app.handle_navigate(private,&(self.url.clone()+"private"));self.step+=1;
            }
            9=>{
                let private=self.app.windows.values().find(|w|w.private).ok_or("Private window missing")?.id;
                let private_view=self.app.windows[&private].tab_manager.active_tab_id().and_then(|tab|self.app.windows[&private].tabs.get(&tab)).ok_or("Private WebView missing for profile-mode check")?;
                if !profile_private_mode(private_view)? {return Err("Private window's WebView2 profile is not in InPrivate mode".into());}
                self.query(private,false,"(()=>{if(document.title!=='Pip native test')return null;if(!window.__privacyProbeStarted){window.__privacyProbeStarted=true;window.__privacyProbeReady=false;(async()=>{const databases=await indexedDB.databases();const cacheNames=await caches.keys();const registration=await navigator.serviceWorker.getRegistration('/');window.__privacyProbeState={databases:databases.map(db=>db.name),cacheNames,hasServiceWorker:!!registration}})().then(()=>window.__privacyProbeReady=true).catch(()=>window.__privacyProbeReady=false)}if(!window.__privacyProbeReady)return null;const state=window.__privacyProbeState;const isolated=localStorage.getItem('pip-secret')===null&&!document.cookie.includes('pip_secret')&&typeof window.__pipSend==='undefined'&&state.databases.length===0&&!state.cacheNames.includes('pip-privacy-isolation')&&!state.hasServiceWorker;if(!isolated)return false;localStorage.setItem('pip-private-lifetime','must-disappear');document.cookie='pip_private_lifetime=must-disappear; SameSite=Lax';return true})()");
            }
            92=>{
                let private=self.app.windows.values().find(|w|w.private).ok_or("Private window missing before lifetime cleanup")?.id;
                self.app.close_window(private,events);
                if self.app.windows.values().any(|w|w.private){return Err("Closing the last InPrivate window left another private window open".into());}
                self.app.create_profile_window(events,Some(self.url.clone()+"private-reopen"),"Default".into(),true,None).ok_or("Could not open a fresh InPrivate window after closing the last one")?;
                self.step=93;
            }
            93=>{
                let private=self.app.windows.values().find(|w|w.private).ok_or("Fresh InPrivate window missing")?.id;
                self.query(private,false,"(()=>document.title!=='Pip native test'?null:(localStorage.getItem('pip-private-lifetime')===null&&!document.cookie.includes('pip_private_lifetime=must-disappear'))?true:JSON.stringify({localStorage:localStorage.getItem('pip-private-lifetime'),cookie:document.cookie}))()");
            }
            10=>{
                let path=self.app.library_path(id).unwrap();let library=evergreen_core::library::Library::load(&path).map_err(|e|e.to_string())?;
                if library.history.iter().any(|p|p.url.ends_with("/private")){return Err("Private history leaked".into());}
                self.app.handle_navigate(id,&(self.url.clone()+"download"));self.step+=1;
            }
            11=>{
                let path=self.app.settings.downloads.default_folder.join("native-test.txt");
                let completed=self.app.windows[&id].downloads.values().any(|download|download["state"]=="Completed"&&download["path"].as_str().is_some_and(|download_path|std::path::Path::new(download_path)==path));
                if !completed||std::fs::read(&path).ok().as_deref()!=Some(b"Pip download test\n"){return Ok(())}
                self.app.handle_navigate(id,&(self.url.clone()+"download"));self.step+=1;
            }
            12=>{
                let win=&self.app.windows[&id];
                if win.downloads.len()!=2 || !win.downloads.values().all(|d|d["state"]=="Completed") || !self.app.settings.downloads.default_folder.join("native-test (1).txt").is_file(){return Ok(())}
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
                let profile_view=self.app.windows[&profile].tab_manager.active_tab_id().and_then(|tab|self.app.windows[&profile].tabs.get(&tab)).ok_or("Persistent WebView missing for profile-mode check")?;
                if profile_private_mode(profile_view)? {return Err("Persistent profile unexpectedly entered InPrivate mode".into());}
                self.query(profile,false,"(()=>{if(document.title!=='Pip native test')return null;if(!window.__privacyProbeStarted){window.__privacyProbeStarted=true;window.__privacyProbeReady=false;(async()=>{const databases=await indexedDB.databases();const cacheNames=await caches.keys();const registration=await navigator.serviceWorker.getRegistration('/');window.__privacyProbeState={databases:databases.map(db=>db.name),cacheNames,hasServiceWorker:!!registration}})().then(()=>window.__privacyProbeReady=true).catch(()=>window.__privacyProbeReady=false)}if(!window.__privacyProbeReady)return null;const state=window.__privacyProbeState;return localStorage.getItem('pip-secret')===null&&!document.cookie.includes('pip_secret')&&state.databases.length===0&&!state.cacheNames.includes('pip-privacy-isolation')&&!state.hasServiceWorker?true:false})()");
            }
            15=>{
                let tab=self.privacy_test_tab.ok_or("Privacy isolation test tab missing for cleanup")?;
                self.query_tab(id,tab,"(()=>{if(!window.__pipPrivacyCleanupStarted){window.__pipPrivacyCleanupStarted=true;window.__pipPrivacyCleanupDone=false;window.__pipPrivacyCleanupError=null;(async()=>{const registration=await navigator.serviceWorker.getRegistration('/');if(registration){const script=registration.active?.scriptURL||registration.waiting?.scriptURL||registration.installing?.scriptURL;if(!script?.endsWith('/privacy-isolation-sw.js'))throw new Error('unexpected service worker: '+script);if(!await registration.unregister())throw new Error('service worker unregister failed')}await caches.delete('pip-privacy-isolation');await new Promise((resolve,reject)=>{const request=indexedDB.deleteDatabase('pip-privacy-isolation');request.onsuccess=()=>resolve();request.onerror=()=>reject(request.error);request.onblocked=()=>reject(new Error('database delete blocked'))})})().then(()=>window.__pipPrivacyCleanupDone=true).catch(error=>window.__pipPrivacyCleanupError=String(error))}if(window.__pipPrivacyCleanupError)return JSON.stringify({cleanupError:window.__pipPrivacyCleanupError});return window.__pipPrivacyCleanupDone?true:null})()");
            }
            150=>self.query(id,true,"(()=>{const button=document.getElementById('profileButton');button.click();const names=[...document.querySelectorAll('#profileList .profile-choice-name')].map(n=>n.textContent);return !document.getElementById('profilePopover').hidden&&names.includes('Cá nhân')&&names.includes('Kiểm thử')})()"),
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
                let previous=self.app.windows.get(&id).and_then(|win|win.tab_manager.active_tab_id()).ok_or("No active page before library shortcut test")?;
                let before_count=self.app.windows[&id].tab_manager.tabs().len();
                for (shortcut,expected_route) in [
                    ("Ctrl+H","evergreen://newtab?history"),
                    ("Ctrl+Shift+O","evergreen://newtab?bookmarks"),
                ] {
                    self.app.user_event(events,BrowserEvent::Shortcut(id,shortcut.into()));
                    let win=self.app.windows.get(&id).ok_or("Library shortcut closed its window")?;
                    let active=win.tab_manager.active_tab_id().ok_or("Library shortcut did not open a tab")?;
                    let route=win.tab_manager.get_tab_by_id(active).map(|tab|tab.url.as_str()).unwrap_or("");
                    if win.tab_manager.tabs().len()!=before_count+1 || active==previous || route!=expected_route || !win.tabs.contains_key(&previous) {
                        return Err(format!("{shortcut} did not open its manager in a new tab while preserving the active page"));
                    }
                    self.app.handle_close_tab(id,active,events);
                    let win=self.app.windows.get(&id).ok_or("Closing the library tab closed its window")?;
                    if win.tab_manager.tabs().len()!=before_count || win.tab_manager.active_tab_id()!=Some(previous) || !win.tabs.contains_key(&previous) {
                        return Err(format!("Closing the {shortcut} tab did not return to the original page"));
                    }
                }
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
                let win = self.app.windows.get_mut(&id).unwrap();
                let active_tab = win.tab_manager.active_tab_id();
                let closed_id = win.tab_manager.create_tab("https://recently-closed.example/", BrowserApp::now_secs());
                win.tab_manager.close_tab(closed_id);
                if let Some(active_tab) = active_tab {
                    win.tab_manager.switch_tab(active_tab, BrowserApp::now_secs());
                }
                self.app.save_session(id);
                let restored=self.app.create_profile_window(events,Some("evergreen://newtab".into()),"Default".into(),false,Some(0)).ok_or("Restore test window missing")?;
                self.restored_window=Some(restored);
                self.step+=1;
            }
            23=>{
                let restored=self.restored_window.unwrap();
                let win=&self.app.windows[&restored];
                if win.tab_manager.clone().pop_last_closed().as_deref()!=Some("https://recently-closed.example/") {return Err("Recently closed tabs were not restored from the profile session".into())}
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
                if self.app.windows[&restored].tabs.len()!=2{return Err(format!("Selecting a deferred tab created unrelated views ({} views, {} states)",self.app.windows[&restored].tabs.len(),self.app.windows[&restored].tab_manager.tabs().len()));}
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
                if self.private_permission_check_phase == 0 {
                    let private_id=self.app.windows.values().find(|window|window.private).ok_or("Private window missing for permission isolation check")?.id;
                    self.query(private_id,false,"(()=>{if(!window.__pipPrivatePermissionProbeStarted){window.__pipPrivatePermissionProbeStarted=true;window.__pipPrivatePermissionProbeResult=null;navigator.permissions.query({name:'camera'}).then(permission=>window.__pipPrivatePermissionProbeResult=permission.state==='prompt').catch(()=>window.__pipPrivatePermissionProbeResult=false)}return window.__pipPrivatePermissionProbeResult===null?null:window.__pipPrivatePermissionProbeResult===true})()");
                } else {
                    let origin=serde_json::to_string(self.url.trim_end_matches('/')).unwrap();
                    self.query(id,false,&format!("(()=>{{const list=document.getElementById('sitePermissionsList'),render=window.__renderSitePermissions;if(!list||!render)return null;const actual=[...list.querySelectorAll('.site-permission-item')].find(row=>row.textContent.includes({origin}));if(!actual)return null;if(!actual.textContent.includes('Camera')||!actual.textContent.includes('Được phép'))return false;render([{{origin:'<img src=x onerror=alert(1)>',kind_name:'Camera',state_name:'Được phép',kind_id:2}}]);const safe=!list.querySelector('img')&&list.textContent.includes('<img src=x onerror=alert(1)>');render([{{origin:{origin},kind_name:'Camera',state_name:'Được phép',kind_id:2}}]);const row=list.querySelector('.site-permission-item'),button=row?.querySelector('button');if(!safe||!button)return false;window.confirm=()=>true;button.click();return true}})()"));
                }
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
            120=>self.query(id,false,"(()=>{document.body.innerHTML='<p data-match=\"one\">pipfindunique</p><p data-match=\"two\">pipfindunique</p>';return true})()"),
            121=>{
                let chrome=self.app.windows[&id].chrome_webview.as_ref().ok_or("Chrome WebView missing for find-next shortcut test")?;
                chrome.evaluate_script("document.getElementById('findInput').value='pipfindunique'").map_err(|error|error.to_string())?;
                self.app.user_event(events,BrowserEvent::Shortcut(id,"FindNext".into()));
                self.step=122;
            }
            122=>self.query(id,false,"(()=>{const p=getSelection().anchorNode?.parentElement?.closest('p');return p?.dataset.match==='one'?true:null})()"),
            123=>{
                self.app.user_event(events,BrowserEvent::Shortcut(id,"FindNext".into()));
                self.step=124;
            }
            124=>self.query(id,false,"(()=>{const p=getSelection().anchorNode?.parentElement?.closest('p');return p?.dataset.match==='two'?true:null})()"),
            125=>{
                self.app.user_event(events,BrowserEvent::Shortcut(id,"FindPrevious".into()));
                self.step=126;
            }
            126=>self.query(id,false,"(()=>{const p=getSelection().anchorNode?.parentElement?.closest('p');return p?.dataset.match==='one'?true:null})()"),
            127=>self.query(id,false,"document.title==='Pip native test' ? true : null"),
            39=>{
                let tab_id=self.app.windows[&id].tab_manager.active_tab_id().unwrap();
                self.app.user_event(events,BrowserEvent::Ipc(id,UiToHostMessage::SetTabGroup{tab_id,name:"Research".into()}));
                self.query(id,true,"(()=>{const tab=document.getElementById('tab-'+activeTabId);return !!tab&&tab.classList.contains('grouped')&&tab.style.getPropertyValue('--tab-group-color')!==''&&tab.getAttribute('aria-label').includes('nhóm Research')&&typeof tab.oncontextmenu==='function'})()");
            }
            40=>{
                let tab_id=self.app.windows[&id].tab_manager.active_tab_id().unwrap();
                if self.app.windows[&id].tab_manager.get_tab_by_id(tab_id).and_then(|tab|tab.group.as_ref()).map(|group|group.name.as_str())!=Some("Research") {return Err("Tab group assignment did not reach native tab state".into());}
                let win=&self.app.windows[&id];
                let saved_windows=load_session_windows(&get_data_directory().join("sessions").join(format!("{}.json",win.profile_id)));
                let saved_group=saved_windows.get(win.session_slot).and_then(|tabs|tabs.iter().find(|tab|tab.id==tab_id)).and_then(|tab|tab.group.as_ref()).map(|group|group.name.as_str());
                if saved_group!=Some("Research") {return Err("Tab group assignment did not persist immediately through the production IPC handler".into());}
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
                self.app.handle_navigate(id,&(self.worker_url.clone()+"worker-test"));
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
                let active_view=self.app.windows[&id].tab_manager.active_tab_id().and_then(|tab|self.app.windows[&id].tabs.get(&tab)).ok_or("Active page WebView missing for native password setting check")?;
                if password_autofill_flags(active_view)?!=(true,true){return Err("Persistent WebView2 profile did not enable native password autosave and general autofill".into())}
                self.app.settings.profiles.iter_mut().find(|profile|profile.id=="Default").ok_or("Default profile missing for password setting toggle check")?.password_autosave=false;
                self.app.settings.profiles.iter_mut().find(|profile|profile.id=="Default").unwrap().autofill=false;
                self.app.configure_autofill(active_view,"Default",false);
                if password_autofill_flags(active_view)?!=(false,false){return Err("Disabling password autosave and autofill did not update the native WebView2 profile".into())}
                self.app.settings.profiles.iter_mut().find(|profile|profile.id=="Default").unwrap().password_autosave=true;
                self.app.settings.profiles.iter_mut().find(|profile|profile.id=="Default").unwrap().autofill=true;
                self.app.configure_autofill(active_view,"Default",false);
                if password_autofill_flags(active_view)?!=(true,true){return Err("Re-enabling password autosave and autofill did not update the native WebView2 profile".into())}
                let path=self.app.passwords_path(id).ok_or("Password vault path missing")?;
                let mut entries=Vec::new();
                passwords::upsert(&mut entries,&self.url,"native-one","test-secret-one").map_err(|e|e.to_string())?;
                passwords::upsert(&mut entries,&self.url,"native-two","test-secret-two").map_err(|e|e.to_string())?;
                passwords::save(&path,&entries)?;
                self.app.handle_navigate(id,&(self.url.clone()+"password-test"));
                self.step=57;
            }
            57=>{
                let private_id=self.app.windows.values().find(|window|window.private).map(|window|window.id).ok_or("Private window missing for native password setting check")?;
                let private_view=self.app.windows[&private_id].tab_manager.active_tab_id().and_then(|tab|self.app.windows[&private_id].tabs.get(&tab)).ok_or("Private WebView missing for native password setting check")?;
                if password_autofill_flags(private_view)?!=(false,false){return Err("Private WebView2 profile unexpectedly enabled native password autosave or autofill".into())}
                self.query(id,false,"document.querySelector('#username')&&document.querySelector('#password')&&document.querySelector('#username').value===''&&document.querySelector('#password').value==='' ? true : null")
            }
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
            63=>self.query(id,false,&format!("(()=>{{const button=document.getElementById('checkAppUpdatesButton'),status=document.getElementById('appUpdateStatus'),link=document.getElementById('openAppReleaseButton'),filters=document.getElementById('updateAdblockFiltersButton'),sleep=document.getElementById('backgroundThrottlingToggle'),delay=document.getElementById('sleepAfterSelect'),saveTarget=document.getElementById('passwordSaveTargetTab'),fillTarget=document.getElementById('passwordTargetTab'),origin={};if(!button||!status||!link||!filters||!sleep||!delay||!sleep.checked||delay.value!=='300'||!saveTarget||!fillTarget||saveTarget===fillTarget||typeof window.__renderAppUpdate!=='function'||typeof window.__renderAdblockFilterUpdate!=='function')return false;const option=[...saveTarget.options].find(item=>item.dataset.origin===origin);const saveControl=saveTarget.disabled===false&&!document.getElementById('passwordOrigin')&&document.getElementById('savePasswordButton').textContent.includes('tab đã chọn');window.__renderAppUpdate({{currentVersion:'0.1.9',latestVersion:'0.1.10',updateAvailable:true,releaseUrl:'https://github.com/giangocquoc-art/pip-browser/releases/tag/v0.1.10'}});const offered=!link.hidden&&link.href==='https://github.com/giangocquoc-art/pip-browser/releases/tag/v0.1.10'&&status.textContent.includes('v0.1.10');window.__renderAppUpdate({{currentVersion:'0.1.12',latestVersion:'0.1.9',updateAvailable:false,releaseUrl:'https://github.com/giangocquoc-art/pip-browser/releases/tag/v0.1.9'}});window.__renderAdblockFilterUpdate({{listCount:4,ruleCount:1200}});const filterDone=document.getElementById('adblockUpdateStatus').textContent.includes('1.200');return !!option&&saveControl&&offered&&link.hidden&&status.textContent.includes('mới hơn')&&filterDone}})()",serde_json::to_string(&passwords::normalize_origin(&self.url).unwrap()).unwrap())),
            90=>{
                let (target_id,target_url)=self.app.windows[&id].tab_manager.tabs().iter().find(|tab|tab.url.ends_with("/password-test")).map(|tab|(tab.id,tab.url.clone())).ok_or("Password save target tab is missing")?;
                let save=|tab_id:TabId,username:&str,password:&str|UiToHostMessage::SavePasswordForTab{tab_id:tab_id.0,username:username.into(),password:password.into()};
                self.app.user_event(events,BrowserEvent::Ipc(id,save(target_id,"saved-from-selected-tab","native-save-secret")));
                let path=self.app.passwords_path(id).ok_or("Password vault path missing after target-tab save")?;
                let entries=passwords::load(&path)?;
                let origin=passwords::normalize_origin(&target_url).ok_or("Selected web tab did not have a valid origin")?;
                let saved=entries.iter().find(|entry|entry.username=="saved-from-selected-tab").ok_or("Saving against the selected tab did not create an entry")?;
                if saved.origin!=origin||saved.password!="native-save-secret"{return Err("Password was not saved using the selected tab's Rust-derived origin".into())}
                let summaries=serde_json::to_string(&passwords::summaries(&entries)).map_err(|error|error.to_string())?;
                if summaries.contains("native-save-secret"){return Err("Password summary exposed the saved secret".into())}
                let count=entries.len();
                let non_web=self.app.windows[&id].tab_manager.tabs().iter().find(|tab|tab.url.starts_with("evergreen://settings")).map(|tab|tab.id).ok_or("Internal settings tab missing for non-web rejection check")?;
                self.app.user_event(events,BrowserEvent::Ipc(id,save(non_web,"must-not-save-non-web","rejected-secret")));
                self.app.user_event(events,BrowserEvent::Ipc(id,save(TabId(u64::MAX),"must-not-save-stale","rejected-secret")));
                let private_id=self.app.windows.values().find(|window|window.private).map(|window|window.id).ok_or("Private window missing for password rejection check")?;
                let private_tab=self.app.windows[&private_id].tab_manager.active_tab_id().ok_or("Private tab missing for password rejection check")?;
                self.app.user_event(events,BrowserEvent::Ipc(private_id,save(private_tab,"must-not-save-private","rejected-secret")));
                let after=passwords::load(&path)?;
                if after.len()!=count||after.iter().any(|entry|entry.username.starts_with("must-not-save")){return Err("Non-web, stale, or private password save altered the persistent vault".into())}
                if after.iter().any(|entry|entry.password=="rejected-secret"){return Err("Rejected password save request persisted its secret".into())}
                self.step=66;
            }
            64=>{
                let tab_id=self.draft_tab.unwrap();
                let now_secs=BrowserApp::now_secs();
                let tab=self.app.windows[&id].tab_manager.get_tab_by_id(tab_id).ok_or("Dirty tab missing during memory-safety check")?;
                let last_active=tab.last_active_timestamp_secs;
                if now_secs.saturating_sub(last_active)<self.app.settings.performance.sleep_after_secs{return Ok(())}
                if self.app.sleep_checking_tabs.contains(&(id,tab_id))||self.app.suspending_tabs.contains(&(id,tab_id)){return Ok(())}
                if tab.status==evergreen_core::tabs::TabStatus::Suspended{return Err("A tab with unsaved form state was suspended".into())}
                self.query_tab(id,tab_id,"window.__pipCanDiscard?.()===false");
            }
            65=>{
                let tab_id=self.draft_tab.unwrap();
                if self.app.windows[&id].tab_manager.get_tab_by_id(tab_id).is_some_and(|tab|tab.status==evergreen_core::tabs::TabStatus::Suspended){return Err("Dirty tab suspended before preserving its draft".into())}
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
                let tab_id = self.app.windows[&id]
                    .tab_manager
                    .active_tab_id()
                    .ok_or("No active tab for gambling redirect check")?;
                self.app.windows[&id].tabs[&tab_id]
                    .evaluate_script("location.href='https://ad88vip6.com/home/register?id=940814853'")
                    .map_err(|error| error.to_string())?;
                self.step=82;
            }
            82=>self.query(id,false,"location.hostname==='127.0.0.1'&&location.pathname==='/' ? true : null"),
            83=>{
                if self.app.windows[&id].tab_manager.active_tab().map(|tab|tab.url.as_str())!=Some(self.url.as_str()){return Err("A known gambling redirect escaped current-tab navigation blocking".into())}
                if !sidebar_ui::SIDEBAR_HTML.contains("sendAction('RepairPlayback')")||!sidebar_ui::SIDEBAR_HTML.contains("Sửa lỗi phát video") {return Err("The Pip menu does not expose the confirmed video repair action".into())}
                self.app.user_event(events,BrowserEvent::Ipc(id,UiToHostMessage::RepairPlayback));
                if self.app.adblock.borrow().enabled(&self.url)!=Some(false){return Err("Video repair did not persist a site-only ad-blocking exception".into())}
                self.step=75;
            }
            75=>{
                self.app.force_memory_pressure_for_tests=true;
                if self.discard_safe_tab.is_none(){
                    self.app.handle_create_tab(id,Some(format!("{}discard-safe",self.url)));
                    self.discard_safe_tab=self.app.windows[&id].tab_manager.active_tab_id();
                    self.app.handle_create_tab(id,Some("evergreen://newtab".into()));
                }
                let tab_id=self.discard_safe_tab.unwrap();
                if let Some(tab)=self.app.windows.get_mut(&id).and_then(|win|win.tab_manager.tabs_mut().iter_mut().find(|tab|tab.id==tab_id)){
                    tab.last_active_timestamp_secs=BrowserApp::now_secs().saturating_sub(10);
                }
                self.app.settings.performance.sleep_after_secs=1;
                self.query_tab(id,tab_id,"(()=>{document.body.click();return window.__pipCanDiscard?.()===true})()");
            }
            76=>{
                let tab_id=self.discard_safe_tab.unwrap();
                if !self.app.windows[&id].tab_manager.get_tab_by_id(tab_id).is_some_and(|tab|tab.status==evergreen_core::tabs::TabStatus::Inactive){return Err("A clean page became protected by a simple click".into())}
                if let Some(tab)=self.app.windows[&id].tab_manager.get_tab_by_id(tab_id){
                    eprintln!("[native-ui] suspend candidate: url={:?}, status={:?}, age={}s, loading={}, active={}, keep_awake={}, pinned={}, media={}, protected={}, threshold={}s",tab.url,tab.status,BrowserApp::now_secs().saturating_sub(tab.last_active_timestamp_secs),tab.is_loading,self.app.windows[&id].tab_manager.active_tab_id()==Some(tab_id),tab.keep_awake,tab.pinned,tab.is_audio_playing,tab.memory_discard_protected,self.app.settings.performance.sleep_after_secs);
                }
                self.app.suspend_idle_tabs(BrowserApp::now_secs());
                self.step=77;
            }
            77=>{
                let tab_id=self.discard_safe_tab.unwrap();
                if self.app.windows[&id].tab_manager.get_tab_by_id(tab_id).map(|tab|tab.status)!=Some(evergreen_core::tabs::TabStatus::Suspended){return Ok(())}
                if let Some(tab)=self.app.windows.get_mut(&id).and_then(|win|win.tab_manager.tabs_mut().iter_mut().find(|tab|tab.id==tab_id)){
                    tab.last_active_timestamp_secs=BrowserApp::now_secs().saturating_sub(AUTO_DISCARD_IDLE_SECS+1);
                }
                self.app.suspend_idle_tabs(BrowserApp::now_secs());
                self.step=78;
            }
            78=>{
                let tab_id=self.discard_safe_tab.unwrap();
                if self.app.windows[&id].tab_manager.get_tab_by_id(tab_id).map(|tab|tab.status)!=Some(evergreen_core::tabs::TabStatus::Discarded){return Ok(())}
                if self.app.windows[&id].tabs.contains_key(&tab_id){return Err("Memory discard retained the WebView renderer".into())}
                self.app.user_event(events,BrowserEvent::Ipc(id,UiToHostMessage::SwitchTab{id:tab_id}));
                self.step=79;
            }
            79=>self.step=99,
            99=>self.query(id,false,"location.pathname==='/discard-safe'&&document.readyState==='complete' ? true : null"),
            100=>{
                if self.discard_dirty_tab.is_none(){
                    self.app.handle_create_tab(id,Some(format!("{}discard-dirty",self.url)));
                    self.discard_dirty_tab=self.app.windows[&id].tab_manager.active_tab_id();
                }
                self.query_tab(id,self.discard_dirty_tab.unwrap(),"(()=>{const guarded=()=>window.__pipCanDiscard?.()===false;const input=document.getElementById('draft');if(!input)return null;input.value='unsaved value';if(!guarded())return false;input.value=input.defaultValue;const checkbox=document.createElement('input');checkbox.type='checkbox';document.body.append(checkbox);checkbox.checked=true;if(!guarded())return false;checkbox.checked=false;const radio=document.createElement('input');radio.type='radio';document.body.append(radio);radio.checked=true;if(!guarded())return false;radio.checked=false;const select=document.createElement('select');select.innerHTML='<option selected>One</option><option>Two</option>';document.body.append(select);select.selectedIndex=1;if(!guarded())return false;select.selectedIndex=0;const file=document.createElement('input');file.type='file';document.body.append(file);const data=new DataTransfer();data.items.add(new File(['draft'],'draft.txt'));file.files=data.files;if(!guarded())return false;file.value='';checkbox.remove();radio.remove();select.remove();file.remove();input.value='unsaved value';return window.__pipCanDiscard?.()===false})()");
            }
            101=>{
                let tab_id=self.discard_dirty_tab.unwrap();
                self.app.handle_create_tab(id,Some("evergreen://newtab".into()));
                let active_tab=self.app.windows[&id].tab_manager.active_tab_id().ok_or("New tab missing while testing dirty-page discard protection")?;
                if active_tab==tab_id||self.app.windows[&id].tab_manager.get_tab_by_id(active_tab).map(|tab|tab.url.as_str())!=Some("evergreen://newtab"){return Err("Dirty discard test did not leave its new-tab page active".into())}
                if let Some(tab)=self.app.windows.get_mut(&id).and_then(|win|win.tab_manager.tabs_mut().iter_mut().find(|tab|tab.id==tab_id)){
                    tab.last_active_timestamp_secs=BrowserApp::now_secs().saturating_sub(10);
                }
                self.app.suspend_idle_tabs(BrowserApp::now_secs());
                self.step=102;
            }
            102=>{
                let tab_id=self.discard_dirty_tab.unwrap();
                if self.app.sleep_checking_tabs.contains(&(id,tab_id)) {return Ok(())}
                if self.app.windows[&id].tab_manager.get_tab_by_id(tab_id).is_some_and(|tab|tab.status==evergreen_core::tabs::TabStatus::Suspended){return Err("A tab with an unsaved form was suspended".into())}
                if self.app.windows[&id].tab_manager.active_tab_id()==Some(tab_id){return Err("Dirty discard test unexpectedly made the protected page active".into())}
                self.query_tab(id,tab_id,"(()=>{const input=document.getElementById('draft');if(!input)return null;const value=input.value;input.value=input.defaultValue;const video=document.createElement('video');Object.defineProperty(video,'paused',{value:false});document.body.append(video);const mediaBlocked=window.__pipCanDiscard?.()===false;video.remove();input.value=value;return mediaBlocked})()");
            }
            103=>{
                let tab_id=self.discard_dirty_tab.unwrap();
                self.app.windows[&id].tabs[&tab_id]
                    .evaluate_script("navigator.mediaDevices.getUserMedia({audio:true,video:true}).then(stream=>window.__pipTestStream=stream).catch(error=>window.__pipTestMediaError=String(error))")
                    .map_err(|error|error.to_string())?;
                self.step=104;
            }
            104=>{
                self.query_tab(id,self.discard_dirty_tab.unwrap(),"window.__pipTestMediaError?false:(window.__pipTestStream?window.__pipCanDiscard?.()===false:null)");
            }
            105=>{
                let tab_id=self.discard_dirty_tab.unwrap();
                if let Some(tab)=self.app.windows.get_mut(&id).and_then(|win|win.tab_manager.tabs_mut().iter_mut().find(|tab|tab.id==tab_id)){
                    tab.status=evergreen_core::tabs::TabStatus::Suspended;
                    tab.last_active_timestamp_secs=BrowserApp::now_secs().saturating_sub(AUTO_DISCARD_IDLE_SECS+1);
                }
                self.app.suspend_idle_tabs(BrowserApp::now_secs());
                self.step=106;
            }
            106=>{
                let tab_id=self.discard_dirty_tab.unwrap();
                if self.app.discard_checking_tabs.contains(&(id,tab_id)) {return Ok(())}
                let win=&self.app.windows[&id];
                if !win.tabs.contains_key(&tab_id)||win.tab_manager.get_tab_by_id(tab_id).is_some_and(|tab|tab.status==evergreen_core::tabs::TabStatus::Discarded){return Err("Dirty form or active getUserMedia stream was discarded".into())}
                self.query_tab(id,tab_id,"(()=>{window.__pipTestStream?.getTracks().forEach(track=>track.stop());const input=document.getElementById('draft');if(input)input.value=input.defaultValue;return window.__pipCanDiscard?.()===true})()");
            }
            107=>{
                let tab_id=self.discard_dirty_tab.unwrap();
                if self.app.windows[&id].tab_manager.active_tab_id()==Some(tab_id){return Err("Dirty discard test made the protected page active before release".into())}
                if self.app.windows[&id].tab_manager.get_tab_by_id(tab_id).map(|tab|tab.status)!=Some(evergreen_core::tabs::TabStatus::Suspended){return Err("Dirty discard test lost its suspended target before release".into())}
                self.app.suspend_idle_tabs(BrowserApp::now_secs());
                self.step=108;
            }
            108=>{
                let tab_id=self.discard_dirty_tab.unwrap();
                if self.app.windows[&id].tab_manager.get_tab_by_id(tab_id).map(|tab|tab.status)!=Some(evergreen_core::tabs::TabStatus::Discarded){return Ok(())}
                if self.app.windows[&id].tabs.contains_key(&tab_id){return Err("Safe tab was not discarded after form/media protection cleared".into())}
                self.app.force_memory_pressure_for_tests=false;
                if let Some(folder)=self.extension_folder.take(){let _=std::fs::remove_dir_all(folder);}
                self.app.adblock.borrow_mut().toggle(&self.url).map_err(|error|error.to_string())?;
                if self.app.adblock.borrow().enabled(&self.url) != Some(true) {
                    return Err("Adblock must remain enabled before the popup navigation checks".into());
                }
                self.app.handle_navigate(id,&self.url);
                self.step=109;
            }
            109=>{
                if self.app.windows[&id].tab_manager.active_tab().map(|tab|tab.url.as_str())!=Some(self.url.as_str()){return Ok(())}
                self.blocked_before_popup_test=self.app.adblock.borrow().blocked;
                self.popup_tab_count_before=self.app.windows[&id].tab_manager.tabs().len();
                self.popup_window_count_before=self.app.windows.len();
                self.query(id,false,"(()=>{window.open('https://ad88vip6.com/home/register?id=940814853','_blank');return true})()");
            }
            110=>{
                if self.app.adblock.borrow().blocked<=self.blocked_before_popup_test {
                    if self.step_started.elapsed() >= Duration::from_secs(5) {
                        return Err(format!(
                            "Known gambling popup check timed out: blocked={} before={}, tabs={} before={}, windows={} before={}",
                            self.app.adblock.borrow().blocked,
                            self.blocked_before_popup_test,
                            self.app.windows[&id].tab_manager.tabs().len(),
                            self.popup_tab_count_before,
                            self.app.windows.len(),
                            self.popup_window_count_before,
                        ));
                    }
                    return Ok(());
                }
                let tab_count=self.app.windows[&id].tab_manager.tabs().len();
                if tab_count!=self.popup_tab_count_before||self.app.windows.len()!=self.popup_window_count_before{return Err("Blocked ad88vip6.com pop-up created a Pip tab or window".into())}
                if self.app.windows[&id].tab_manager.active_tab().map(|tab|tab.url.as_str())!=Some(self.url.as_str()){return Err("Blocked ad88vip6.com pop-up navigated the opener tab".into())}
                self.blocked_before_popup_test=self.app.adblock.borrow().blocked;
                self.popup_tab_count_before=tab_count;
                self.popup_window_count_before=self.app.windows.len();
                self.query(id,false,"(()=>{window.open(new URL('/casino/register',location.href).href,'_blank');return true})()");
            }
            111=>{
                if self.app.adblock.borrow().blocked<=self.blocked_before_popup_test {
                    if self.step_started.elapsed() >= Duration::from_secs(5) {
                        return Err(format!(
                            "Local gambling popup check timed out: blocked={} before={}, tabs={} before={}, windows={} before={}",
                            self.app.adblock.borrow().blocked,
                            self.blocked_before_popup_test,
                            self.app.windows[&id].tab_manager.tabs().len(),
                            self.popup_tab_count_before,
                            self.app.windows.len(),
                            self.popup_window_count_before,
                        ));
                    }
                    return Ok(());
                }
                let tab_count=self.app.windows[&id].tab_manager.tabs().len();
                if tab_count!=self.popup_tab_count_before||self.app.windows.len()!=self.popup_window_count_before{return Err("Blocked gambling pop-up created a Pip tab or window".into())}
                if self.app.windows[&id].tab_manager.active_tab().map(|tab|tab.url.as_str())!=Some(self.url.as_str()){return Err("Blocked gambling pop-up navigated the opener tab".into())}
                self.blocked_before_popup_test=self.app.adblock.borrow().blocked;
                self.popup_tab_count_before=tab_count;
                self.popup_window_count_before=self.app.windows.len();
                self.app.handle_navigate(id,&(self.url.clone()+"async-popup-test"));
                self.step=112;
            }
            112=>{
                if self.app.windows[&id].tab_manager.active_tab().map(|tab|tab.url.as_str())!=Some(format!("{}async-popup-test",self.url).as_str())
                    ||self.app.adblock.borrow().blocked<=self.blocked_before_popup_test{return Ok(())}
                let tab_count=self.app.windows[&id].tab_manager.tabs().len();
                if tab_count!=self.popup_tab_count_before||self.app.windows.len()!=self.popup_window_count_before{return Err("A page-load timer blank popup created a Pip tab or window".into())}
                self.query(id,false,"document.title==='Pip async popup test' ? true : null");
            }
            113=>{
                self.app.handle_create_tab(id,Some(self.url.clone()+"session-window-one"));
                let first_tab=self.app.windows[&id].tab_manager.active_tab_id().unwrap();
                self.app.windows.get_mut(&id).unwrap().tab_manager.set_group(first_tab,"Session One");
                self.session_test_window=self.app.create_profile_window(events,Some(self.url.clone()+"session-window-two"),"Default".into(),false,None);
                let second_id=self.session_test_window.ok_or("Could not create second same-profile test window")?;
                let second_tab=self.app.windows[&second_id].tab_manager.active_tab_id().ok_or("Second session test window has no tab")?;
                self.app.windows.get_mut(&second_id).unwrap().tab_manager.set_group(second_tab,"Session Two");
                self.step=114;
                return Ok(());
            }
            114=>{
                let first_tab=self.app.windows[&id].tab_manager.active_tab_id().ok_or("First session test tab missing")?;
                self.query_tab(id,first_tab,"document.title==='Pip native test' ? true : null");
            }
            115=>{
                let second_id=self.session_test_window.ok_or("Second session test window missing")?;
                let second_tab=self.app.windows[&second_id].tab_manager.active_tab_id().ok_or("Second session test tab missing")?;
                self.query_tab(second_id,second_tab,"document.title==='Pip native test' ? true : null");
            }
            116=>{
                let second_id=self.session_test_window.ok_or("Second session test window missing")?;
                self.app.windows.get_mut(&id).ok_or("First session test window missing")?.session_slot=0;
                self.app.windows.get_mut(&second_id).ok_or("Second session test window missing")?.session_slot=1;
                {
                    let second_window=self.app.windows.get_mut(&second_id).ok_or("Second session test window missing")?;
                    let active=second_window.tab_manager.active_tab_id().ok_or("Second session test active tab missing")?;
                    let now=BrowserApp::now_secs();
                    for url in ["evergreen://newtab?history","evergreen://newtab?bookmarks","evergreen://settings?clear-data"] {
                        second_window.tab_manager.create_tab(url,now);
                        if !second_window.tab_manager.switch_tab(active,now) {return Err("Could not restore the active web tab before snapshot".into());}
                    }
                    second_window.sync_ui_state();
                }
                self.app.save_session(id);
                self.app.save_session(second_id);
                self.app.windows.clear();
                let manifest_path=std::env::temp_dir().join(format!("pip-native-startup-manifest-{}.json",std::process::id()));
                let profiles=startup_profiles_for_windows([
                    ("Default".into(),0,false),
                    ("Default".into(),1,false),
                    ("Default".into(),2,true),
                ]);
                persist_startup_profiles(&manifest_path,&profiles).map_err(|error|error.to_string())?;
                self.app.startup_profiles=serde_json::from_slice(&std::fs::read(&manifest_path).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;
                let _=std::fs::remove_file(manifest_path);
                verify_cold_start_child(&self.url, &profiles)?;
                self.app.resumed(events);
                let first=self.app.windows.iter().find(|(_,window)|window.profile_id=="Default"&&window.session_slot==0).map(|(id,_)|*id).ok_or("Restart did not recreate the first same-profile window")?;
                let second=self.app.windows.iter().find(|(_,window)|window.profile_id=="Default"&&window.session_slot==1).map(|(id,_)|*id).ok_or("Restart did not recreate the second same-profile window")?;
                self.session_restore_first=Some(first);
                self.session_restore_second=Some(second);
                self.main=Some(first);
                self.step=117;
            }
            117=>{
                let first=self.session_restore_first.ok_or("First restored session window missing")?;
                let second=self.session_restore_second.ok_or("Second restored session window missing")?;
                let first_win=&self.app.windows[&first];
                let second_win=&self.app.windows[&second];
                let first_tab=first_win.tab_manager.tabs().iter().find(|tab|tab.url==self.url.clone()+"session-window-one").ok_or("First same-profile window did not restore its distinct tab")?;
                let second_tab=second_win.tab_manager.tabs().iter().find(|tab|tab.url==self.url.clone()+"session-window-two").ok_or("Second same-profile window did not restore its distinct tab")?;
                if first_win.session_slot!=0||second_win.session_slot!=1||first_tab.group.as_ref().map(|group|group.name.as_str())!=Some("Session One")||second_tab.group.as_ref().map(|group|group.name.as_str())!=Some("Session Two") {
                    return Err("Same-profile session windows did not preserve separate slots and tab groups".into());
                }
                for url in ["evergreen://newtab?history","evergreen://newtab?bookmarks","evergreen://settings?clear-data"] {
                    if !second_win.tab_manager.tabs().iter().any(|tab|tab.url==url) {return Err(format!("Session restore dropped Pip's internal tab route {url}"));}
                }
                let proxy=self.app.proxy.clone();
                let cert_cache=&self.app.cert_cache;
                let win=self.app.windows.get_mut(&first).ok_or("Restored fullscreen test window missing")?;
                win.open_sidebar("history",cert_cache,proxy);
                let chrome=win.chrome_webview.as_ref().ok_or("Fullscreen test chrome WebView missing")?;
                let page_id=win.tab_manager.active_tab_id().ok_or("Fullscreen test page tab missing")?;
                let page=win.tabs.get(&page_id).ok_or("Fullscreen test page WebView missing")?;
                let sidebar=win.sidebar_webview.as_ref().ok_or("Fullscreen test sidebar WebView missing")?;
                let (chrome_bounds,chrome_visible)=webview_native_state(chrome)?;
                let (page_bounds,page_visible)=webview_native_state(page)?;
                let (sidebar_bounds,sidebar_visible)=webview_native_state(sidebar)?;
                if !chrome_visible||!page_visible||!sidebar_visible||chrome_bounds.top!=0||page_bounds.right<=page_bounds.left||page_bounds.bottom<=page_bounds.top||sidebar_bounds.right<=sidebar_bounds.left {
                    return Err(format!("Fullscreen fixture did not begin with visible chrome, page, and sidebar bounds: chrome_visible={chrome_visible} chrome=({}, {}, {}, {}), page_visible={page_visible} page=({}, {}, {}, {}), sidebar_visible={sidebar_visible} sidebar=({}, {}, {}, {})",chrome_bounds.left,chrome_bounds.top,chrome_bounds.right,chrome_bounds.bottom,page_bounds.left,page_bounds.top,page_bounds.right,page_bounds.bottom,sidebar_bounds.left,sidebar_bounds.top,sidebar_bounds.right,sidebar_bounds.bottom));
                }
                self.fullscreen_normal_bounds=Some((chrome_bounds,page_bounds,sidebar_bounds));
                self.app.user_event(events,BrowserEvent::Shortcut(first,"F11".into()));
                self.app.windows[&first].window.set_visible(false);
                self.step=118;
            }
            118=>{
                let first=self.session_restore_first.ok_or("Fullscreen test window missing")?;
                let win=&self.app.windows[&first];
                let chrome=win.chrome_webview.as_ref().ok_or("Fullscreen test chrome WebView missing")?;
                let page_id=win.tab_manager.active_tab_id().ok_or("Fullscreen test page tab missing")?;
                let page=win.tabs.get(&page_id).ok_or("Fullscreen test page WebView missing")?;
                let sidebar=win.sidebar_webview.as_ref().ok_or("Fullscreen test sidebar WebView missing")?;
                let (chrome_bounds,chrome_visible)=webview_native_state(chrome)?;
                let (page_bounds,page_visible)=webview_native_state(page)?;
                let (sidebar_bounds,sidebar_visible)=webview_native_state(sidebar)?;
                let normal=self.fullscreen_normal_bounds.ok_or("Fullscreen baseline bounds missing")?;
                let fullscreen_size=win.window.inner_size();
                let expected_page_width=fullscreen_size.width as i32;
                let expected_page_height=fullscreen_size.height as i32;
                if !win.is_fullscreen||win.window.fullscreen().is_none()||chrome_visible||sidebar_visible||!page_visible||page_bounds.left!=0||page_bounds.top!=0||page_bounds.right!=expected_page_width||page_bounds.bottom!=expected_page_height||chrome_bounds.left!=normal.0.left||chrome_bounds.top!=normal.0.top||chrome_bounds.bottom!=chrome_bounds.top||!same_rect(sidebar_bounds,normal.2) {
                    return Err(format!("F11 did not hide chrome/sidebar and expand page to the full window: fullscreen={}, chrome_visible={}, sidebar_visible={}, page={}x{}, expected={}x{}",win.is_fullscreen,chrome_visible,sidebar_visible,page_bounds.right-page_bounds.left,page_bounds.bottom-page_bounds.top,expected_page_width,expected_page_height));
                }
                self.app.user_event(events,BrowserEvent::Shortcut(first,"F11".into()));
                self.app.windows[&first].window.set_visible(false);
                self.step=119;
            }
            119=>{
                let first=self.session_restore_first.ok_or("Fullscreen test window missing")?;
                let win=&self.app.windows[&first];
                let chrome=win.chrome_webview.as_ref().ok_or("Fullscreen test chrome WebView missing")?;
                let page_id=win.tab_manager.active_tab_id().ok_or("Fullscreen test page tab missing")?;
                let page=win.tabs.get(&page_id).ok_or("Fullscreen test page WebView missing")?;
                let sidebar=win.sidebar_webview.as_ref().ok_or("Fullscreen test sidebar WebView missing")?;
                let (chrome_bounds,chrome_visible)=webview_native_state(chrome)?;
                let (page_bounds,page_visible)=webview_native_state(page)?;
                let (sidebar_bounds,sidebar_visible)=webview_native_state(sidebar)?;
                let normal=self.fullscreen_normal_bounds.ok_or("Fullscreen baseline bounds missing")?;
                if win.is_fullscreen||win.window.fullscreen().is_some()||!chrome_visible||!sidebar_visible||!page_visible||!same_rect(chrome_bounds,normal.0)||!same_rect(page_bounds,normal.1)||!same_rect(sidebar_bounds,normal.2) {
                    return Err("Second F11 did not restore normal WebView visibility and exact bounds".into());
                }
                self.app.user_event(events,BrowserEvent::Shortcut(first,"F11".into()));
                self.app.windows[&first].window.set_visible(false);
                self.step=128;
            }
            128=>{
                let first=self.session_restore_first.ok_or("Fullscreen test window missing")?;
                if !self.app.windows[&first].is_fullscreen||self.app.windows[&first].window.fullscreen().is_none() {return Err("F11 did not re-enter fullscreen before Escape check".into());}
                self.app.user_event(events,BrowserEvent::Shortcut(first,"Escape".into()));
                self.app.windows[&first].window.set_visible(false);
                self.step=129;
            }
            129=>{
                let first=self.session_restore_first.ok_or("Fullscreen test window missing")?;
                let win=&self.app.windows[&first];
                let chrome=win.chrome_webview.as_ref().ok_or("Fullscreen test chrome WebView missing")?;
                let page_id=win.tab_manager.active_tab_id().ok_or("Fullscreen test page tab missing")?;
                let page=win.tabs.get(&page_id).ok_or("Fullscreen test page WebView missing")?;
                let sidebar=win.sidebar_webview.as_ref().ok_or("Fullscreen test sidebar WebView missing")?;
                let (chrome_bounds,chrome_visible)=webview_native_state(chrome)?;
                let (page_bounds,page_visible)=webview_native_state(page)?;
                let (sidebar_bounds,sidebar_visible)=webview_native_state(sidebar)?;
                let normal=self.fullscreen_normal_bounds.ok_or("Fullscreen baseline bounds missing")?;
                if win.is_fullscreen||win.window.fullscreen().is_some()||!chrome_visible||!sidebar_visible||!page_visible||!same_rect(chrome_bounds,normal.0)||!same_rect(page_bounds,normal.1)||!same_rect(sidebar_bounds,normal.2) {
                    return Err("Escape did not exit fullscreen and restore chrome/sidebar/page bounds".into());
                }
                self.send_escape(first)?;
                self.step=130;
            }
            130=>{
                let first=self.session_restore_first.ok_or("Escape test window missing")?;
                let win=&self.app.windows[&first];
                let (_,sidebar_visible)=webview_native_state(win.sidebar_webview.as_ref().ok_or("Escape destroyed the retained sidebar WebView")?)?;
                if win.is_sidebar_open||sidebar_visible {return Err("Escape handler did not close the visible sidebar cleanly".into());}
                self.send_escape(first)?;
                self.step=131;
            }
            131=>{
                let first=self.session_restore_first.ok_or("Escape test window missing")?;
                let win=&self.app.windows[&first];
                let sidebar=win.sidebar_webview.as_ref().ok_or("Escape destroyed the hidden sidebar WebView instead of stopping the page")?;
                let (_,sidebar_visible)=webview_native_state(sidebar)?;
                if win.is_sidebar_open||sidebar_visible {return Err("Escape reopened or showed the closed sidebar".into());}
                self.complete=true;
                events.exit();
            }
            _=>return Err(format!("Unexpected native verifier step {}",self.step)),
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
                self.pending_started = None;
                if reply == "null" {
                    self.next = Instant::now() + Duration::from_millis(500);
                    return;
                }
                if reply != "true" {
                    self.error = Some(format!("UI step {} returned {reply}", self.step));
                    events.exit();
                    return;
                }
                if self.step == 15 {
                    let id = self.main.unwrap();
                    let tab = self.privacy_test_tab.unwrap();
                    self.app
                        .windows
                        .get_mut(&id)
                        .unwrap()
                        .handle_switch_tab(tab, &self.app.cert_cache);
                    self.app
                        .handle_navigate(id, &(self.url.clone() + "privacy-cleanup-complete"));
                    self.step = 150;
                    self.next = Instant::now() + Duration::from_millis(400);
                    return;
                }
                if self.step == 150 {
                    self.step = 16;
                    self.next = Instant::now() + Duration::from_millis(400);
                    return;
                }
                if self.step == 9 {
                    self.step = 92;
                    self.next = Instant::now() + Duration::from_millis(400);
                    return;
                }
                if self.step == 93 {
                    self.step = 10;
                    self.next = Instant::now() + Duration::from_millis(400);
                    return;
                }
                if self.step == 29 && self.private_permission_check_phase < 1 {
                    self.private_permission_check_phase += 1;
                    self.next = Instant::now() + Duration::from_millis(400);
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
                    self.step = 90;
                    self.next = Instant::now() + Duration::from_millis(400);
                    return;
                }
                if self.step == 68 {
                    self.step = 69;
                    self.next = Instant::now() + Duration::from_millis(400);
                    return;
                }
                if self.step == 38 {
                    self.step = 120;
                    self.next = Instant::now() + Duration::from_millis(400);
                    return;
                }
                if self.step == 126 {
                    let id = self.main.unwrap();
                    self.app.handle_navigate(id, &self.url);
                    self.step = 127;
                    self.next = Instant::now() + Duration::from_millis(400);
                    return;
                }
                if self.step == 127 {
                    self.step = 39;
                    self.next = Instant::now() + Duration::from_millis(400);
                    return;
                }
                self.step += 1;
                self.next = Instant::now() + Duration::from_millis(400);
            }
            event => {
                if let BrowserEvent::PermissionPrompt { permission_id, .. } = &event {
                    accelerator::resolve_permission(*permission_id, true);
                    return;
                }
                if let BrowserEvent::TabSuspendSafetyResult(window_id, tab_id, safe, checked_url) =
                    &event
                {
                    if Some(*tab_id) == self.discard_safe_tab {
                        eprintln!(
                            "[native-ui] discard-safe safety result: window={window_id:?}, safe={safe}, checked_url={checked_url:?}"
                        );
                    }
                }
                if let BrowserEvent::TabSuspendResult(window_id, tab_id, suspended) = &event {
                    if Some(*tab_id) == self.discard_safe_tab {
                        eprintln!(
                            "[native-ui] discard-safe suspend result: window={window_id:?}, suspended={suspended}"
                        );
                    }
                }
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
        if self.error.is_some() {
            events.exit();
            return;
        }
        if self.error.is_some() {
            events.exit();
            return;
        }
        if self.observed_step != Some(self.step) {
            self.observed_step = Some(self.step);
            self.step_started = Instant::now();
            eprintln!("[native-ui] starting verifier step {}", self.step);
        }
        if let Some(started) = self.pending_started {
            if started.elapsed() >= Duration::from_secs(20) {
                self.error = Some(format!(
                    "Timed out waiting for WebView callback at step {} after {:?}",
                    self.step,
                    started.elapsed()
                ));
                eprintln!("[native-ui] {}", self.error.as_deref().unwrap());
                events.exit();
                return;
            }
        } else if self.step_started.elapsed() >= Duration::from_secs(30) {
            let detail = self
                .main
                .and_then(|window_id| {
                    let tab_id = if (75..=79).contains(&self.step) {
                        self.discard_safe_tab?
                    } else if (100..=108).contains(&self.step) {
                        self.discard_dirty_tab?
                    } else {
                        self.draft_tab?
                    };
                    let window = self.app.windows.get(&window_id)?;
                    let tab = window.tab_manager.get_tab_by_id(tab_id)?;
                    let now_secs = BrowserApp::now_secs();
                    Some(format!(
                        "diagnostic tab {:?}: status={:?}, url={:?}, age={}s, loading={}, active={}, keep_awake={}, pinned={}, media={}, protected={}, threshold={}s, memory_pressure={}, safety_pending={}, suspend_pending={}",
                        tab_id,
                        tab.status,
                        tab.url,
                        now_secs.saturating_sub(tab.last_active_timestamp_secs),
                        tab.is_loading,
                        window.tab_manager.active_tab_id() == Some(tab_id),
                        tab.keep_awake,
                        tab.pinned,
                        tab.is_audio_playing,
                        tab.memory_discard_protected,
                        self.app.settings.performance.sleep_after_secs,
                        self.app.force_memory_pressure_for_tests,
                        self.app.sleep_checking_tabs.contains(&(window_id, tab_id)),
                        self.app.suspending_tabs.contains(&(window_id, tab_id)),
                    ))
                })
                .unwrap_or_else(|| "dirty tab state unavailable".into());
            self.error = Some(format!(
                "Verifier made no progress at step {} for {:?} ({detail})",
                self.step,
                self.step_started.elapsed()
            ));
            eprintln!("[native-ui] {}", self.error.as_deref().unwrap());
            events.exit();
            return;
        }
        if Instant::now() > self.deadline {
            self.error = Some(format!(
                "Verifier exceeded total runtime at step {} (pending callback: {})",
                self.step, self.pending
            ));
            eprintln!("[native-ui] {}", self.error.as_deref().unwrap());
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

/// Re-run the real startup path in a new Pip process using a copied, isolated
/// profile fixture. This distinguishes a cold process restore from calling
/// `resumed()` again on the verifier's already initialized BrowserApp.
fn verify_cold_start_child(base_url: &str, profiles: &[String]) -> Result<(), String> {
    let source = get_data_directory();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stage = std::env::temp_dir().join(format!("Pip-cold-start-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&stage).map_err(|error| format!("Create cold-start fixture: {error}"))?;
    let _cleanup = ColdStartStage(stage.clone());

    let app_data = stage.join("roaming").join("evergreen-browser");
    let sessions = app_data.join("sessions");
    std::fs::create_dir_all(&sessions)
        .map_err(|error| format!("Create isolated session directory: {error}"))?;
    for name in [
        "settings.json",
        "sessions/Default.json",
        "sessions/Default.closed.json",
    ] {
        let from = source.join(name);
        if from.is_file() {
            let to = app_data.join(name);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("Create fixture parent: {error}"))?;
            }
            std::fs::copy(&from, &to).map_err(|error| {
                format!(
                    "Copy isolated cold-start fixture {}: {error}",
                    from.display()
                )
            })?;
        }
    }
    persist_startup_profiles(&startup_profiles_path_for(&app_data), profiles)
        .map_err(|error| format!("Write isolated startup manifest: {error}"))?;

    let artifact = stage.join("child-report.json");
    let stdout_path = stage.join("child.stdout.log");
    let stderr_path = stage.join("child.stderr.log");
    let stdout = std::fs::File::create(&stdout_path)
        .map_err(|error| format!("Create child stdout log: {error}"))?;
    let stderr = std::fs::File::create(&stderr_path)
        .map_err(|error| format!("Create child stderr log: {error}"))?;
    let executable = std::env::current_exe()
        .map_err(|error| format!("Resolve Pip verifier executable: {error}"))?;
    let mut child = Command::new(executable)
        .arg("--verify-cold-start-child")
        .arg(&artifact)
        .arg(base_url)
        .env("APPDATA", stage.join("roaming"))
        .env_remove("WEBVIEW2_USER_DATA_FOLDER")
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| format!("Launch isolated Pip cold-start child: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Cold-start child exceeded 90 seconds; stderr: {}",
                    read_log(&stderr_path)
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Wait for cold-start child: {error}"));
            }
        }
    };
    if !status.success() {
        return Err(format!(
            "Cold-start child exited with {status}; stdout: {}; stderr: {}",
            read_log(&stdout_path),
            read_log(&stderr_path)
        ));
    }
    let bytes = std::fs::read(&artifact)
        .map_err(|error| format!("Cold-start child did not produce its report: {error}"))?;
    let report: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Cold-start child report is invalid: {error}"))?;
    if report["status"] != "passed"
        || report["pid"].as_u64() == Some(u64::from(std::process::id()))
        || report["restored_windows"].as_u64() != Some(2)
        || report["restored_tab_count"]
            .as_u64()
            .is_none_or(|count| count < 5)
        || report["page_webview_count"].as_u64() != Some(2)
    {
        return Err(format!(
            "Cold-start child report failed validation: {report}"
        ));
    }
    eprintln!(
        "[native-ui] cold-start child restored two windows, expected tabs, and one page WebView per window in PID {}",
        report["pid"]
    );
    Ok(())
}

fn startup_profiles_path_for(data_directory: &std::path::Path) -> std::path::PathBuf {
    data_directory.join("restart-profiles.json")
}

fn read_log(path: &std::path::Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| format!("<could not read {}: {error}>", path.display()))
}

struct ColdStartStage(std::path::PathBuf);

impl Drop for ColdStartStage {
    fn drop(&mut self) {
        let temp = std::env::temp_dir();
        if self.0.parent() == Some(temp.as_path())
            && self
                .0
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("Pip-cold-start-"))
        {
            if let Err(error) = std::fs::remove_dir_all(&self.0) {
                eprintln!(
                    "[native-ui] cold-start fixture remains at {}: {error}",
                    self.0.display()
                );
            }
        }
    }
}

struct ColdStartChild {
    app: BrowserApp,
    artifact: std::path::PathBuf,
    expected_base: String,
    checked: bool,
    verify_deadline: Option<Instant>,
}

impl ColdStartChild {
    fn verify_restored_state(&self) -> Result<serde_json::Value, String> {
        let mut windows = self.app.windows.values().collect::<Vec<_>>();
        windows.sort_by_key(|window| window.session_slot);
        if windows.len() != 2
            || windows
                .iter()
                .any(|window| window.private || window.profile_id != "Default")
            || windows[0].session_slot != 0
            || windows[1].session_slot != 1
        {
            return Err(format!(
                "Expected two normal Default windows in session slots 0 and 1; got {:?}",
                windows
                    .iter()
                    .map(|window| (&window.profile_id, window.session_slot, window.private))
                    .collect::<Vec<_>>()
            ));
        }
        let first_url = format!("{}session-window-one", self.expected_base);
        let second_url = format!("{}session-window-two", self.expected_base);
        let first_tabs = windows[0].tab_manager.tabs();
        let second_tabs = windows[1].tab_manager.tabs();
        let first = first_tabs.iter().find(|tab| tab.url == first_url);
        let second = second_tabs.iter().find(|tab| tab.url == second_url);
        if first.is_none_or(|tab| {
            tab.group.as_ref().map(|group| group.name.as_str()) != Some("Session One")
        }) || second.is_none_or(|tab| {
            tab.group.as_ref().map(|group| group.name.as_str()) != Some("Session Two")
        }) || ![
            "evergreen://newtab?history",
            "evergreen://newtab?bookmarks",
            "evergreen://settings?clear-data",
        ]
        .iter()
        .all(|url| second_tabs.iter().any(|tab| tab.url == *url))
            || first_tabs.is_empty()
            || second_tabs.len() != 4
            || windows.iter().any(|window| window.tabs.len() != 1)
            || windows
                .iter()
                .any(|window| window.tab_manager.active_tab_id().is_none())
        {
            return Err(format!(
                "Cold-start restore did not recover distinct grouped windows and all routes: first={:?}, second={:?}, page_webviews={:?}, active_tabs={:?}",
                first_tabs.iter().map(|tab| (&tab.url, &tab.group)).collect::<Vec<_>>(),
                second_tabs.iter().map(|tab| (&tab.url, &tab.group)).collect::<Vec<_>>(),
                windows.iter().map(|window| window.tabs.len()).collect::<Vec<_>>(),
                windows.iter().map(|window| window.tab_manager.active_tab_id()).collect::<Vec<_>>()
            ));
        }
        Ok(serde_json::json!({
            "status": "passed",
            "pid": std::process::id(),
            "restored_windows": windows.len(),
            "restored_tab_count": first_tabs.len() + second_tabs.len(),
            "page_webview_count": windows.iter().map(|window| window.tabs.len()).sum::<usize>(),
            "profiles": windows.iter().map(|window| window.profile_id.clone()).collect::<Vec<_>>(),
            "session_slots": windows.iter().map(|window| window.session_slot).collect::<Vec<_>>(),
            "restored_urls": [
                first_url,
                second_url,
                "evergreen://newtab?history".to_string(),
                "evergreen://newtab?bookmarks".to_string(),
                "evergreen://settings?clear-data".to_string()
            ]
        }))
    }

    fn finish_verification(&mut self, events: &ActiveEventLoop) {
        let report = match self.verify_restored_state() {
            Ok(report) => report,
            Err(error) => {
                let _ = std::fs::write(
                    &self.artifact,
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "status": "failed",
                        "pid": std::process::id(),
                        "error": error
                    }))
                    .unwrap_or_default(),
                );
                events.exit();
                return;
            }
        };
        if let Err(error) = std::fs::write(
            &self.artifact,
            serde_json::to_vec_pretty(&report).unwrap_or_default(),
        ) {
            eprintln!("Write cold-start child report: {error}");
            events.exit();
            return;
        }
        for window in self.app.windows.values() {
            window.window.set_visible(false);
        }
        events.exit();
    }
}

impl ApplicationHandler<BrowserEvent> for ColdStartChild {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        if self.checked {
            return;
        }
        self.checked = true;
        self.app.resumed(events);
        self.verify_deadline = Some(Instant::now() + Duration::from_secs(10));
        events.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(50),
        ));
    }

    fn window_event(&mut self, events: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.app.window_event(events, id, event);
    }

    fn user_event(&mut self, events: &ActiveEventLoop, event: BrowserEvent) {
        self.app.user_event(events, event);
    }

    fn about_to_wait(&mut self, events: &ActiveEventLoop) {
        self.app.about_to_wait(events);
        let Some(deadline) = self.verify_deadline else {
            return;
        };
        let active_views_ready = self.app.windows.len() == 2
            && self.app.windows.values().all(|window| {
                window
                    .tab_manager
                    .active_tab_id()
                    .is_some_and(|id| window.tabs.contains_key(&id))
            });
        if active_views_ready || Instant::now() >= deadline {
            self.verify_deadline = None;
            self.finish_verification(events);
        } else {
            events.set_control_flow(ControlFlow::WaitUntil(
                (Instant::now() + Duration::from_millis(50)).min(deadline),
            ));
        }
    }
}

pub fn run_cold_start_child() {
    let mut args = std::env::args().skip_while(|arg| arg != "--verify-cold-start-child");
    let _mode = args.next();
    let artifact = args
        .next()
        .map(std::path::PathBuf::from)
        .expect("cold-start child requires a report path");
    let expected_base = args
        .next()
        .expect("cold-start child requires the session fixture URL prefix");
    let data_directory = get_data_directory();
    std::fs::create_dir_all(data_directory.join("webview2_data"))
        .expect("create isolated cold-start WebView2 data folder");
    std::env::set_var(
        "WEBVIEW2_USER_DATA_FOLDER",
        data_directory.join("webview2_data"),
    );
    let runtime_version = evergreen_core::env::detect_webview2_runtime()
        .expect("WebView2 Runtime is required for cold-start verification");
    let events = EventLoop::<BrowserEvent>::with_user_event()
        .with_any_thread(false)
        .build()
        .expect("create cold-start child event loop");
    let mut app = BrowserApp::new(events.create_proxy(), runtime_version);
    app.background_check = true;
    let mut child = ColdStartChild {
        app,
        artifact,
        expected_base,
        checked: false,
        verify_deadline: None,
    };
    events
        .run_app(&mut child)
        .expect("cold-start child event loop failed");
    child.app.windows.clear();
    drop(child);
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
    listener.set_nonblocking(true).unwrap();
    let url = format!(
        "http://127.0.0.1:{}/",
        listener.local_addr().unwrap().port()
    );
    let main_port = listener.local_addr().unwrap().port();
    let worker_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    worker_listener.set_nonblocking(true).unwrap();
    let worker_url = format!(
        "http://127.0.0.1:{}/",
        worker_listener.local_addr().unwrap().port()
    );
    let server_stop = Arc::new(AtomicBool::new(false));
    let server_stop_thread = server_stop.clone();
    let server_thread = std::thread::spawn(move || {
        while !server_stop_thread.load(Ordering::Relaxed) {
            let (mut socket, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(error) => {
                    eprintln!("[native-ui] local fixture server stopped: {error}");
                    break;
                }
            };
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
            } else if path == "/privacy-isolation-sw.js" {
                (
                    "Content-Type: application/javascript\r\nService-Worker-Allowed: /\r\n",
                    "self.addEventListener('install',event=>event.waitUntil(self.skipWaiting()));self.addEventListener('activate',event=>event.waitUntil(self.clients.claim()))",
                )
            } else if path == "/shared-worker.js" {
                (
                    "Content-Type: application/javascript\r\n",
                    "onconnect=event=>{const port=event.ports[0];port.onmessage=message=>{if(message.data==='probe')fetch('https://ad.doubleclick.net/pip-shared-worker-probe.js').catch(()=>null).then(()=>port.postMessage('done'))};port.start()}",
                )
            } else if path == "/async-popup-test" {
                (
                    "Content-Type: text/html\r\n",
                    "<!doctype html><title>Pip async popup test</title><body><script>window.addEventListener('load',()=>setTimeout(()=>window.open('about:blank','_blank'),0),{once:true})</script></body>",
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
    let worker_server_stop = server_stop.clone();
    let worker_server_thread = std::thread::spawn(move || {
        while !worker_server_stop.load(Ordering::Relaxed) {
            let (mut client, _) = match worker_listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(error) => {
                    eprintln!("[native-ui] worker fixture proxy stopped: {error}");
                    break;
                }
            };
            let _ = client.set_read_timeout(Some(Duration::from_secs(2)));
            let mut request = [0; 8192];
            let Ok(length) = client.read(&mut request) else {
                continue;
            };
            let Ok(mut upstream) = TcpStream::connect(("127.0.0.1", main_port)) else {
                continue;
            };
            let _ = upstream.set_read_timeout(Some(Duration::from_secs(2)));
            if upstream.write_all(&request[..length]).is_err() {
                continue;
            }
            let mut response = Vec::new();
            if upstream.read_to_end(&mut response).is_ok() {
                let _ = client.write_all(&response);
            }
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
        deadline: Instant::now() + Duration::from_secs(180),
        url,
        worker_url,
        error: None,
        complete: false,
        observed_step: None,
        step_started: Instant::now(),
        pending_started: None,
        draft_tab: None,
        discard_safe_tab: None,
        discard_dirty_tab: None,
        restored_window: None,
        session_test_window: None,
        session_restore_first: None,
        session_restore_second: None,
        fullscreen_normal_bounds: None,
        extension_folder: None,
        extension_id: None,
        extension_install_done: false,
        extension_removing: false,
        extension_remove_done: false,
        blocked_before_worker_test: 0,
        blocked_before_popup_test: 0,
        popup_tab_count_before: 0,
        popup_window_count_before: 0,
        redirected_before_test: 0,
        private_permission_check_phase: 0,
        privacy_test_tab: None,
        removed_history_visit: None,
    };
    let event_loop_result = events.run_app(&mut check);
    check.app.windows.clear();
    server_stop.store(true, Ordering::Relaxed);
    if server_thread.join().is_err() {
        eprintln!("[native-ui] local fixture server panicked during shutdown");
    }
    if worker_server_thread.join().is_err() {
        eprintln!("[native-ui] worker fixture proxy panicked during shutdown");
    }
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
    if let Err(error) = event_loop_result {
        check.error = Some(format!("Native verifier event loop failed: {error}"));
    }
    let complete = check.complete;
    let error = check.error.take();
    drop(check);
    let cleanup_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match std::fs::remove_dir_all(&stage) {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) if Instant::now() < cleanup_deadline => {
                eprintln!(
                    "[native-ui] waiting for WebView2 handles to close before cleanup: {error}"
                );
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(error) => {
                eprintln!(
                    "[native-ui] temporary verifier data remains at {}: {error}",
                    stage.display()
                );
                break;
            }
        }
    }
    if !complete {
        panic!("Native UI verification failed: {:?}", error);
    }
    let artifact = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/native-ui-verification.json");
    std::fs::write(&artifact,serde_json::to_vec_pretty(&serde_json::json!({"status":"passed","all_windows_hidden":true,"checks":["chrome dimensions","DNS toolbar/persistence","nested settings/IPC","Settings WebView2 YouTube probe handles response, network failure, and timeout without credentials or referrer","bookmarks/history","Chrome-style history/bookmarks shortcuts open a new tab while preserving the current page","native local bookmark/history omnibox suggestions through IPC, keyboard navigation, safe text rendering, and Alt+Home","profile-local quick-link add/remove through the native IPC with persistence","InPrivate storage/history","distinct downloads/no overwrite","profile cookie/storage isolation","profile-scoped persistent download history","quick profile popup and create flow","native toolbar WebView stays above page while open and restores page order on close","12 local page tabs preserve unsent form and JS state while switching","exactly one tab view visible; hidden views request LOW memory and active NORMAL when supported","idle inactive WebView2 tab suspends after configured delay, exposes an accessible sleeping state, and preserves unsent form after waking","session restore creates one WebView regardless of background tab count; selecting deferred tab creates only that view","large centered search over mascot background and quick guide","native disk cache cleanup callback succeeds and preserves cookies/localStorage","completed download COM handles released on window close","history records navigation when page title is unchanged","native Page.reload ignores cache; document reload verified","Find menu opens and focuses search","Tab group action syncs to native state and draws a visible group indicator","Page translation invokes local Translator API and restores original text","disabled downloads manager stays closed","unpacked WebView2 extension installs, content script runs, and removal stops it","procedural cosmetic ad filter removes matching ad nodes in real WebView2","service-worker and shared-worker requests pass through Brave network filtering","uBlock noop redirect resource reaches a native WebView2 response","native WebView2 profile password/autofill flags track settings and remain off in InPrivate","Alt+Shift+P leaves multiple saved logins untouched and fills the only exact-origin match","Pip password save uses the selected open tab's Rust-derived origin; stale, non-web, and private requests are rejected and summaries omit secrets","toolbar Home button and Alt+Home navigate the current tab to Pip's start page","version checker offers only a newer Pip release on the official GitHub repository"]})).unwrap()).unwrap();
    let mut report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&artifact).unwrap()).unwrap();
    report["checks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(
            "Keep Awake menu action toggles and protects a background tab from WebView2 suspension"
        ));
    report["checks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(
            "Low-memory simulation releases a long-idle safe tab renderer and restores the URL when selected"
        ));
    report["checks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(
            "Low-memory simulation preserves a background tab with an edited form"
        ));
    report["checks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(
            "Known gambling redirect is canceled in the current tab while normal navigation remains available"
        ));
    report["checks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(
            "Recently closed tab URLs survive profile session restore and retain Ctrl+Shift+T order"
        ));
    report["checks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(
            "Known ad88vip6.com and local gambling pop-ups emitted by window.open are blocked by WebView2 without creating a Pip tab"
        ));
    report["checks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(
            "A page-load timer blank popup is blocked without creating an additional Pip tab or app window"
        ));
    report["checks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(
            "Separate windows in one profile restore distinct tabs, groups, history/bookmarks views, and the safe Pip clear-data settings route without eager-loading background tabs"
        ));
    report["checks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(
            "F11 enters and exits fullscreen; shortcut Escape restores fullscreen bounds; the shared Escape handler closes an open sidebar without destroying it and leaves a hidden sidebar intact on the next Escape"
        ));
    report["checks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(
            "Tab group IPC changes are written to the profile session snapshot immediately"
        ));
    std::fs::write(&artifact, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(stage);
    println!(
        "PASS: hidden native app: profiles, DNS, password picker, release checker, smart memory saver, keep-awake tabs, tab groups, translation, extensions, ad filtering, settings, downloads, session restore, and cache cleanup."
    );
}

struct GoogleDnsWebViewCheck {
    app: BrowserApp,
    main: Option<WindowId>,
    destination: &'static str,
    expected_domain: &'static str,
    target_name: &'static str,
    proxy_count_before_navigation: Option<u64>,
    doh_resolutions_before_navigation: Option<u64>,
    connect_tunnels_before_navigation: Option<u64>,
    pending: bool,
    next: Instant,
    deadline: Instant,
    error: Option<String>,
    complete: bool,
    phase: &'static str,
    last_reply: Option<String>,
    last_navigation: Option<String>,
}

fn google_dns_verification_page_is_usable(page: &serde_json::Value) -> bool {
    page.get("loaded").and_then(serde_json::Value::as_bool) == Some(true)
        && page
            .get("offlineUiDetected")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
}

fn google_dns_offline_page_diagnostic(page: &serde_json::Value) -> String {
    let title = page
        .get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>");
    let excerpt = page
        .get("visibleTextExcerpt")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>");
    let failed_resources = page
        .get("failedResources")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    format!(
        "title={title:?}, visible_text={excerpt:?}, failed_resources={}",
        serde_json::Value::Array(failed_resources)
    )
}

impl ApplicationHandler<BrowserEvent> for GoogleDnsWebViewCheck {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        self.app.resumed(events);
        self.main = self.app.windows.keys().next().copied();
        self.phase = "resumed";
        let Some(port) = self.app.doh_proxy.as_ref().map(doh_proxy::DohProxy::port) else {
            self.error = Some("Google DNS verifier started without Pip's local proxy".into());
            events.exit();
            return;
        };
        let browser_args = dns_proxy_browser_args(true, Some(port));
        if !browser_args.contains(
            "--host-resolver-rules=\"MAP * ~NOTFOUND , EXCLUDE localhost , EXCLUDE 127.0.0.1\"",
        ) {
            self.error =
                Some("WebView2 DNS prefetch is not restricted by Pip's resolver rule".into());
            events.exit();
            return;
        }
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
                let page = serde_json::from_str::<serde_json::Value>(&reply)
                    .ok()
                    .or_else(|| {
                        serde_json::from_str::<String>(&reply)
                            .ok()
                            .and_then(|value| serde_json::from_str(&value).ok())
                    });
                let Some(page) = page else {
                    self.next = Instant::now() + Duration::from_millis(500);
                    return;
                };
                if page.get("loaded").and_then(serde_json::Value::as_bool) != Some(true) {
                    self.next = Instant::now() + Duration::from_millis(500);
                    return;
                }
                if !google_dns_verification_page_is_usable(&page) {
                    let offline_state = page
                        .get("offlineUiDetected")
                        .and_then(serde_json::Value::as_bool);
                    let reason = if offline_state == Some(true) {
                        "the offline UI is present"
                    } else {
                        "the offline UI check returned no definitive result"
                    };
                    self.error = Some(format!(
                        "WebView2 reached {} but verification failed because {reason}; {}",
                        self.target_name,
                        google_dns_offline_page_diagnostic(&page)
                    ));
                    events.exit();
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
                    self.error = Some(format!(
                        "WebView2 reached {} without connecting to Pip's local DoH proxy",
                        self.target_name
                    ));
                    events.exit();
                    return;
                }
                let doh_resolutions = self
                    .app
                    .doh_proxy
                    .as_ref()
                    .map(doh_proxy::DohProxy::successful_doh_resolutions)
                    .unwrap_or(0);
                let doh_before = self
                    .doh_resolutions_before_navigation
                    .unwrap_or(doh_resolutions);
                if doh_resolutions <= doh_before {
                    self.error = Some(format!(
                        "WebView2 reached {} through Pip's proxy without a successful Google DoH hostname resolution",
                        self.target_name
                    ));
                    events.exit();
                    return;
                }
                let connect_tunnels = self
                    .app
                    .doh_proxy
                    .as_ref()
                    .map(doh_proxy::DohProxy::successful_connect_tunnels)
                    .unwrap_or(0);
                let tunnels_before = self
                    .connect_tunnels_before_navigation
                    .unwrap_or(connect_tunnels);
                if connect_tunnels <= tunnels_before {
                    self.error = Some(format!(
                        "WebView2 reached {} without opening an upstream HTTPS CONNECT tunnel through Pip's proxy",
                        self.target_name
                    ));
                    events.exit();
                    return;
                }
                let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../target/google-dns-webview-verification.json");
                let report = serde_json::json!({
                    "status": "passed",
                    "destination": self.destination,
                    "proxy_connections": current - before,
                    "successful_doh_resolutions": doh_resolutions - doh_before,
                    "successful_https_connect_tunnels": connect_tunnels - tunnels_before,
                    "page": page,
                    "host_resolver_rule_configured": "MAP * ~NOTFOUND , EXCLUDE localhost , EXCLUDE 127.0.0.1",
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
            let doh_resolutions = self
                .app
                .doh_proxy
                .as_ref()
                .map(doh_proxy::DohProxy::successful_doh_resolutions);
            let connect_tunnels = self
                .app
                .doh_proxy
                .as_ref()
                .map(doh_proxy::DohProxy::successful_connect_tunnels);
            self.error = Some(format!(
                "Timed out waiting for the Google DNS WebView route (phase={}, url={active_url:?}, proxy={proxy_count:?}, DoH resolutions={doh_resolutions:?}, CONNECT tunnels={connect_tunnels:?}, proxy baseline={:?}, DoH baseline={:?}, CONNECT baseline={:?}, last_reply={:?}, navigation={:?}, pending={})",
                self.phase,
                self.proxy_count_before_navigation,
                self.doh_resolutions_before_navigation,
                self.connect_tunnels_before_navigation,
                self.last_reply,
                self.last_navigation,
                self.pending
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
            self.doh_resolutions_before_navigation = self
                .app
                .doh_proxy
                .as_ref()
                .map(doh_proxy::DohProxy::successful_doh_resolutions);
            self.connect_tunnels_before_navigation = self
                .app
                .doh_proxy
                .as_ref()
                .map(doh_proxy::DohProxy::successful_connect_tunnels);
            self.app.handle_navigate(window, self.destination);
            self.phase = match self.target_name {
                "YouTube" => "navigated to YouTube",
                _ => "navigated to Google",
            };
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
        let expected_domain = serde_json::to_string(self.expected_domain).unwrap_or_default();
        self.pending = true;
        if let Err(error) = view.evaluate_script_with_callback(
            &format!(
                "(()=>{{const expected={expected_domain};const hostname=location.hostname.toLowerCase();const pageText=(document.body?.innerText||'').replace(/\\s+/g,' ').trim();const resources=performance.getEntriesByType('resource').slice(-100).map(entry=>{{try{{const url=new URL(entry.name,location.href);return {{origin:url.origin,path:url.pathname.slice(0,120),status:Number.isInteger(entry.responseStatus)?entry.responseStatus:null}}}}catch{{return null}}}}).filter(Boolean);const failedResources=resources.filter(resource=>resource.status===0||resource.status>=400).slice(0,20);return {{loaded:(hostname===expected||hostname.endsWith('.'+expected))&&document.readyState==='complete'&&pageText.length>0,hostname,title:document.title,offlineUiDetected:/connect to the internet|you'?re offline|you are offline|no internet connection/i.test(pageText),visibleTextExcerpt:pageText.slice(0,1200),failedResources,resources}}}})()"
            ),
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
    let (destination, expected_domain, target_name) =
        match std::env::var("PIP_VERIFY_DOH_TARGET").as_deref() {
            Ok("youtube") => ("https://www.youtube.com/", "youtube.com", "YouTube"),
            _ => ("https://www.google.com/", "google.com", "Google"),
        };
    let mut check = GoogleDnsWebViewCheck {
        app,
        main: None,
        destination,
        expected_domain,
        target_name,
        proxy_count_before_navigation: None,
        doh_resolutions_before_navigation: None,
        connect_tunnels_before_navigation: None,
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
    println!(
        "PASS: WebView2 reached {} and opened a connection to Pip's local Google DNS proxy.",
        target_name
    );
}

#[cfg(test)]
mod google_dns_verification_tests {
    use super::google_dns_verification_page_is_usable;
    use serde_json::json;

    #[test]
    fn verifier_rejects_youtube_offline_shell_and_missing_offline_status() {
        assert!(google_dns_verification_page_is_usable(&json!({
            "loaded": true,
            "offlineUiDetected": false
        })));
        assert!(!google_dns_verification_page_is_usable(&json!({
            "loaded": true,
            "offlineUiDetected": true
        })));
        assert!(!google_dns_verification_page_is_usable(&json!({
            "loaded": true
        })));
        assert!(!google_dns_verification_page_is_usable(&json!({
            "loaded": false,
            "offlineUiDetected": false
        })));
    }
}
