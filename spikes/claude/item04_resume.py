#!/usr/bin/env python3
"""Item 4: resume with the full spec replayed; the system-prompt snapshot; resume after the
original cwd was deleted.

Spec v1: --setting-sources "" --settings <file allowing exactly Bash(touch ok-r1.txt) … Bash(touch ok-r4b.txt)> --strict-mcp-config
         --mcp-config {} --agents {zephyr: prompt v1} --agent zephyr
         --append-system-prompt-file <v1: codeword ALPHA> --permission-mode manual
         --permission-prompt-tool stdio --session-id <uuid>
R0: new session (spec v1): "remember PELICAN".
R1: --resume, spec v2 (agent prompt says ZEPHYR-V2, codeword BRAVO), snapshot default (on).
R2: --resume with NO spec replayed (no --settings/--agents/--agent/--append): settings and agent kept?
R3: --resume, spec v2, --system-prompt-snapshot off.
R4a: original cwd deleted; --resume from another cwd.
R4b: original cwd re-created empty at the same path; --resume there.
Each resume asks for the codeword, the remembered word, and to `touch ok-<run>.txt` (approval asked?).
Fixtures: resume-<run>.jsonl. Model turns: 6.
"""
import json
import shutil

import lib

b = lib.make_tmp("s4")
cfg, ws, rt = b / "cfg", b / "ws", b / "runtime"
rt.mkdir()
sid = lib.new_session_id()
settings = rt / "settings.json"
# exact rules, no wildcard (the recorded run used "Bash(touch ok-*)")
settings.write_text(json.dumps({"permissions": {"allow": [f"Bash(touch ok-{r}.txt)" for r in ("r1", "r2", "r3", "r4a", "r4b")]}}))


def spec(version, snapshot=None):
    agents = {"zephyr": {"description": "Office-run agent Zephyr",
                         "prompt": f"You are Zephyr. Begin every reply with 'ZEPHYR-V{version}:'."}}
    instr = rt / f"instructions-v{version}.md"
    instr.write_text(f"Office instructions version {version}. The codeword is {'ALPHA' if version == 1 else 'BRAVO'}.\n")
    a = ["--setting-sources", "", "--settings", str(settings), "--permission-mode", "manual",
         "--permission-prompt-tool", "stdio", "--agents", json.dumps(agents), "--agent", "zephyr",
         "--append-system-prompt-file", str(instr)]
    if snapshot:
        a += ["--system-prompt-snapshot", snapshot]
    return a


ASK = ("Answer in one line: the codeword from your instructions, the word I asked you to remember earlier "
       "(or NONE), and your reply prefix. Then use the Bash tool to run exactly: touch ok-{run}.txt")
out = {"session_id_given": "<given>", "runs": {}}


def run(name, args, cwd, prompt, expect=None):
    """expect: the one Bash command this run may approve (None: approve nothing)."""
    s = lib.Session(lib.stream_args(*args), lib.clean_env(cfg, b / "tmp"), cwd, f"item04-{name}")
    asked = []
    guard = lib.approver(s, [("Bash", expect)] if expect else [])

    def on(ev):
        if ev.get("type") == "control_request" and ev["request"].get("subtype") == "can_use_tool":
            asked.append(ev["request"]["input"].get("command"))
        guard(ev)

    v = {"cwd": str(cwd)}
    try:
        s.user(prompt)
        r = s.until_result(150, on)
        si = s.events("system", "init")[-1]
        v.update({"reply": r.get("result"), "subtype": r.get("subtype"), "approval_asked_for": asked,
                  "session_id_same": si.get("session_id") == sid, "init_tools_count": len(si.get("tools", [])),
                  "init_cwd": si.get("cwd")})
    except Exception as e:
        v["error"] = repr(e)
    v["exit"] = s.close(30)
    v["denied_unexpected_calls"] = s.denials
    v["stderr"] = s.stderr[-5:]
    s.save(f"resume-{name}", [str(b)])
    out["runs"][name] = v
    print(name, json.dumps(lib.Scrubber([str(b)]).scrub(v))[:700])


run("R0-new", spec(1) + ["--session-id", sid], ws, "Remember the word PELICAN. Reply with your prefix and OK.")
run("R1-spec-v2-snapshot-on", spec(2) + ["--resume", sid], ws, ASK.format(run="r1"), "touch ok-r1.txt")
run("R2-no-spec", ["--permission-mode", "manual", "--permission-prompt-tool", "stdio", "--setting-sources", "",
                   "--resume", sid], ws, ASK.format(run="r2"), "touch ok-r2.txt")
run("R3-spec-v2-snapshot-off", spec(2, "off") + ["--resume", sid], ws, ASK.format(run="r3"), "touch ok-r3.txt")
out["transcripts_before_delete"] = sorted(str(p.relative_to(cfg)) for p in (cfg / "projects").rglob("*.jsonl"))
shutil.rmtree(ws)
ws2 = b / "ws-other"
ws2.mkdir()
run("R4a-cwd-deleted-other-cwd", spec(2) + ["--resume", sid], ws2, ASK.format(run="r4a"), "touch ok-r4a.txt")
ws.mkdir()
run("R4b-cwd-recreated", spec(2) + ["--resume", sid], ws, ASK.format(run="r4b"), "touch ok-r4b.txt")
out["transcripts_after"] = sorted(str(p.relative_to(cfg)) for p in (cfg / "projects").rglob("*.jsonl"))
out["files"] = {"ws": sorted(p.name for p in ws.iterdir()), "ws-other": sorted(p.name for p in ws2.iterdir())}
lib.write_result("item04", out)
print(json.dumps(lib.Scrubber([str(b)]).scrub({k: out[k] for k in ("transcripts_before_delete", "transcripts_after", "files")}), indent=1))
