#!/usr/bin/env python3
"""Claude Code hooks and standalone verification. Python 3.10+, macOS/Linux/WSL."""

import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time

from kit_config import CheckError, ancestors_in_repo, covered, digest, load_checks, protected_name
from kit_evidence import git, inspect_receipt, same_sources, save_receipt, sources, timestamp


class VerificationError(CheckError):
    def __init__(self, message, receipt, receipt_path):
        super().__init__(message)
        self.receipt = receipt
        self.receipt_path = receipt_path


def read_payload():
    try:
        payload = json.load(sys.stdin)
    except (ValueError, OSError) as exc:
        raise CheckError(f"Invalid hook input: {exc}") from exc
    if not isinstance(payload, dict):
        raise CheckError("Hook input must be a JSON object")
    return payload


def cwd_from(payload):
    value = payload.get("cwd", os.getcwd())
    if not isinstance(value, str) or not value:
        raise CheckError("Hook cwd must be a nonempty directory path")
    path = Path(value).resolve()
    if not path.is_dir():
        raise CheckError(f"Hook cwd does not exist: {path}")
    return path


def file_from(payload):
    data = payload.get("tool_input", {})
    if not isinstance(data, dict):
        raise CheckError("tool_input must be an object")
    value = data.get("file_path", "")
    if not isinstance(value, str):
        raise CheckError("file_path must be a string")
    return cwd_from(payload) / value if value else None


def guard(payload):
    path = file_from(payload)
    if path and (protected_name(path.name) or protected_name(path.resolve().name)):
        raise CheckError(f"Direct Edit/Write to {path} is blocked. "
                         "Use a non-secret .env.example, .env.sample or .env.template.")


def command(argv, cwd, timeout, record=None, log_dir=None):
    """Keep full verification output on disk; show only a bounded failure tail."""
    started = time.monotonic()
    record = record if record is not None else {}
    record.update({"argv": argv, "cwd": str(cwd), "exit_code": None, "timed_out": False})
    if log_dir is not None:
        if any(p.is_symlink() for p in (log_dir, *log_dir.parents)):
            raise CheckError("Command log directory must not traverse a symlink")
        log_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
        fd, name = tempfile.mkstemp(prefix="check-", suffix=".log", dir=log_dir)
        record["raw_log"] = name
        output_file = os.fdopen(fd, "w+b")
    else:
        output_file = tempfile.TemporaryFile()
    with output_file as output:
        try:
            process = subprocess.Popen(argv, cwd=cwd, stdout=output,
                                       stderr=subprocess.STDOUT, start_new_session=True)
        except OSError as exc:
            record.update({"elapsed_seconds": round(time.monotonic() - started, 3), "launch_failed": True})
            raise CheckError(f"Cannot run {shlex.join(argv)} in {cwd}: {exc}") from exc
        timed_out = False
        try:
            code = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
            code = process.returncode
        record.update({"exit_code": code, "timed_out": timed_out,
                       "elapsed_seconds": round(time.monotonic() - started, 3)})
        output.seek(0, os.SEEK_END)
        record["output_bytes"] = output.tell()
        record["displayed_tail_chars"] = 0
        if code or timed_out:
            output.seek(0, os.SEEK_END)
            output.seek(max(0, output.tell() - 12000))
            tail = "\n".join(output.read().decode("utf-8", errors="replace").splitlines()[-60:])
            record["displayed_tail_chars"] = len(tail)
            reason = "timed out" if timed_out else f"exited {code}"
            location = f"\nFull output: {record['raw_log']}" if "raw_log" in record else ""
            raise CheckError(f"{cwd}: {shlex.join(argv)} {reason}\n{tail}{location}")


def format_file(payload, timeout=30):
    path = file_from(payload)
    if not path or not path.is_file():
        return
    path = path.resolve()
    candidates = list(ancestors_in_repo(path.parent))
    if path.suffix in {".ts", ".tsx", ".js", ".jsx", ".json", ".css", ".md"}:
        for directory in candidates:
            prettier = directory / "node_modules/.bin/prettier"
            if prettier.is_file():
                command([str(prettier), "--write", "--", str(path)], directory, timeout)
                return
    elif path.suffix == ".py":
        ruff = next((str(p / ".venv/bin/ruff") for p in candidates
                     if (p / ".venv/bin/ruff").is_file()), shutil.which("ruff"))
        if ruff:
            command([ruff, "format", "--", str(path)], path.parent, timeout)
    elif path.suffix == ".go" and shutil.which("gofmt"):
        command([shutil.which("gofmt"), "-w", str(path)], path.parent, timeout)
    # No installed formatter: skip. Never download a package as a hook side effect.


