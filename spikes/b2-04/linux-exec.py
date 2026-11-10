import ctypes
import fcntl
import hashlib
import os
from pathlib import Path
import signal
import tempfile

if not __debug__:
    raise SystemExit("probe assertions must be enabled")

libc = ctypes.CDLL(None, use_errno=True)
worker = Path(__file__).parent / "linux-bin/exec-fixture"
image = worker.read_bytes()
expected = hashlib.sha256(image).digest()  # Fixture stands in for signed manifest.


def launch(candidate, trusted_digest, marker):
    fd = os.memfd_create("sage-b204-worker", os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
    with os.fdopen(os.dup(fd), "wb") as output:
        output.write(candidate)
    os.fchmod(fd, 0o500)
    fcntl.fcntl(fd, fcntl.F_ADD_SEALS, fcntl.F_SEAL_WRITE | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SEAL)
    try:
        os.pwrite(fd, b"X", 0)
        raise AssertionError("sealed image changed")
    except PermissionError:
        pass
    readfd, writefd = os.pipe2(os.O_CLOEXEC)
    pid = os.fork()
    if pid == 0:
        try:
            os.dup2(readfd, 0)
            os.closerange(3, fd)
            os.closerange(fd + 1, 65536)
            os.chdir("/")
            if libc.ptrace(0, 0, None, None) != 0:
                os._exit(120)
            os.execve(fd, [str(worker), str(marker)], {})
        finally:
            os._exit(127)
    os.close(readfd)
    authority_sent = 0
    try:
        _, status = os.waitpid(pid, 0)
        assert os.WIFSTOPPED(status) and os.WSTOPSIG(status) == signal.SIGTRAP, status
        assert libc.ptrace(0x4200, pid, None, ctypes.c_void_p(0x00100000)) == 0  # EXITKILL
        actual = Path(f"/proc/{pid}/exe")
        admitted = hashlib.sha256(actual.read_bytes()).digest() == trusted_digest
        if not admitted:
            os.kill(pid, signal.SIGKILL)
            _, status = os.waitpid(pid, 0)
            pid = -1
            assert os.WIFSIGNALED(status) and os.WTERMSIG(status) == signal.SIGKILL
            assert not marker.exists()
            return False, authority_sent
        assert (os.stat(actual).st_dev, os.stat(actual).st_ino) == (os.fstat(fd).st_dev, os.fstat(fd).st_ino)
        os.kill(pid, signal.SIGCONT)
        state = Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()[0]
        assert state == "t" and not marker.exists(), state
        assert libc.ptrace(7, pid, None, None) == 0
        authority_sent = os.write(writefd, b"test")  # Public synthetic authority marker.
        os.close(writefd)
        writefd = -1
        stops = 0
        while True:
            _, status = os.waitpid(pid, 0)
            if not os.WIFSTOPPED(status):
                pid = -1
                break
            assert os.WSTOPSIG(status) == signal.SIGCONT, status
            stops += 1
            assert stops == 1
            assert libc.ptrace(7, pid, None, None) == 0
        assert stops == 1 and os.waitstatus_to_exitcode(status) == 0
        assert marker.exists()
        return True, authority_sent
    finally:
        if pid > 0:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        if writefd >= 0:
            os.close(writefd)
        os.close(fd)


with tempfile.TemporaryDirectory(prefix="sage-b204-exec-") as tmp:
    accepted, sent = launch(image + b"changed", expected, Path(tmp) / "rejected")
    assert not accepted and sent == 0
    print("PASS modified executable rejected at common admission check and killed before authority delivery")
    accepted, sent = launch(image, expected, Path(tmp) / "accepted")
    assert accepted and sent == 4
    print("PASS sealed image refuses mutation")
    print("PASS external SIGCONT leaves exec stopped; exactly one signal stop observed")
    print("PASS exact sealed executable admitted before synthetic authority delivery")
    print("PASS admitted image ran only after tracer continued it")
