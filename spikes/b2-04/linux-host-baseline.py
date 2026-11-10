import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time

binary = Path(__file__).parent / "linux-bin/org-office-host"
worker = binary.with_name("org-office-server")
with tempfile.TemporaryDirectory(prefix="shb204-") as tmp:
    root = Path(tmp)
    home = root / "h"
    with tempfile.TemporaryFile() as log:
        child = subprocess.Popen([str(binary), "--home", str(home), "--server-bin", str(worker)], env={"ORG_OFFICE_DEV_KEY_DIR": str(root / "keys")}, stdout=log, stderr=log)
        try:
            endpoint = home / "host.sock"
            deadline = time.monotonic() + 10
            while not endpoint.exists():
                assert child.poll() is None, "host exited at startup"
                assert time.monotonic() < deadline, "host socket missing"
                time.sleep(0.02)
            with socket.socket(socket.AF_UNIX) as stream:
                stream.settimeout(35)
                stream.connect(str(endpoint))
                with stream.makefile("rwb", buffering=0) as wire:
                    def call(id, method, params):
                        wire.write(json.dumps({"id": id, "method": method, "params": params}).encode() + b"\n")
                        return json.loads(wire.readline())
                    hello = call(1, "hello", {})
                    assert "result" in hello, hello
                    answer = call(2, "organization.create", {"name": "B204 disposable"})
                    assert answer["error"]["code"] == "storage_error", answer
                    print("PASS baseline Linux host refuses worker startup:", answer)
        finally:
            if child.poll() is None:
                child.send_signal(signal.SIGTERM)
            child.wait(timeout=10)
            log.seek(0)
            output = log.read().decode()
            assert "office servers run only on macOS" in output, output
            print("PASS baseline refusal reproduces macOS-only spawn path")
