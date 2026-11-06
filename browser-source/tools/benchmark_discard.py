"""Measure production Pip tab discard and restore in disposable instances.

This intentionally waits for the real 30-minute idle gate and natural Windows
low-memory condition. Run only in a disposable, memory-constrained Windows VM.
It does not allocate memory to force pressure, touch user profiles, or benchmark
remote/video sites.

Usage: python tools/benchmark_discard.py BEFORE.exe AFTER.exe [--pressure-timeout 900]
Requires psutil and websockets. Each executable is sampled once; hashes must differ.
"""
import argparse
import asyncio
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import urlopen

IDLE_GATE_SECONDS = 30 * 60
BACKGROUND_TABS = 11


class FixtureHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        body = b"<!doctype html><title>Pip discard fixture</title><p>Local static page</p>"
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        pass


async def evaluate(ws, command_id, expression):
    await ws.send(json.dumps({
        "id": command_id,
        "method": "Runtime.evaluate",
        "params": {"expression": expression, "returnByValue": True},
    }))
    while True:
        reply = json.loads(await ws.recv())
        if reply.get("id") == command_id:
            if "exceptionDetails" in reply.get("result", {}):
                raise RuntimeError(reply["result"]["exceptionDetails"])
            return reply.get("result", {}).get("result", {}).get("value")


