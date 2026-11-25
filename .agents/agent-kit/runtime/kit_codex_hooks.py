#!/usr/bin/env python3
"""Codex event adapter; shared verification remains an explicit acceptance gate."""

import argparse
import json
from pathlib import Path
import re
import sys
import time

from kit_config import CheckError
from kit_hooks import cwd_from, default_base, format_file, guard, read_payload, verify
from kit_memory import checkpoint


def patch_paths(payload):
    value = payload.get("tool_input")
    if isinstance(value, dict) and "file_path" in value:
        if not isinstance(value["file_path"], str) or not value["file_path"]:
            raise CheckError("Edit/Write requires a file path")
        return [value["file_path"]]
    if isinstance(value, dict):
        value = value.get("patch", value.get("input"))
    if not isinstance(value, str) or not value.startswith("*** Begin Patch\n") or not value.rstrip().endswith("*** End Patch"):
        raise CheckError("Unrecognized patch input; no edit approval")
    paths = re.findall(r"^\*\*\* (?:Add File|Update File|Delete File|Move to): (.+)$", value, flags=re.M)
    if not paths:
        raise CheckError("Patch contains no recognized file paths")
    return paths


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("guard", "format", "verify", "checkpoint"))
    args = parser.parse_args()
    payload = {}
    try:
        payload = read_payload()
        cwd = cwd_from(payload)
        if args.action == "guard":
            for name in patch_paths(payload):
                guard({"cwd": str(cwd), "tool_input": {"file_path": name}})
        elif args.action == "format":
            deadline = time.monotonic() + 30
            for name in dict.fromkeys(patch_paths(payload)):
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise CheckError("Formatting budget exhausted; run the project formatter explicitly")
                entry = {"cwd": str(cwd), "tool_input": {"file_path": name}}
                guard(entry)
                format_file(entry, remaining)
        elif args.action == "verify":
            if payload.get("agent_type") == "fullstack-engineer":
                verify(cwd, None, default_base(cwd))
            # Stop events require JSON, including the successful case.
            print("{}")
        else:
            print(checkpoint({**payload, "cwd": str(cwd)}))
        return 0
    except (CheckError, OSError, ValueError, KeyError, TypeError) as exc:
        message = f"Starter kit Codex {args.action} failed: {exc}"
        if args.action == "verify" and payload.get("stop_hook_active") is True:
            print(json.dumps({"continue": False, "stopReason": message + "; still not accepted; return to lead"}))
            return 0
        print(message, file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