def find_config(cwd):
    for directory in ancestors_in_repo(cwd):
        candidate = directory / ".agent-kit/verification.json"
        if candidate.is_file():
            return candidate
    primary = os.environ.get("CLAUDE_PROJECT_DIR")
    if primary:
        candidate = Path(primary).resolve() / ".agent-kit/verification.json"
        if candidate.is_file():
            return candidate
    raise CheckError("No .agent-kit/verification.json found. Configure project paths "
                     "and checks before accepting implementation work.")


ADVISORY_SECONDS = 120
# Time kept back for the closing source fingerprint, so a non-blocking check cannot use it.
EVIDENCE_RESERVE_SECONDS = 30


def default_base(cwd, config=None):
    """The configured integration base, else the upstream when it is another branch, else None
    (run every check). A branch tracking its own remote copy is not a base: after a push it
    would leave nothing in scope."""
    try:
        config = Path(config).resolve() if config else find_config(cwd)
        configured = json.loads(config.read_text()).get("base")
    except (CheckError, OSError, ValueError, AttributeError):
        configured = None
    if configured is not None and not isinstance(configured, str):
        return None  # load_checks refuses the configuration with the reason
    branch = git(cwd, "symbolic-ref", "--short", "--quiet", "HEAD", optional=True)
    branch = branch.decode().strip() if branch else None
    base = configured
    if not base:
        upstream = git(cwd, "rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}", optional=True)
        base = upstream.decode().strip() if upstream else None
    # On the base branch itself everything is already integrated: nothing would be in scope.
    if not base or (branch and base.split("/", 1)[-1] == branch):
        return None
    return base


def changed_files(cwd, base, deadline):
    """The checkout's top level, the merge base with `base`, and the files that differ from
    it, including uncommitted and untracked ones."""
    if base.startswith("-"):
        raise CheckError(f"Base {base!r} is not a ref")
    root = Path(os.fsdecode(git(cwd, "rev-parse", "--show-toplevel", deadline=deadline).rstrip(b"\n"))).resolve()
    try:
        merge_base = git(root, "merge-base", "HEAD", base, deadline=deadline).decode().strip()
    except ValueError as exc:
        raise CheckError(f"Cannot scope checks to base {base!r}: it is unknown here or shares no history "
                         "with HEAD. Fetch it, set base in verification.json, or pass --base REF / --all") from exc
    raw = git(root, "diff", "--name-only", "--no-renames", "-z", merge_base, "--", deadline=deadline)
    raw += git(root, "ls-files", "--others", "--exclude-standard", "-z", deadline=deadline)
    return root, merge_base, sorted({root / os.fsdecode(p) for p in raw.split(b"\0") if p})


