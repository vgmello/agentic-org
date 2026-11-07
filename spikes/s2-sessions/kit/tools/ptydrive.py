# Minimal PTY driver: run argv in a pty, follow a script of (wait_seconds|expect_regex, send_bytes) steps, dump raw output.
import os, pty, sys, time, re, json, select, signal, struct, fcntl, termios
def run(argv, steps, out_path, cols=120, rows=40, env=None, cwd=None, total=60):
    pid, fd = pty.fork()
    if pid == 0:
        if cwd: os.chdir(cwd)
        e = dict(os.environ, TERM="xterm-256color", COLORTERM="truecolor", LANG="en_US.UTF-8", **(env or {}))
        os.execvpe(argv[0], argv, e)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    buf = bytearray(); t0 = time.time(); log = []
    def pump(dur):
        end = time.time() + dur
        while time.time() < end:
            r,_,_ = select.select([fd],[],[],0.05)
            if r:
                try: b = os.read(fd, 65536)
                except OSError: return False
                if not b: return False
                if not buf: log.append(("first_byte", round(time.time()-t0,3)))
                buf.extend(b)
        return True
    alive = True
    for st in steps:
        kind = st[0]
        if kind == "wait": alive = pump(st[1]) and alive
        elif kind == "expect":
            end = time.time() + st[2]; ok=False
            while time.time() < end:
                if re.search(st[1].encode(), strip(bytes(buf))): ok=True; break
                if not pump(0.2): break
            log.append(("expect", st[1], ok, round(time.time()-t0,3)))
        elif kind == "send":
            try: os.write(fd, st[1]); log.append(("send", repr(st[1]), round(time.time()-t0,3)))
            except OSError as ex: log.append(("send_failed", repr(st[1]), str(ex)))
        elif kind == "resize":
            fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", st[2], st[1], 0, 0)); log.append(("resize", st[1], st[2], round(time.time()-t0,3)))
        elif kind == "shell":
            import subprocess; r=subprocess.run(st[1],shell=True,capture_output=True,text=True,stdin=subprocess.DEVNULL); log.append(("shell", st[1], r.returncode, r.stdout.strip()[:300], r.stderr.strip()[:300], round(time.time()-t0,3)))
        elif kind == "mark": log.append(("mark", st[1], len(buf)))
        elif kind == "signal": os.kill(pid, st[1]); log.append(("signal", st[1], round(time.time()-t0,3)))
    pump(0.5)
    try:
        wpid, status = os.waitpid(pid, os.WNOHANG)
    except ChildProcessError: wpid, status = pid, 0
    if wpid == 0:
        log.append(("still_running_killing_hup",)); os.kill(pid, signal.SIGHUP); pump(1.0)
        try: wpid, status = os.waitpid(pid, 0)
        except ChildProcessError: status=0
    log.append(("exit", "code" if os.WIFEXITED(status) else "signal", os.WEXITSTATUS(status) if os.WIFEXITED(status) else os.WTERMSIG(status)))
    open(out_path, "wb").write(bytes(buf))
    return log
ANSI = re.compile(rb"\x1b\[[0-9;?<>=]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[()][0-9A-B]|\x1b[=>78DEHMNOc]")
CUF = re.compile(rb"\x1b\[(\d*)C")
def strip(b):
    b = CUF.sub(lambda m: b" " * int(m.group(1) or 1), b)
    return ANSI.sub(b"", b)
if __name__ == "__main__":
    spec = json.load(open(sys.argv[1]))
    steps = []
    for s in spec["steps"]:
        if s[0] == "send": steps.append(("send", s[1].encode().decode("unicode_escape").encode("latin-1") if isinstance(s[1], str) else bytes(s[1])))
        else: steps.append(tuple(s))
    log = run(spec["argv"], steps, spec["out"], spec.get("cols",120), spec.get("rows",40), spec.get("env"), spec.get("cwd"))
    for l in log: print(l)
    raw = open(spec["out"],"rb").read()
    open(spec["out"]+".txt","wb").write(strip(raw))
    print("bytes:", len(raw))
