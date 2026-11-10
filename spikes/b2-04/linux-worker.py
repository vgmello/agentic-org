import ctypes
import fcntl
import hashlib
import json
import os
from pathlib import Path
import select
import signal
import tempfile
import time

if not __debug__:
    raise SystemExit("probe assertions must be enabled")

libc = ctypes.CDLL(None, use_errno=True)
binary = Path(__file__).parent / "linux-bin/org-office-server"
image = binary.read_bytes()
expected = hashlib.sha256(image).digest()  # Trusted fixture, not a signed release manifest.

def launch(args, home, first, wanted):
    executable = os.memfd_create("sage-b204-worker", os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
    with os.fdopen(os.dup(executable), "wb") as output:
        output.write(image)
    os.fchmod(executable, 0o500)
    fcntl.fcntl(executable, fcntl.F_ADD_SEALS, fcntl.F_SEAL_WRITE | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SEAL)
    inp, writer = os.pipe2(os.O_CLOEXEC)
    reader, out = os.pipe2(os.O_CLOEXEC)
    pid = os.fork()
    if pid == 0:
        try:
            os.dup2(inp, 0); os.dup2(out, 1)
            null = os.open("/dev/null", os.O_WRONLY)
            os.dup2(null, 2)
            os.closerange(3, executable)
            os.closerange(executable + 1, 65536)
            os.chdir("/")
            assert libc.ptrace(0, 0, None, None) == 0
            os.execve(executable, [str(binary), *args], {"ORG_OFFICE_HOME": str(home)})
        finally:
            os._exit(127)
    os.close(inp); os.close(out)
    seen = []
    try:
        _, status = os.waitpid(pid, 0)
        assert os.WIFSTOPPED(status) and os.WSTOPSIG(status) == signal.SIGTRAP, status
        assert hashlib.sha256(Path(f"/proc/{pid}/exe").read_bytes()).digest() == expected
        assert libc.ptrace(0x4200, pid, None, ctypes.c_void_p(0x00100000)) == 0
        assert libc.ptrace(7, pid, None, None) == 0
        os.write(writer, json.dumps(first).encode() + b"\n")
        deadline = time.monotonic() + 25
        buf = b""
        while len(seen) < len(wanted):
            timeout = deadline - time.monotonic()
            assert timeout > 0 and select.select([reader], [], [], timeout)[0], "no worker event"
            chunk = os.read(reader, 8192)
            assert chunk, "worker closed control output"
            buf += chunk
            while b"\n" in buf:
                line, buf = buf.split(b"\n", 1)
                event = json.loads(line)
                seen.append(event)
                print("worker event:", event)
        assert [e["event"] for e in seen] == wanted, seen
        os.close(writer); writer = -1
        deadline = time.monotonic() + 10
        while True:
            done, status = os.waitpid(pid, os.WNOHANG)
            if done:
                assert os.waitstatus_to_exitcode(status) == 0, status
                pid = -1
                break
            assert time.monotonic() < deadline, "worker did not exit on host EOF"
            time.sleep(0.02)
        print("PASS worker admitted, control channel EOF stopped and reaped it")
    finally:
        if pid > 0:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        if writer >= 0: os.close(writer)
        os.close(reader); os.close(executable)

with tempfile.TemporaryDirectory(prefix="sb204-") as tmp:
    home = Path(tmp) / "h"
    for folder in [home / "offices", home / "run"]:
        folder.mkdir(parents=True, mode=0o700)
    office = "01M6WGY1WJCC503RXF2NS0SH84"
    key = os.urandom(32).hex()
    token = os.urandom(32).hex()
    launch(["--create"], home, {"v": 2, "op": "create", "org_id": "01M6WGY1WJCC503RXF2NS0SH83", "org_name": "B204 fixture", "office_id": office, "office_name": "HQ", "role": "hq", "app_token": token, "db_key": key}, ["created", "ready"])
    assert (home / "offices" / office / "office.json").exists()
    launch(["--office", office], home, {"v": 2, "op": "start", "app_token": token, "db_key": key}, ["ready"])
    print("PASS real encrypted office created and reopened on Linux without host macOS spawn path")