def verify(cwd, config=None, base=None):
    """Run the configured checks. With a base, a check that declares paths runs only when a
    changed file matches them; without one, or when the configuration itself changed, every
    check runs. Non-blocking checks run last, time-capped; their failure is a warning."""
    config = Path(config).resolve() if config else find_config(cwd)
    receipt = {"version": 1, "started_at": timestamp(), "cwd": str(cwd), "config": str(config),
               "config_sha256": None, "scope": None, "executed": 0, "checks": [], "sources_before": [],
               "sources_after": [], "source_bound": False, "passed": False}
    completed = []
    failure = None
    try:
        receipt["config_sha256"] = digest(config.read_bytes())
        checks, timeout = load_checks(config, cwd)
        deadline = time.monotonic() + timeout
        root, changed = config.parent.parent.resolve(), None
        if base:
            top, merge_base, changed = changed_files(cwd, base, deadline)
            receipt["scope"] = {"base": base, "merge_base": merge_base, "changed": [str(p) for p in changed]}
            if config in changed:
                completed.append("NOTE the verification configuration changed: running every check")
                receipt["scope"]["configuration_changed"] = True
                changed = None
        else:
            receipt["scope"] = "all"
        receipt["sources_before"] = sources(checks, deadline)
        receipt["source_bound"] = all(source["bound"] for source in receipt["sources_before"])
        for check in sorted(checks, key=lambda c: not c.blocking):
            # A project outside the scoped checkout cannot be scoped by its changes: run it whole.
            scoped = (changed is not None and check.paths and root.is_relative_to(top)
                      and check.directory.is_relative_to(top))
            files = covered(check, changed, root) if scoped else None
            if files == []:
                receipt["checks"].append({"argv": check.argv, "cwd": str(check.directory), "blocking": check.blocking,
                                          "skipped": "no changed file matches its paths"})
                completed.append(f"SKIP {check.directory}: {shlex.join(check.argv)} (nothing it covers changed)")
                continue
            argv = check.argv
            remaining = deadline - time.monotonic()
            limit = remaining if check.blocking else min(ADVISORY_SECONDS, remaining - EVIDENCE_RESERVE_SECONDS)
            if limit <= 0:
                if check.blocking:
                    raise CheckError("Verification exceeded its total time budget")
                receipt["checks"].append({"argv": argv, "cwd": str(check.directory), "blocking": False,
                                          "skipped": "time budget"})
                completed.append(f"WARN {check.directory}: {shlex.join(argv)} skipped (non-blocking): no time left")
                continue
            record = {"blocking": check.blocking}
            receipt["checks"].append(record)
            receipt["executed"] += 1
            try:
                command(argv, check.directory, limit, record, config.parent / "logs/commands")
            except CheckError as exc:
                if check.blocking or record.get("launch_failed"):
                    raise
                record["advisory_failure"] = True
                completed.append(f"WARN {check.directory}: {shlex.join(argv)} failed (non-blocking): {str(exc).splitlines()[0]}")
                continue
            completed.append(f"PASS {check.directory}: {shlex.join(argv)}")
        if not receipt["executed"]:
            completed.append("NOTE no configured check covers this change")
        receipt["sources_after"] = sources(checks, deadline)
        if (receipt["config_sha256"] != digest(config.read_bytes()) or
                not same_sources(receipt["sources_before"], receipt["sources_after"])):
            raise CheckError("Source/configuration changed during checks; rerun on the final contents")
        receipt["passed"] = True
    except (CheckError, OSError, ValueError) as exc:
        failure = exc
        # Receipts link private raw logs; the console only receives a bounded tail.
        receipt["failure"] = "Verification failed; inspect command statuses and the console diagnostic"
    receipt["finished_at"] = timestamp()
    try:
        receipt_path = save_receipt(config, receipt)
    except (OSError, ValueError) as exc:
        raise CheckError(f"Could not save verification evidence: {exc}; original result: {failure or 'checks passed'}") from exc
    if failure:
        raise VerificationError(f"{failure}\nEvidence: {receipt_path}", receipt, receipt_path) from failure
    return completed, receipt, receipt_path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("guard", "format", "verify", "check", "evidence"))
    parser.add_argument("--config", help="verification JSON (standalone check only)")
    parser.add_argument("--receipt", help="receipt to inspect (evidence action only)")
    scope = parser.add_mutually_exclusive_group()
    scope.add_argument("--base", help="check only what changed since this ref (check only; default: the configured "
                       "base, else an upstream on another branch, else every check)")
    scope.add_argument("--all", action="store_true", help="run every configured check (check only)")
    parser.add_argument("--json", action="store_true", help="JSON output for check/evidence")
    args = parser.parse_args()
    payload = {}
    try:
        if args.action == "evidence":
            if not args.receipt or args.config:
                raise CheckError("evidence requires --receipt and does not accept --config")
            report = inspect_receipt(args.receipt)
            print(json.dumps(report, indent=2) if args.json else
                  "\n".join([("CURRENT " + report["receipt"] if report["current"] else "STALE/FAILED: " + "; ".join(report["reasons"])),
                             "Scope: " + ("all" if report["scope"] == "all" else f"changes since {report['scope']['base']}"
                                          if isinstance(report["scope"], dict) else "unknown"),
                             *("WARNING " + w for w in report["warnings"])]))
            return 0 if report["current"] else 1
        if args.receipt or (args.json and args.action != "check"):
            raise CheckError("--receipt is for evidence; --json is for check/evidence")
        if args.action == "check":
            cwd = Path.cwd().resolve()
            completed, receipt, receipt_path = verify(cwd, args.config, None if args.all else args.base or default_base(cwd, args.config))
            print(json.dumps({**receipt, "receipt_path": str(receipt_path)}, indent=2) if args.json else
                  "\n".join([*completed, f"Evidence: {receipt_path}",
                              "Source identity: Git-bound" if receipt["source_bound"] else "Source identity: unbound (no Git checkout)"]))
            return 0
        if args.config or args.base or args.all:
            raise CheckError("--config, --base and --all are only supported by the standalone check command")
        payload = read_payload()
        if args.action == "guard":
            guard(payload)
        elif args.action == "format":
            format_file(payload)
        elif payload.get("agent_type") == "fullstack-engineer":
            cwd = cwd_from(payload)
            verify(cwd, None, default_base(cwd))
        return 0
    except (CheckError, OSError, RuntimeError, ValueError, KeyError, TypeError) as exc:
        message = f"Starter kit {args.action} failed: {exc}"
        if args.action in {"check", "evidence"} and args.json:
            print(json.dumps({**getattr(exc, "receipt", {}), "passed": False, "error": message,
                              "receipt_path": str(exc.receipt_path) if hasattr(exc, "receipt_path") else None}, indent=2))
            return 1
        if args.action == "verify" and payload.get("stop_hook_active") is True:
            print(json.dumps({"continue": False,
                              "stopReason": message + "\nVerification is still failing; "
                              "implementation is not approved. Automatic retries stopped.",
                              "systemMessage": "Verification failed after a repair attempt."}))
            return 0
        print(message, file=sys.stderr)
        return 1 if args.action in {"check", "evidence"} else 2


if __name__ == "__main__":
    sys.exit(main())
