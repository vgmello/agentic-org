#!/usr/bin/env python3
"""Item 0: can an isolated config dir authenticate? No model call: `claude auth status` only.

  A: CLAUDE_CONFIG_DIR=<tmp>/cfg                                      (keychain name gets a hash suffix)
  B: CLAUDE_CONFIG_DIR=<tmp>/cfg CLAUDE_SECURESTORAGE_CONFIG_DIR=""   (default keychain item name)
Account fields are scrubbed before saving.
"""
import json
import subprocess

import lib

b = lib.make_tmp("s0")
out = {}
for name, extra in {"A-config-dir-only": {}, "B-config-dir-plus-empty-securestorage": None}.items():
    env = lib.clean_env(b / "cfg", b / "tmp")
    if extra is not None:
        env.pop("CLAUDE_SECURESTORAGE_CONFIG_DIR")
    r = subprocess.run([lib.CLAUDE, "auth", "status"], env=env, cwd=b / "ws", capture_output=True, text=True, timeout=60)
    try:
        status = json.loads(r.stdout)
    except ValueError:
        status = {"raw": r.stdout[:500]}
    keep = ("loggedIn", "authMethod", "apiProvider", "configDirectory", "subscriptionType")
    out[name] = {"exit": r.returncode, "status": {k: status.get(k) for k in keep if k in status},
                 "env_has_securestorage_var": "CLAUDE_SECURESTORAGE_CONFIG_DIR" in env}
out["files_in_config_dir"] = sorted(p.name for p in (b / "cfg").iterdir())
lib.write_result("item00", out)
print(json.dumps(lib.Scrubber([str(b)]).scrub(out), indent=1))
