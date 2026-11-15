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

from kit_config import CheckError, ancestors_in_repo, digest, load_checks, protected_name
from kit_evidence import inspect_receipt, same_sources, save_receipt, sources, timestamp


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


def verify(cwd, config=None):
    config = Path(config).resolve() if config else find_config(cwd)
    receipt = {"version": 1, "started_at": timestamp(), "cwd": str(cwd), "config": str(config),
               "config_sha256": None, "checks": [], "sources_before": [], "sources_after": [],
               "source_bound": False, "passed": False}
    completed = []
    failure = None
    try:
        receipt["config_sha256"] = digest(config.read_bytes())
        checks, timeout = load_checks(config, cwd)
        deadline = time.monotonic() + timeout
        receipt["sources_before"] = sources(checks, deadline)
        receipt["source_bound"] = all(source["bound"] for source in receipt["sources_before"])
        for directory, argv in checks:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise CheckError("Verification exceeded its total time budget")
            record = {}
            receipt["checks"].append(record)
            command(argv, directory, remaining, record, config.parent / "logs/commands")
            completed.append(f"PASS {directory}: {shlex.join(argv)}")
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
    parser.add_argument("--json", action="store_true", help="JSON output for check/evidence")
    args = parser.parse_args()
    payload = {}
    try:
        if args.action == "evidence":
            if not args.receipt or args.config:
                raise CheckError("evidence requires --receipt and does not accept --config")
            report = inspect_receipt(args.receipt)
            print(json.dumps(report, indent=2) if args.json else
                  ("CURRENT " + report["receipt"] if report["current"] else "STALE/FAILED: " + "; ".join(report["reasons"])))
            return 0 if report["current"] else 1
        if args.receipt or (args.json and args.action != "check"):
            raise CheckError("--receipt is for evidence; --json is for check/evidence")
        if args.action == "check":
            completed, receipt, receipt_path = verify(Path.cwd().resolve(), args.config)
            print(json.dumps({**receipt, "receipt_path": str(receipt_path)}, indent=2) if args.json else
                  "\n".join([*completed, f"Evidence: {receipt_path}",
                              "Source identity: Git-bound" if receipt["source_bound"] else "Source identity: unbound (no Git checkout)"]))
            return 0
        if args.config:
            raise CheckError("--config is only supported by the standalone check command")
        payload = read_payload()
        if args.action == "guard":
            guard(payload)
        elif args.action == "format":
            format_file(payload)
        elif payload.get("agent_type") == "fullstack-engineer":
            verify(cwd_from(payload))
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
