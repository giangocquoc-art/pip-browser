"""Run the packaged Pip in an isolated portable profile; stdlib only, no debugger.

Usage: python tools/check_adblock_binary.py [dist-pip/Pip.exe]
All requests and the page's report stay on loopback. No existing Pip session is touched.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


PAGE = b"""<!doctype html><title>Pip Brave verification</title>
<script src='/ads/cbr.js'></script><div data-ad-name='smoke'>Ad</div>
<p id='normal'>Normal content</p><video src='/adserver.mp4' preload='auto'></video>
<video src='/normal.mp4' preload='auto'></video><script>
addEventListener('load', async () => {
  const manifest = await (await fetch('/stream.m3u8')).text();
  const hidden = () => getComputedStyle(document.querySelector('[data-ad-name]')).display === 'none';
  const end = performance.now() + 3000;
  while (!hidden() && performance.now() < end) await new Promise(r => setTimeout(r, 25));
  await fetch('/report', {method:'POST', body:JSON.stringify({
    ad_executed:!!window.adLoaded, cosmetic_hidden:hidden(), manifest,
    normal_visible:getComputedStyle(document.getElementById('normal')).display !== 'none',
    no_privileged_bridge:typeof window.__pipSend === 'undefined', runtime:navigator.userAgent
  })});
});</script>"""


class Handler(BaseHTTPRequestHandler):
    counts = {}
    result = None
    lock = threading.Lock()
    reported = threading.Event()

    def do_GET(self):
        with self.lock:
            self.counts[self.path] = self.counts.get(self.path, 0) + 1
        mime, body = {
            '/': ('text/html; charset=utf-8', PAGE),
            '/ads/cbr.js': ('application/javascript', b'window.adLoaded=true'),
            '/adserver.mp4': ('video/mp4', b'media request transport test'),
            '/normal.mp4': ('video/mp4', b'media request transport test'),
            '/stream.m3u8': ('application/vnd.apple.mpegurl', b'ok'),
        }.get(self.path, ('text/plain', b'ok'))
        self.send_response(200)
        self.send_header('Content-Type', mime)
        self.send_header('Content-Length', str(len(body)))
        self.send_header('Cache-Control', 'no-store')
        self.send_header('Content-Security-Policy', "style-src 'none'")
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        if self.path != '/report':
            self.send_error(404)
            return
        data = self.rfile.read(int(self.headers['Content-Length']))
        type(self).result = json.loads(data)
        self.send_response(204)
        self.end_headers()
        self.reported.set()

    def log_message(self, *_):
        pass


def main():
    binary = Path(sys.argv[1] if len(sys.argv) > 1 else 'dist-pip/Pip.exe').resolve(strict=True)
    directory = Path(tempfile.mkdtemp(prefix='pip-brave-binary-'))
    staged = directory / 'Pip.exe'
    shutil.copy2(binary, staged)
    (directory / 'portable.ini').touch()
    # EasyList deliberately exempts generic CSS on 127.0.0.1, so use another loopback address.
    server = ThreadingHTTPServer(('127.0.0.2', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    environment = os.environ.copy()
    for key in ('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', 'WEBVIEW2_USER_DATA_FOLDER'):
        environment.pop(key, None)
    process = subprocess.Popen([str(staged), f'http://127.0.0.2:{server.server_port}/', '--portable', '--background-check'],
                               cwd=directory, env=environment, creationflags=subprocess.CREATE_NO_WINDOW)
    try:
        assert Handler.reported.wait(35), f'Packaged browser did not report; exit={process.poll()}'
        result = Handler.result
        assert result['ad_executed'] is False, result
        assert result['cosmetic_hidden'] is True, result
        assert result['normal_visible'] is True and result['manifest'] == 'ok', result
        assert result['no_privileged_bridge'] is True, result
        with Handler.lock:
            counts = dict(Handler.counts)
        assert counts.get('/ads/cbr.js', 0) == 0, counts
        assert counts.get('/adserver.mp4', 0) == 0, counts
        assert counts.get('/normal.mp4', 0) > 0, counts
        result.update(binary=str(binary), sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                      requests=counts)
        output = Path('target/adblock-binary-verification.json')
        output.parent.mkdir(exist_ok=True)
        output.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
        print('PASS: packaged Pip blocked script/media before network; cosmetic CSS worked under CSP;')
        print('normal content/media/manifest passed; website received no privileged bridge.')
        print(f'Evidence: {output}')
    finally:
        # This process owns only our disposable test page, never the user's running profile.
        if process.poll() is None:
            process.terminate()
        process.wait(timeout=10)
        server.shutdown()
        # WebView2 descendants may briefly retain files; leave the profile to OS temp cleanup.


if __name__ == '__main__':
    main()
