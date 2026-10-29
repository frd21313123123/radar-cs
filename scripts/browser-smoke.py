"""Browser regressions against a real, isolated demo server (no CS2 needed).

Install: python -m pip install playwright==1.62.0
         python -m playwright install chromium
Run:     python scripts/browser-smoke.py --exe target/debug/cas-external-radar
"""
import argparse
import re
import shutil
import socket
import subprocess
import tempfile
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Thread

from playwright.sync_api import sync_playwright


class StalledJSON(BaseHTTPRequestHandler):
    def do_GET(self):
        self.server.hits.append(1)
        try:
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", "2")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()
            self.wfile.flush()
            time.sleep(4)  # Headers arrive, but reading the JSON body stalls.
            self.wfile.write(b"{}")
        except (BrokenPipeError, ConnectionResetError):
            pass

    def log_message(self, *args):
        pass


def check_browser(url, executable):
    with sync_playwright() as playwright:
        options = {"headless": True}
        if executable:
            options["executable_path"] = executable
        browser = playwright.chromium.launch(**options)
        try:
            context = browser.new_context(viewport={"width": 1512, "height": 982})
            errors = []
            a, b = context.new_page(), context.new_page()
            for page in (a, b):
                page.on("pageerror", lambda error: errors.append(str(error)))
                page.set_default_timeout(10000)
            a.goto(url)
            a.wait_for_selector(".player")
            assert a.locator(".card-health").count() == 6
            assert a.locator(".card-inventory .equip-chip").count() > 0

            a.locator("#mapSelect").select_option("de_nuke")
            a.wait_for_function("() => map==='de_nuke'&&!autoMapEnabled&&!mapSelectionPending")
            b.goto(url)
            b.wait_for_function("() => map==='de_nuke'&&!autoMapEnabled")
            b.locator("#autoMapBtn").click()
            for page in (a, b):
                page.wait_for_function("() => map==='de_mirage'&&autoMapEnabled")
            print("PASS: manual selection and AUTO synchronize between clients")

            # An older poll that completes after a selection must not undo it.
            old = a.evaluate("JSON.parse(JSON.stringify(snapshot))")
            held = []

            def hold_one(route):
                if not held:
                    held.append(route)
                else:
                    route.continue_()

            a.route("**/api/state?*", hold_one)
            a.wait_for_timeout(150)
            assert held, "No state request was captured"
            a.locator("#mapSelect").select_option("de_nuke")
            a.wait_for_function("() => map==='de_nuke'&&!mapSelectionPending")
            a.evaluate("""() => {
                window.auditMapChanges=[];
                new MutationObserver(() => window.auditMapChanges.push($('mapCode').textContent))
                    .observe($('mapCode'),{childList:true,subtree:true});
            }""")
            held[0].fulfill(json=old)
            a.wait_for_timeout(200)
            assert not any("MIRAGE" in name for name in a.evaluate("window.auditMapChanges"))
            a.unroute("**/api/state?*", hold_one)
            print("PASS: an in-flight old poll cannot override a newer map selection")

            frozen = a.evaluate("JSON.parse(JSON.stringify(snapshot))")
            frozen.update(status="live", game_map="de_nuke", game_map_source="manual", source_hz=60)
            frozen["time"] += 1
            frozen["bomb_timer"] = {"site": "A", "remaining_secs": 30, "total_secs": 40, "being_defused": False}
            frozen["grenades"] = [
                {"id": 919, "kind": "molotov", "state": "active", "position": [-1800, 120, -1000],
                 "radius_units": 133, "trajectory": [], "fire_cells": [], "remaining_secs": 7, "timer_estimated": False},
                {"id": 920, "kind": "molotov", "state": "active", "position": [-1100, 100, 0],
                 "radius_units": 133, "trajectory": [], "fire_cells": [], "remaining_secs": 7, "timer_estimated": False},
            ]
            a.route("**/api/state?*", lambda route: route.fulfill(json=frozen))
            a.wait_for_function("() => snapshot.grenades.some(g=>g.id===919)")
            a.evaluate("""() => {
                window.auditZones={919:0,920:0};
                const original=ctx.arc.bind(ctx);
                ctx.arc=(x,y,r,...rest)=>{
                    for(const nade of snapshot.grenades){
                        if(!(nade.id in window.auditZones))continue;
                        const point=xy(nade.position);
                        const radius=nade.radius_units/mapData().radar_info.scale/1024*baseSide*scale;
                        if(Math.abs(x-point.x)<.01&&Math.abs(y-point.y)<.01&&Math.abs(r-radius)<.01)
                            window.auditZones[nade.id]++;
                    }
                    return original(x,y,r,...rest);
                };
            }""")
            a.wait_for_function("() => window.auditZones[920]>0")
            assert a.evaluate("window.auditZones[919]") == 0
            a.locator("#layerSelect").select_option("lower")
            a.evaluate("window.auditZones={919:0,920:0}")
            a.wait_for_function("() => window.auditZones[919]>0")
            assert a.evaluate("window.auditZones[920]") == 0
            assert not a.locator("#bombTimerPanel").evaluate("element=>element.hidden")
            print("PASS: utility zones and their timers use the selected Nuke floor")

            a.wait_for_function("() => snapshotStale()&&shown().length===0", timeout=5000)
            a.wait_for_function("() => $('statusText').textContent.startsWith('STALE')")
            assert a.locator(".player").count() == 0
            assert a.locator("#bombTimerPanel").evaluate("element=>element.hidden")
            a.evaluate("window.auditZones={919:0,920:0}")
            a.wait_for_timeout(150)
            assert sum(a.evaluate("window.auditZones").values()) == 0
            print("PASS: frozen snapshots stop reporting LIVE and hide old entities/timers")

            a.unroute("**/api/state?*")
            a.wait_for_selector(".player")
            stalled = ThreadingHTTPServer(("127.0.0.1", 0), StalledJSON)
            stalled.hits = []
            Thread(target=stalled.serve_forever, daemon=True).start()
            try:
                a.route("**/api/state?*", lambda route: route.continue_(url=f"http://127.0.0.1:{stalled.server_port}/body"))
                a.wait_for_function("() => $('statusText').textContent==='DISCONNECTED'", timeout=4000)
                a.wait_for_function("() => snapshotStale()", timeout=4000)
                a.wait_for_timeout(1500)
                assert len(stalled.hits) >= 2, "A timed-out JSON body prevented subsequent requests"
                assert a.locator(".player").count() == 0
            finally:
                a.unroute("**/api/state?*")
                stalled.shutdown()
                stalled.server_close()
            a.wait_for_selector(".player", timeout=5000)
            a.wait_for_function("() => !connectionError&&!snapshotStale()")
            print("PASS: hung requests time out, polling retries, and live data recovers")
            assert not errors, errors
            print("PASS: no browser JavaScript errors")
        finally:
            for context in browser.contexts:
                for page in context.pages:
                    page.unroute_all(behavior="ignoreErrors")
            browser.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", type=Path, default=Path("target/debug/cas-external-radar"))
    parser.add_argument("--browser", help="Optional installed Chromium executable")
    args = parser.parse_args()
    exe = args.exe.resolve(strict=True)
    root = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix="radar-browser-") as temporary:
        work = Path(temporary)
        (work / "web").mkdir()
        (work / "cache").mkdir()
        shutil.copyfile(root / "web/index.html", work / "web/index.html")
        for name in ("de_mirage_radar_psd.png", "de_nuke_radar_psd.png", "de_nuke_lower_radar_psd.png"):
            shutil.copyfile(root / "assets/bomb_ground.png", work / "cache" / name)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        with (work / "server.log").open("w") as log:
            process = subprocess.Popen([str(exe), "--demo", "--offline", "--no-browser", "--no-gsi-install", "--port", str(port)],
                                       cwd=work, stdout=log, stderr=subprocess.STDOUT)
            try:
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    assert process.poll() is None, "Demo server exited before startup"
                    match = re.search(r"token=([0-9a-f]+)", (work / "server.log").read_text())
                    if match:
                        break
                    time.sleep(.05)
                else:
                    raise AssertionError("Demo server did not start within 10 seconds")
                check_browser(f"http://127.0.0.1:{port}/?token={match[1]}", args.browser)
            finally:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


if __name__ == "__main__":
    main()
