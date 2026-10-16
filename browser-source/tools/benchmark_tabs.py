"""Compare 12 local tabs in hidden, disposable Pip instances. Dev-only CDP, never user profiles.
Usage: python tools/benchmark_tabs.py old.exe new.exe
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
        for _ in range(3):
            processes=[parent]+parent.children(recursive=True)
            memory=[p.memory_info() for p in processes if p.is_running()]
            samples.append({'processes':len(memory),'host_private_mib':parent.memory_info().private/1048576,
                            'total_private_mib':sum(m.private for m in memory)/1048576,
                            'summed_working_set_mib':sum(m.rss for m in memory)/1048576})
            await asyncio.sleep(.3)
        result={'binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
                'tabs':12,'median':{k:round(statistics.median(s[k] for s in samples),2) for k in samples[0]}}
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
    assert len(sys.argv)==3,'Supply before and after executables'
    server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    try:
        url=f'http://127.0.0.1:{server.server_port}/'
        binaries=sys.argv[1:]
        results=[]
        for cycle in range(3):
            for binary in (binaries if cycle%2==0 else binaries[::-1]):
                results.append(await run(binary,url))
        comparison=[]
        for binary in binaries:
            samples=[r['median'] for r in results if r['binary']==str(Path(binary).resolve())]
            comparison.append({'binary':str(Path(binary).resolve()),'runs':len(samples),
                'median':{key:round(statistics.median(sample[key] for sample in samples),2) for key in samples[0]},
                'private_mib_range':[min(sample['total_private_mib'] for sample in samples),max(sample['total_private_mib'] for sample in samples)]})
        report={'scenario':'12 identical local static tabs; hidden windows; CDP on loopback; fresh portable profiles',
                'limitations':'Three fresh-profile runs per binary, alternating order; three samples each. Working sets double-count shared pages. Does not prove real-site, media, 4GB-device or long-session performance.',
                'results':results,'comparison':comparison}
        Path('target/tab-benchmark.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    finally:server.shutdown()

if __name__=='__main__':asyncio.run(main())
