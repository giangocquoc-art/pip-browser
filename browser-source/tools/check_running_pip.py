"""Local verification only. Needs Python websockets and Pip launched with test CDP port 9227.
Never enable remote debugging in the distributed launcher.
"""
import asyncio
import base64
import json
from pathlib import Path
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import urlopen
import websockets

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        body = b"<title>Pip integration check</title><script src='/ads/cbr.js'></script><p>Pip smoke page</p>"
        mime = "text/html"
        if self.path == "/ads/cbr.js":
            body, mime = b"window.adLoaded=true", "application/javascript"
        self.send_response(200)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *_):
        pass

def pages():
    with urlopen("http://127.0.0.1:9227/json", timeout=3) as response:
        return json.load(response)

async def command(page, method, params):
    async with websockets.connect(page["webSocketDebuggerUrl"], max_size=8*1024*1024) as ws:
        await ws.send(json.dumps({"id": 1, "method": method, "params": params}))
        while True:
            message = json.loads(await ws.recv())
            if message.get("id") == 1:
                assert "error" not in message, message
                return message["result"]

async def evaluate(page, expression):
    result = await command(page, "Runtime.evaluate", {"expression": expression, "returnByValue": True})
    assert "exceptionDetails" not in result, result
    return result["result"].get("value")

async def main():
    home = next(p for p in pages() if "Tab mới" in p["title"])
    assert await evaluate(home, "typeof window.__pipSend") == "function"
    assert await evaluate(home, "document.body.innerText.includes('Google') && !document.body.innerText.includes('DuckDuckGo')")
    motion_check = "Array.from(document.querySelectorAll('*')).every(el => {const s=getComputedStyle(el);return s.animationName==='none' && s.transitionDuration==='0s' && s.backdropFilter==='none'})"
    assert await evaluate(home, motion_check), "Home motion still enabled"
    chrome = next(p for p in pages() if p["id"] != home["id"] and p.get("type") == "page")
    assert await evaluate(chrome, "document.getElementById('omnibox').placeholder.includes('Google')")
    assert await evaluate(chrome, motion_check), "Chrome motion still enabled"
    screenshot = await command(home, "Page.captureScreenshot", {})
    Path("pip-newtab-verified.png").write_bytes(base64.b64decode(screenshot["data"]))
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_port}/"
    await evaluate(home, "window.__pipSend(" + json.dumps(json.dumps({"action":"Navigate", "payload":{"url":url}})) + ")")
    remote = None
    for _ in range(40):
        remote = next((p for p in pages() if p["url"] == url), None)
        if remote and await evaluate(remote, "document.readyState") == "complete":
            break
        await asyncio.sleep(.25)
    assert remote, "Home IPC did not navigate"
    assert not await evaluate(remote, "!!window.adLoaded"), "Ad script executed"
    assert await evaluate(remote, "typeof window.__pipSend") == "undefined", "Capability leaked to website"
    count = len(pages())
    await evaluate(remote, 'window.ipc.postMessage(JSON.stringify({action:"OpenSettings"}))')
    await asyncio.sleep(.6)
    assert len(pages()) == count, "Remote page could issue privileged IPC"
    await evaluate(chrome, 'window.__pipSend(JSON.stringify({action:"OpenSettings"}))')
    settings = None
    for _ in range(40):
        settings = next((p for p in pages() if p["title"] == "Cài đặt — Pip"), None)
        if settings:
            break
        await asyncio.sleep(.25)
    assert settings, "Settings did not open"
    assert await evaluate(settings, "document.getElementById('searchEngineSelect').value") == "google"
    assert await evaluate(settings, motion_check), "Settings motion still enabled"
    print("PASS: packaged Pip newtab rendered; internal IPC navigated; ad blocked on initial load; remote privileged IPC rejected.")
    print("PASS: Google in home/omnibox/settings; animations/transitions/backdrop blur disabled in rendered UI.")
    server.shutdown()

asyncio.run(main())
