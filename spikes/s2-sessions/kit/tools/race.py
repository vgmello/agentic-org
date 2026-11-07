# start (already done) -> stop <id8> -> immediately --bg --resume <uuid>, no flags. Captures everything.
import json, subprocess, sys, time, os
id8, uuid = sys.argv[1], sys.argv[2]
CC = "/tmp/ac-spike-s2/tools/cc"
def run(args):
    t = time.time(); p = subprocess.run([CC] + args, capture_output=True, text=True, stdin=subprocess.DEVNULL, cwd="/tmp/ac-spike-s2/ws")
    return {"argv": "claude " + " ".join(args), "exit": p.returncode, "elapsed_s": round(time.time() - t, 3), "at": round(t, 3), "stdout": p.stdout, "stderr": p.stderr}
def mine():
    d = json.loads(run(["agents", "--json", "--all"])["stdout"])
    return [s for s in d if s["cwd"].startswith("/private/tmp/ac-spike-s2")]
def job(i):
    try:
        s = json.load(open(os.path.expanduser(f"~/.claude/jobs/{i}/state.json")))
        return {k: s.get(k) for k in ("state", "tempo", "name", "nameSource", "respawnFlags", "sessionId", "resumeSessionId", "intent", "createdAt", "updatedAt")}
    except Exception as e:
        return str(e)
out = {"before": mine()}
known = {s["id"] for s in out["before"]}
out["stop"] = run(["stop", id8])
if len(sys.argv) > 3 and sys.argv[3] == "poll":
    t0 = time.time(); polls = 0
    while True:
        polls += 1; cur = [s for s in mine() if s["id"] == id8]
        if cur and not cur[0].get("pid"): break
        if time.time() - t0 > 10: break
    out["polled_until_no_pid"] = {"polls": polls, "waited_s": round(time.time() - t0, 3)}; out["before_resume"] = mine()
elif len(sys.argv) > 3: time.sleep(float(sys.argv[3])); out["slept_s"] = float(sys.argv[3]); out["before_resume"] = mine()
out["resume"] = run(["--bg", "--resume", uuid, "Say your name again, one sentence."])
out["after_immediate"] = mine()
time.sleep(4)
out["after_4s"] = mine()
new = [s["id"] for s in out["after_4s"] if s["id"] not in known]
out["new_ids"] = new
out["job_state"] = {i: job(i) for i in [id8] + new}
print(json.dumps(out, indent=1))