async def run(binary, fixture_url, pressure_timeout):
    try:
        import psutil
        import websockets
    except ImportError as error:
        raise RuntimeError("Install the existing benchmark dependencies: pip install psutil websockets") from error
    binary = Path(binary).resolve(strict=True)
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    directory = Path(tempfile.mkdtemp(prefix="pip-discard-benchmark-"))
    staged = directory / "Pip.exe"
    shutil.copy2(binary, staged)
    (directory / "portable.ini").touch()
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]

    env = os.environ.copy()
    env.pop("WEBVIEW2_USER_DATA_FOLDER", None)
    env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = (
        f"--remote-debugging-port={port} --remote-debugging-address=127.0.0.1"
    )
    proc = subprocess.Popen(
        [str(staged), fixture_url, "--portable", "--background-check"],
        cwd=directory,
        env=env,
        creationflags=subprocess.CREATE_NO_WINDOW | subprocess.BELOW_NORMAL_PRIORITY_CLASS,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    parent = psutil.Process(proc.pid)

    async def targets():
        def read():
            with urlopen(f"http://127.0.0.1:{port}/json", timeout=2) as response:
                return json.load(response)
        return await asyncio.to_thread(read)

    async def shell_page():
        pages = await targets()
        return next((page for page in pages if page.get("title") == "Pip"), None)

    async def wait_for(predicate, timeout=60, reason="condition"):
        deadline = asyncio.get_running_loop().time() + timeout
        while asyncio.get_running_loop().time() < deadline:
            if proc.poll() is not None:
                raise RuntimeError(f"Pip benchmark exited with status {proc.returncode}")
            try:
                result = predicate(await targets())
                if result:
                    return result
            except (OSError, ValueError):
                pass
            await asyncio.sleep(0.5)
        return None

    async def shell_eval(command_id, expression):
        page = await shell_page()
        if not page:
            raise RuntimeError("Pip shell CDP target is unavailable")
        async with websockets.connect(page["webSocketDebuggerUrl"], max_size=8 * 1024 * 1024) as ws:
            return await evaluate(ws, command_id, expression)

    def memory_sample(label):
        processes = [parent] + parent.children(recursive=True)
        rows = []
        for process in processes:
            try:
                if process.is_running():
                    info = process.memory_full_info()
                    rows.append((info.uss, info.rss))
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                continue
        return {
            "phase": label,
            "process_count": len(rows),
            "total_private_mib": round(sum(row[0] for row in rows) / 1048576, 2),
            "summed_working_set_mib": round(sum(row[1] for row in rows) / 1048576, 2),
        }

    try:
        if not await wait_for(lambda pages: next((p for p in pages if p.get("title") == "Pip"), None),
                              reason="Pip shell"):
            raise TimeoutError("Pip shell CDP target did not appear")
        await shell_eval(1, "postIpc('SaveSettings',{settings_json:JSON.stringify({performance:{sleep_after_secs:10,background_throttling:true}})})")
        for ordinal in range(1, BACKGROUND_TABS + 1):
            url = fixture_url + f"?tab={ordinal}"
            await shell_eval(ordinal + 1, "postIpc('CreateTab',{url:" + json.dumps(url) + "})")
            expected = url
            found = await wait_for(lambda pages: next((p for p in pages if p.get("url") == expected), None),
                                   timeout=30, reason=f"fixture tab {ordinal}")
            if not found:
                raise TimeoutError(f"Tab {ordinal} did not load")

        # Keep the twelfth tab active and allow the eleven fixture tabs to suspend.
        await asyncio.sleep(3)
        deadline = asyncio.get_running_loop().time() + 60
        suspension = 0
        while asyncio.get_running_loop().time() < deadline:
            suspension = await shell_eval(20, "document.querySelectorAll('.tab.sleeping').length") or 0
            if suspension >= BACKGROUND_TABS:
                break
            await asyncio.sleep(1)
        if suspension < BACKGROUND_TABS:
            raise RuntimeError(f"Only {suspension}/{BACKGROUND_TABS} tabs suspended; discard test is invalid")

        # All background tabs have been idle for at least ten seconds. Wait the full
        # production discard threshold plus a small scheduling margin.
        await asyncio.sleep(IDLE_GATE_SECONDS + 5)
        before_pressure = memory_sample("before_natural_pressure")
        deadline = asyncio.get_running_loop().time() + pressure_timeout
        discarded = []
        while asyncio.get_running_loop().time() < deadline:
            raw = await shell_eval(21, "JSON.stringify(Array.from(document.querySelectorAll('.tab.discarded')).map(el=>({id:Number(el.id.slice(4)),label:el.getAttribute('aria-label')})))")
            discarded = json.loads(raw or "[]")
            if discarded:
                break
            await asyncio.sleep(1)
        after_discard = memory_sample("after_discard_or_pressure_timeout")

        restoration = None
        if discarded:
            candidate = discarded[0]
            target_url = await shell_eval(22, "window.__lastShellState?.tabs.find(t=>t.id===" + str(candidate["id"]) + ")?.url || ''")
            await shell_eval(23, "document.getElementById('tab-" + str(candidate["id"]) + "')?.click()")
            restored = await wait_for(lambda pages: next((p for p in pages if p.get("url") == target_url), None),
                                      timeout=60, reason="discarded WebView recreation")
            restoration = {
                "tab_id": candidate["id"],
                "url": target_url,
                "webview_target_returned": bool(restored),
            }
        after_restore = memory_sample("after_restore") if restoration and restoration["webview_target_returned"] else None

        delta = {
            "discard_private_mib": round(after_discard["total_private_mib"] - before_pressure["total_private_mib"], 2),
            "discard_process_count": after_discard["process_count"] - before_pressure["process_count"],
        }
        if after_restore:
            delta["restore_private_mib"] = round(after_restore["total_private_mib"] - after_discard["total_private_mib"], 2)
            delta["restore_process_count"] = after_restore["process_count"] - after_discard["process_count"]
        return {
            "binary": str(binary),
            "sha256": digest,
            "scenario": "11 local static background tabs; production suspend/discard policy; natural OS pressure",
            "required_idle_seconds": IDLE_GATE_SECONDS,
            "suspended_background_tabs": suspension,
            "discarded_tabs": discarded,
            "pressure_wait_seconds": round(pressure_timeout - max(0, deadline - asyncio.get_running_loop().time()), 1),
            "measurement_exercised": bool(discarded and restoration and restoration["webview_target_returned"]),
            "restoration": restoration,
            "memory": [before_pressure, after_discard] + ([after_restore] if after_restore else []),
            "memory_delta_mib": delta,
            "limitation": "A timeout means the binary did not encounter real Windows low-memory pressure; it does not establish discard savings.",
        }
    finally:
        # This process tree was created by this harness in a fresh portable directory.
        descendants = parent.children(recursive=True) if proc.poll() is None else []
        if proc.poll() is None:
            proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            raise RuntimeError("Disposable benchmark Pip did not exit after close request")
        for child in descendants:
            try:
                child.terminate()
            except psutil.NoSuchProcess:
                pass


async def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", help="Before-build Pip.exe")
    parser.add_argument("after", help="After-build Pip.exe")
    parser.add_argument("--pressure-timeout", type=int, default=900,
                        help="Seconds to wait for natural low-memory pressure after the 30-minute idle gate (default: 900)")
    args = parser.parse_args()
    if args.pressure_timeout < 1:
        parser.error("--pressure-timeout must be positive")
    binaries = [Path(args.before).resolve(strict=True), Path(args.after).resolve(strict=True)]
    hashes = [hashlib.sha256(path.read_bytes()).hexdigest() for path in binaries]
    if hashes[0] == hashes[1]:
        parser.error("before and after executables have the same SHA-256; comparison is invalid")

    server = ThreadingHTTPServer(("127.0.0.1", 0), FixtureHandler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    results = []
    try:
        fixture_url = f"http://127.0.0.1:{server.server_port}/"
        for binary in binaries:
            results.append(await run(binary, fixture_url, args.pressure_timeout))
        report = {
            "comparison_valid": len({result["sha256"] for result in results}) == 2,
            "measurement_exercised_for_both_builds": all(result["measurement_exercised"] for result in results),
            "results": results,
        }
        output = Path("target/tab-discard-benchmark.json")
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
        print(json.dumps(report, ensure_ascii=False, indent=2))
    finally:
        server.shutdown()


if __name__ == "__main__":
    asyncio.run(main())
