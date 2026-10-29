"""Real-process checks for startup links, duplicate launches and HTTP caching."""
import argparse
import json
import re
import shutil
import socket
import subprocess
import tempfile
import time
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import urlopen


def request(port, path, token=None):
    query = ("&" if "?" in path else "?") + "token=" + token if token else ""
    try:
        response = urlopen(f"http://127.0.0.1:{port}{path}{query}", timeout=4)
    except HTTPError as error:
        response = error
    with response:
        return response.status, response.headers, response.read()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", type=Path, default=Path("target/debug/cas-external-radar"))
    args = parser.parse_args()
    exe = args.exe.resolve(strict=True)
    root = Path(__file__).resolve().parents[1]
    process = None
    with tempfile.TemporaryDirectory(prefix="radar-runtime-") as directory:
        work = Path(directory)
        (work / "web").mkdir()
        shutil.copyfile(root / "web/index.html", work / "web/index.html")
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        command = [str(exe), "--demo", "--offline", "--no-browser", "--no-gsi-install", "--port", str(port)]

        def start(name, arguments=None, demo=True):
            log_path = work / name
            with log_path.open("w", encoding="utf-8") as log:
                running = subprocess.Popen(arguments or command, cwd=work, stdout=log, stderr=subprocess.STDOUT)
            deadline = time.monotonic() + 12
            while time.monotonic() < deadline:
                if running.poll() is not None:
                    raise AssertionError("Radar exited before startup")
                match = re.search(r"token=([0-9a-f]+)", log_path.read_text(encoding="utf-8"))
                if match:
                    token = match[1]
                    try:
                        status, _, body = request(port, "/api/state", token)
                        value = json.loads(body)
                        ready = value["status"] == "demo" if demo else value["status"] != "starting"
                        if status == 200 and ready:
                            return running, token
                    except (OSError, ValueError):
                        pass
                time.sleep(.05)
            running.terminate()
            running.wait(timeout=10)
            raise AssertionError("Radar did not become ready")

        try:
            process, token = start("first.log")
            duplicate = subprocess.run(command, cwd=work, capture_output=True, encoding="utf-8", timeout=10)
            assert duplicate.returncode == 0, "A duplicate launch did not reuse the running radar"
            assert "token=" + token in duplicate.stdout, "Duplicate launch returned a different access link"
            assert process.poll() is None, "Duplicate launch stopped the original server"

            status, headers, _ = request(port, "/api/weapon-icon?weapon=AK-47&v=test", token)
            assert status == 200 and "private" in headers["Cache-Control"] and "max-age=" in headers["Cache-Control"]
            status, headers, _ = request(port, "/api/state", token)
            assert status == 200 and headers["Cache-Control"] == "no-store", "Live state was made cacheable"
            for bad_token in (None, "invalid"):
                status, _, _ = request(port, "/api/state", bad_token)
                assert status == 403, "Unauthenticated state request was accepted"
            status, _, body = request(port, "/")
            assert status == 403 and "Откройте ссылку" in body.decode("utf-8")
            assert token.encode() not in body, "Access-help page leaked the token"
            print("PASS: duplicate startup reuses the correct link; API stays authenticated; icons cache and live data does not")

            process.terminate()
            process.wait(timeout=10)
            process, restored = start("restart.log")
            assert restored == token, "Access link changed after restarting the executable"
            assert request(port, "/api/state", token)[0] == 200, "Saved browser link stopped working"
            process.terminate()
            process.wait(timeout=10)
            process = None

            # Exercise a genuinely clean offline install in normal mode, not
            # just --demo, which deliberately bypasses schema loading.
            assert not (work / "cache/schema-bundle.json").exists()
            process, _ = start("offline-schema.log", [arg for arg in command if arg != "--demo"], demo=False)
            bundle = json.loads((work / "cache/schema-bundle.json").read_text(encoding="utf-8"))
            assert len(bundle["revision"]) == 40 and bundle["info"]["build_number"] > 0
            _, _, body = request(port, "/api/state", token)
            assert json.loads(body)["diagnostics"].get("offsets_available") is not False
            process.terminate()
            process.wait(timeout=10)
            process = None
            print("PASS: clean offline normal startup loads one complete bundled schema")

            with socket.socket() as occupied:
                occupied.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                occupied.bind(("127.0.0.1", port))
                occupied.listen(4)
                conflict = subprocess.run(command, cwd=work, capture_output=True, encoding="utf-8", timeout=10)
                assert conflict.returncode != 0, "An unrelated occupied port was ignored"
                assert "token=" not in conflict.stdout, "A broken link was advertised before binding the port"
            print("PASS: access link survives a real process restart; an occupied port never advertises a wrong token")
        finally:
            if process is not None and process.poll() is None:
                process.terminate()
                process.wait(timeout=10)


if __name__ == "__main__":
    main()
