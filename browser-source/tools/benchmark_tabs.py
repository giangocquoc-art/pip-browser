"""Compare 12 local tabs in hidden, disposable Pip instances. Dev-only CDP, never user profiles.
Usage: python tools/benchmark_tabs.py old.exe new.exe [--sleep-test]
Requires existing developer tools: psutil and websockets. This does not benchmark real video sites.
"""
import asyncio
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import statistics
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import urlopen
import psutil
import websockets

class Handler(BaseHTTPRequestHandler):
    def handle(self):
        try: super().handle()
        except (ConnectionResetError,BrokenPipeError): pass  # Our staged browser is terminated after sampling.

    def do_GET(self):
        body=b"<!doctype html><title>Pip tab benchmark</title><p>Local static fixture</p><input aria-label='Draft'>"
        self.send_response(200)
        self.send_header('Content-Type','text/html; charset=utf-8')
        self.send_header('Content-Length',str(len(body)))
        self.send_header('Cache-Control','no-store')
        self.end_headers()
        self.wfile.write(body)
    def log_message(self,*args): pass

async def run(binary,url):
    binary=Path(binary).resolve(strict=True)
    directory=Path(tempfile.mkdtemp(prefix='pip-tab-benchmark-'))
    staged=directory/'Pip.exe'
    shutil.copy2(binary,staged)
    (directory/'portable.ini').touch()
    with socket.socket() as sock:
        sock.bind(('127.0.0.1',0))
        port=sock.getsockname()[1]
    env=os.environ.copy()
    env.pop('WEBVIEW2_USER_DATA_FOLDER',None)
    env['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS']=f'--remote-debugging-port={port} --remote-debugging-address=127.0.0.1'
    proc=subprocess.Popen([str(staged),url,'--portable','--background-check'],cwd=directory,env=env,
                          creationflags=subprocess.CREATE_NO_WINDOW|subprocess.BELOW_NORMAL_PRIORITY_CLASS,
                          stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    parent=psutil.Process(proc.pid)
    async def targets():
        def read():
            with urlopen(f'http://127.0.0.1:{port}/json',timeout=2) as response: return json.load(response)
        return await asyncio.to_thread(read)
    async def wait_for(predicate):
        deadline=asyncio.get_running_loop().time()+40
        while asyncio.get_running_loop().time()<deadline:
            assert proc.poll() is None, f'Pip exited: {proc.returncode}'
            try:
                pages=await targets()
                result=predicate(pages)
                if result:return result
            except (OSError,ValueError): pass
            await asyncio.sleep(.1)
        raise AssertionError('Timed out waiting for benchmark tabs')
    try:
        chrome=await wait_for(lambda pages:next((p for p in pages if p.get('title')=='Pip'),None))
        async with websockets.connect(chrome['webSocketDebuggerUrl'],max_size=8*1024*1024) as ws:
            if sleep_test:
                expression='postIpc("SaveSettings",{settings_json:JSON.stringify({performance:{sleep_after_secs:10,background_throttling:true}})})'
                await ws.send(json.dumps({'id':100,'method':'Runtime.evaluate','params':{'expression':expression,'returnByValue':True}}))
                while True:
                    reply=json.loads(await ws.recv())
                    if reply.get('id')==100:
                        assert 'exceptionDetails' not in reply.get('result',{}),reply
                        break
            for count in range(1,12):
                expression='postIpc("CreateTab",{url:'+json.dumps(url+f'?tab={count}')+'})'
                await ws.send(json.dumps({'id':count,'method':'Runtime.evaluate','params':{'expression':expression,'returnByValue':True}}))
                while True:
                    reply=json.loads(await ws.recv())
                    if reply.get('id')==count:
                        assert 'exceptionDetails' not in reply.get('result',{}),reply
                        break
                await wait_for(lambda pages:sum(p.get('title')=='Pip tab benchmark' for p in pages)==count+1)
        await asyncio.sleep(3)
        samples=[]
        async def sample():
            processes=[parent]+parent.children(recursive=True)
            memory=[p.memory_info() for p in processes if p.is_running()]
            return {'processes':len(memory),'host_private_mib':parent.memory_info().private/1048576,
                    'total_private_mib':sum(m.private for m in memory)/1048576,
                    'summed_working_set_mib':sum(m.rss for m in memory)/1048576}
        samples.append(await sample())
        sleep_check=None
        if sleep_test:
            async def sleeping_tabs(pages):
                chrome_page=next((p for p in pages if p.get('title')=='Pip'),None)
                if not chrome_page:return None
                expression='document.querySelectorAll(".tab[aria-label*=\\\'tab đang ngủ\\\']").length'
                async with websockets.connect(chrome_page['webSocketDebuggerUrl'],max_size=8*1024*1024) as ws:
                    await ws.send(json.dumps({'id':200,'method':'Runtime.evaluate','params':{'expression':expression,'returnByValue':True}}))
                    while True:
                        reply=json.loads(await ws.recv())
                        if reply.get('id')==200:
                            return reply.get('result',{}).get('result',{}).get('value',0)
            deadline=asyncio.get_running_loop().time()+50
            asleep=0
            while asyncio.get_running_loop().time()<deadline and asleep<11:
                await asyncio.sleep(.5)
                asleep=await sleeping_tabs(await targets()) or 0
            samples.append(await sample())
            sleep_check={'suspended_background_tabs':asleep,'expected_background_tabs':11,
                         'wait_seconds':round(50-(deadline-asyncio.get_running_loop().time()),1)}
        else:
            for _ in range(2):
                await asyncio.sleep(.3)
                samples.append(await sample())
        result={'binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
                'tabs':12,'median':{k:round(statistics.median(s[k] for s in samples),2) for k in samples[0]}}
        if sleep_check:
            result['sleep_test']=sleep_check
            result['before_sleep']=samples[0]
            result['after_sleep']=samples[1]
        print(json.dumps(result,ensure_ascii=False),flush=True)
        return result
    finally:
        descendants=parent.children(recursive=True) if proc.poll() is None else []
        if proc.poll() is None:proc.terminate()
        proc.wait(timeout=10)
        for child in descendants:
            try:child.terminate()
            except psutil.NoSuchProcess:pass

async def main():
    global sleep_test
    sleep_test='--sleep-test' in sys.argv
    binaries=[arg for arg in sys.argv[1:] if arg!='--sleep-test']
    assert len(binaries)==2,'Supply before and after executables'
    server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    try:
        url=f'http://127.0.0.1:{server.server_port}/'
        results=[]
        for cycle in range(1 if sleep_test else 3):
            for binary in (binaries if cycle%2==0 else binaries[::-1]):
                results.append(await run(binary,url))
        inputs={}
        for binary in binaries:
            resolved=str(Path(binary).resolve())
            digest=hashlib.sha256(Path(resolved).read_bytes()).hexdigest()
            inputs.setdefault(digest,resolved)
        comparison=[]
        for digest,binary in inputs.items():
            samples=[r['median'] for r in results if r['sha256']==digest]
            summary={'binary':binary,'sha256':digest,'runs':len(samples),
                'median':{key:round(statistics.median(sample[key] for sample in samples),2) for key in samples[0]},
                'private_mib_range':[min(sample['total_private_mib'] for sample in samples),max(sample['total_private_mib'] for sample in samples)]}
            comparison.append(summary)
        report={'scenario':'12 identical local static tabs; hidden windows; CDP on loopback; fresh portable profiles'+('; 10-second idle threshold; compare memory before/after suspension' if sleep_test else ''),
                'comparison_valid':len(inputs)>1,
                'input_sha256':list(inputs),
                'limitations':('One fresh-profile run per binary; static fixture does not model real sites or media. Working sets double-count shared pages.' if sleep_test else 'Three fresh-profile runs per binary, alternating order; three samples each. Working sets double-count shared pages. Does not prove real-site, media, 4GB-device or long-session performance.'),
                'results':results,'comparison':comparison if len(inputs)>1 else None,
                'single_binary_baseline':comparison[0] if len(inputs)==1 else None}
        output=Path('target/tab-sleep-benchmark.json' if sleep_test else 'target/tab-benchmark.json')
        output.write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    finally:server.shutdown()

if __name__=='__main__':asyncio.run(main())
