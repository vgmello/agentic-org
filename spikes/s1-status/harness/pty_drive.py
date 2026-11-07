#!/usr/bin/env python3
"""Drive an interactive `claude` session in a PTY from a JSON step script (spike S1).

Usage: pty_drive.py <steps.json> <transcript.log> -- <command...>
Steps: {"expect": regex, "timeout": s, "optional": bool} | {"send": text} | {"type": text}
       (type = text, short pause, then Enter) | {"sleep": s} | {"mark": label} | {"kill9": true}
Every step is logged with a wall-clock ms timestamp; the screen text (ANSI stripped) goes to
the transcript. Prints the child pid first so the caller can verify it.
"""
import json, os, pty, re, select, signal, struct, sys, termios, fcntl, time

ANSI = re.compile(r'\x1b\[[0-9;?<>=]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[()][A-Z0-9]|\x1b[=>78]')


def now():
    return time.time() * 1000


def main():
    steps_path, log_path = sys.argv[1], sys.argv[2]
    cmd = sys.argv[sys.argv.index('--') + 1:]
    steps = json.load(open(steps_path))
    log = open(log_path, 'a')

    def note(msg):
        line = f'{now():.0f} DRIVER {msg}'
        print(line, flush=True)
        log.write(line + '\n')
        log.flush()

    pid, fd = pty.fork()
    if pid == 0:
        os.execvp(cmd[0], cmd)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', 45, 140, 0, 0))
    note(f'child pid={pid} cmd={" ".join(cmd)}')
    buf = ''
    alive = True

    def pump(timeout):
        nonlocal buf, alive
        r, _, _ = select.select([fd], [], [], timeout)
        if not r:
            return
        try:
            data = os.read(fd, 65536)
        except OSError:
            alive = False
            return
        if not data:
            alive = False
            return
        text = ANSI.sub('', data.decode('utf-8', 'replace'))
        # Answer a cursor-position query so the TUI never waits on us.
        if b'\x1b[6n' in data:
            os.write(fd, b'\x1b[1;1R')
        buf += re.sub(r'\s+', '', text)  # the TUI positions words with cursor moves, so match without spaces
        log.write(text)
        log.flush()

    def run(steps):
      for st in steps:
          if not alive:
              note(f'child gone before step {st}')
              return False
          if 'mark' in st:
              note(f'MARK {st["mark"]}')
          elif 'sleep' in st:
              end = time.time() + st['sleep']
              while time.time() < end and alive:
                  pump(0.1)
          elif 'send' in st:
              os.write(fd, st['send'].encode())
              note(f'SEND {st["send"]!r}')
          elif 'type' in st:
              os.write(fd, st['type'].encode())
              end = time.time() + 0.6
              while time.time() < end:
                  pump(0.05)
              os.write(fd, b'\r')
              note(f'TYPE {st["type"]!r} + Enter')
          elif 'expect' in st:
              pat = re.compile(st['expect'].replace(' ', ''), re.I)
              start = len(buf) if st.get('fresh') else max(0, len(buf) - 4000)
              end = time.time() + st.get('timeout', 30)
              m = None
              while time.time() < end and alive:
                  m = pat.search(buf, start)
                  if m:
                      break
                  pump(0.1)
              m = m or pat.search(buf, start)
              if m:
                  note(f'EXPECT ok {st["expect"]!r} -> {m.group(0)!r}')
                  if 'then' in st:
                      if not run(st['then']):
                          return False
              else:
                  note(f'EXPECT {"MISS (optional)" if st.get("optional") else "TIMEOUT"} {st["expect"]!r}')
                  if not st.get('optional'):
                      return False
          elif 'kill9' in st:
              note(f'KILL -9 {pid} (our own child)')
              os.kill(pid, signal.SIGKILL)
      return True

    run(steps)
    # Drain until exit or 15 s.
    end = time.time() + 15
    while alive and time.time() < end:
        pump(0.2)
    try:
        wpid, status = os.waitpid(pid, os.WNOHANG)
        note(f'child exit wait -> {wpid} status={status}')
    except ChildProcessError:
        note('child already reaped')
    if alive:
        note('child still alive after script; leaving it for the caller to stop')


if __name__ == '__main__':
    main()
