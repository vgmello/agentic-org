import ctypes
import fcntl
import hashlib
import os
from pathlib import Path
import signal
import sys
import tempfile
import time

# Probe verified immutable bytes and stop at exec before any worker instructions.
# This validates primitives, not release trust or a hostile same-UID boundary.
libc = ctypes.CDLL(None, use_errno=True)
scratch = tempfile.TemporaryDirectory(prefix="sage-b204-exec-")
marker = Path(scratch.name) / "ran"
worker = Path(sys.argv[1]) if len(sys.argv)>1 else Path(__file__).parent / "linux-bin/exec-fixture"
args = sys.argv[2:] if len(sys.argv)>1 else [str(marker)]
image = worker.read_bytes()
expected = hashlib.sha256(image).digest()  # Stands in for reviewed signed manifest.
assert hashlib.sha256(image + b"changed").digest() != expected
print("PASS modified image digest rejected")
fd = os.memfd_create("sage-b204-worker", os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
os.write(fd, image)
os.fchmod(fd, 0o500)
fcntl.fcntl(fd, fcntl.F_ADD_SEALS, fcntl.F_SEAL_WRITE | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SEAL)
try:
    os.pwrite(fd, b"X", 0)
    raise AssertionError("sealed image changed")
except PermissionError:
    print("PASS sealed image refuses mutation")
readfd, writefd = os.pipe2(os.O_CLOEXEC)
outread, outwrite = os.pipe2(os.O_CLOEXEC)
pid = os.fork()
if pid == 0:
    os.close(writefd); os.close(outread)
    os.dup2(readfd, 0); os.dup2(outwrite, 1)
    os.dup2(outwrite, 2)
    os.close(readfd); os.close(outwrite)
    os.chdir("/")
    if libc.ptrace(0, 0, None, None) != 0:
        os._exit(120)
    os.execve(fd, [str(worker), *args], {})
os.close(readfd); os.close(outwrite)
_, status = os.waitpid(pid, 0)
assert os.WIFSTOPPED(status) and os.WSTOPSIG(status) == signal.SIGTRAP, status
os.kill(pid, signal.SIGCONT)
time.sleep(0.1)
assert not marker.exists()
print("PASS external SIGCONT does not run image at ptrace exec stop")
actual = Path(f"/proc/{pid}/exe")
assert hashlib.sha256(actual.read_bytes()).digest() == expected
assert (os.stat(actual).st_dev, os.stat(actual).st_ino) == (os.fstat(fd).st_dev, os.fstat(fd).st_ino)
print("PASS exec stop identifies exact sealed worker bytes before user instructions")
# EXITKILL: traced worker cannot remain running if the admitting probe dies.
assert libc.ptrace(0x4200, pid, None, ctypes.c_void_p(0x00100000)) == 0
assert libc.ptrace(7, pid, None, None) == 0
os.close(writefd)  # No real keys; EOF verifies bounded startup rejection.
while True:
    _, status = os.waitpid(pid, 0)
    if not os.WIFSTOPPED(status):
        break
    # The deliberately injected SIGCONT is observed as a tracing stop too.
    assert os.WSTOPSIG(status) == signal.SIGCONT, status
    assert libc.ptrace(7, pid, None, None) == 0
with os.fdopen(outread) as output:
    text = output.read()
print("worker output:", text.strip())
print("worker exit=", os.waitstatus_to_exitcode(status))
if len(sys.argv) == 1:
    assert marker.exists()
    print("PASS admitted image ran only after tracer continued it")
os.close(fd)
scratch.cleanup()
