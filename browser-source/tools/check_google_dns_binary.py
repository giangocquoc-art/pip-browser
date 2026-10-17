"""Trace DNS in a disposable Pip profile. No remote debugging; no personal pages.

NetLog stays in the temporary directory. Only resolver evidence is exported.
"""
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

def close_process(process):
    @ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def close(hwnd, _):
        pid=wintypes.DWORD()
        ctypes.windll.user32.GetWindowThreadProcessId(hwnd,ctypes.byref(pid))
        if pid.value==process.pid: ctypes.windll.user32.PostMessageW(hwnd,0x0010,0,0)
        return True
    ctypes.windll.user32.EnumWindows(close,0)
    try: process.wait(timeout=15)
    except subprocess.TimeoutExpired: process.terminate();process.wait(timeout=10)

source=Path(sys.argv[1] if len(sys.argv)>1 else 'target/release/evergreen-browser.exe').resolve()
directory=Path(tempfile.mkdtemp(prefix='pip-dns-check-'))
shutil.copy2(source,directory/'Pip.exe')
(directory/'portable.ini').touch()
(directory/'user_data').mkdir()
(directory/'user_data/settings.json').write_text('{"google_dns":true}')
trace=directory/'netlog.json'
env=os.environ.copy()
env['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS']=f'--log-net-log="{trace}"'
process=subprocess.Popen([str(directory/'Pip.exe'),'https://www.youtube.com/','--portable','--background-check'],env=env,cwd=directory,creationflags=subprocess.CREATE_NO_WINDOW)
try:
    time.sleep(18)
finally: close_process(process)
time.sleep(2)
data=json.loads(trace.read_text(encoding='utf-8'))
names={v:k for k,v in data['constants']['logEventTypes'].items()}
events=[{'event':names.get(e['type'],e['type']),'params':e.get('params',{})} for e in data['events'] if any(t in names.get(e['type'],'') for t in ('PROXY_CONFIG_CHANGED','DNS_TRANSACTION','DNS_OVER_HTTPS','DOH'))]
proxy_configs=[e['params'].get('new_config',{}) for e in events if e['event']=='PROXY_CONFIG_CHANGED']
local_proxy=any('127.0.0.1:' in json.dumps(config) for config in proxy_configs)
dns_transactions=[e for e in events if e['event']=='DNS_TRANSACTION']
report={'binary':str(source),'local_proxy_configured':local_proxy,'direct_dns_transactions':len(dns_transactions),'network_events':events}
out=Path('target/google-dns-verification.json');out.write_text(json.dumps(report,indent=2))
print(json.dumps(events,indent=2)[:12000])
print('Evidence:',out,'; isolated profile:',directory)
if not local_proxy: raise SystemExit('Google DNS proxy was not configured in WebView2')
if dns_transactions: raise SystemExit('WebView2 emitted direct DNS transactions; possible DNS leak')
