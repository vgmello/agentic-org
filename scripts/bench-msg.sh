#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cargo build --release --offline --manifest-path "$ROOT_DIR/Cargo.toml" >/dev/null

ORG_AGENT="$ROOT_DIR/target/release/org-agent"
ORG_RELAY="$ROOT_DIR/target/release/org-relay"

python3 - "$ORG_AGENT" "$ORG_RELAY" << 'PY'
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time

org_agent, org_relay = sys.argv[1], sys.argv[2]

agent_size = os.path.getsize(org_agent)
relay_size = os.path.getsize(org_relay)

bench_home = tempfile.mkdtemp(prefix="ob_", dir="/tmp")
env_sender = {**os.environ, "ORG_HOME": bench_home, "ORG_AGENT": "agy-senior-dev"}
env_ada = {**os.environ, "ORG_HOME": bench_home, "ORG_AGENT": "ac-staff-engineer-ada"}

def pct(samples, p):
    s = sorted(samples)
    idx = min(len(s) - 1, max(0, int(len(s) * p)))
    return s[idx]

try:
    subprocess.run([org_agent, "msg", "register", "agy-senior-dev"], env=env_sender, check=True, stdout=subprocess.DEVNULL)
    subprocess.run([org_agent, "msg", "register", "ac-staff-engineer-ada"], env=env_ada, check=True, stdout=subprocess.DEVNULL)

    # 1. Benchmark `org-agent msg send` (1,000 runs, fire-and-forget enqueue)
    send_ms = []
    for i in range(1000):
        t0 = time.perf_counter()
        subprocess.run(
            [org_agent, "msg", "send", "--to", "ac-staff-engineer-ada", f"bench-{i}"],
            env=env_sender,
            check=True,
            stdout=subprocess.DEVNULL,
        )
        send_ms.append((time.perf_counter() - t0) * 1000.0)

    # Drain initial 1,000 before starting relay delivery benchmark
    subprocess.run([org_relay, "run", "--once"], env=env_sender, check=True, stdout=subprocess.DEVNULL)
    for f in os.listdir(os.path.join(bench_home, "spool/inbox/ac-staff-engineer-ada/new")):
        os.remove(os.path.join(bench_home, "spool/inbox/ac-staff-engineer-ada/new", f))

    # Start background relay
    relay_proc = subprocess.Popen([org_relay, "run"], env=env_sender, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.05)

    # 2. Benchmark send -> delivered in recipient's new/ (200 runs with relay running)
    deliver_ms = []
    ada_new = os.path.join(bench_home, "spool/inbox/ac-staff-engineer-ada/new")
    for i in range(200):
        t0 = time.perf_counter()
        res = subprocess.run(
            [org_agent, "msg", "send", "--to", "ac-staff-engineer-ada", f"deliv-{i}"],
            env=env_sender,
            check=True,
             capture_output=True,
            text=True,
        )
        msg_id = res.stdout.strip()
        target_file = os.path.join(ada_new, f"{msg_id}.json")
        while not os.path.exists(target_file):
            time.sleep(0.0002)
        deliver_ms.append((time.perf_counter() - t0) * 1000.0)
        os.remove(target_file)

    # 3. Benchmark `ask` round-trip with an immediate responder (100 runs)
    stop_responder = False
    def responder_loop():
        while not stop_responder:
            try:
                files = sorted(f for f in os.listdir(ada_new) if f.endswith(".json") and not f.startswith("."))
            except FileNotFoundError:
                break
            for fname in files:
                req_id = fname[:-5]
                subprocess.run(
                    [org_agent, "msg", "read", req_id],
                    env=env_ada,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
                subprocess.run(
                    [org_agent, "msg", "reply", req_id, "pong"],
                    env=env_ada,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
            time.sleep(0.001)

    t_resp = threading.Thread(target=responder_loop, daemon=True)
    t_resp.start()

    ask_ms = []
    for i in range(100):
        t0 = time.perf_counter()
        res = subprocess.run(
            [org_agent, "msg", "ask", "--to", "ac-staff-engineer-ada", "--timeout", "5s", f"ping-{i}"],
            env=env_sender,
            check=True,
            capture_output=True,
            text=True,
        )
        assert res.stdout.strip() == "pong"
        ask_ms.append((time.perf_counter() - t0) * 1000.0)

    stop_responder = True
    t_resp.join(timeout=1.0)
    relay_proc.terminate()
    relay_proc.wait(timeout=2.0)

    print(f"Binary size (release, stripped):")
    print(f"  org-agent: {agent_size:,} bytes ({agent_size / 1024:.1f} KiB)")
    print(f"  org-relay: {relay_size:,} bytes ({relay_size / 1024:.1f} KiB)")
    print(f"send (1,000 runs, fire-and-forget enqueue):")
    print(f"  p50: {pct(send_ms, 0.50):.2f} ms   p95: {pct(send_ms, 0.95):.2f} ms")
    print(f"send -> delivered in inbox/<to>/new (200 runs, relay running):")
    print(f"  p50: {pct(deliver_ms, 0.50):.2f} ms   p95: {pct(deliver_ms, 0.95):.2f} ms")
    print(f"ask <-> reply round trip (100 runs, immediate responder):")
    print(f"  p50: {pct(ask_ms, 0.50):.2f} ms   p95: {pct(ask_ms, 0.95):.2f} ms")
finally:
    shutil.rmtree(bench_home, ignore_errors=True)
PY
