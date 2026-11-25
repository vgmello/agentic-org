"""Local verification receipts tied to configuration and Git working-tree contents.

Receipts contain command arguments and file metadata/hashes, never captured output
or file contents. They are local evidence, not signed CI attestations.
"""

from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import time
import uuid

from kit_config import digest


def timestamp():
    return datetime.now(timezone.utc).isoformat()


def git(directory, *args, optional=False, deadline=None):
    remaining = min(10, deadline - time.monotonic()) if deadline is not None else 10
    if remaining <= 0:
        raise ValueError("Verification evidence exceeded its time budget")
    try:
        result = subprocess.run(["git", "-c", "core.fsmonitor=false", "-C", str(directory), *args],
                                capture_output=True, timeout=remaining)
    except subprocess.TimeoutExpired as exc:
        raise ValueError(f"Git evidence timed out in {directory}") from exc
    except OSError:
        if optional:
            return None
        raise ValueError(f"Cannot collect Git evidence in {directory}")
    if result.returncode:
        if optional:
            return None
        raise ValueError(f"Git evidence command failed in {directory}: {' '.join(args)}")
    return result.stdout


def artifact_path(path):
    parts = Path(path).parts
    return any(parts[i] in {"agent-kit", ".agent-kit", ".claude"} and parts[i + 1] in
               {"logs", "reviews", ".starter-kit-backups"} for i in range(len(parts) - 1))


def snapshot(directory, deadline=None):
    deadline = deadline if deadline is not None else time.monotonic() + 30
    root_bytes = git(directory, "rev-parse", "--show-toplevel", optional=True, deadline=deadline)
    if root_bytes is None:
        return {"directory": str(directory), "bound": False,
                "reason": "No readable Git checkout; checks alone do not establish source identity"}
    root = Path(os.fsdecode(root_bytes.rstrip(b"\n"))).resolve()
    head = git(root, "rev-parse", "--verify", "HEAD", optional=True, deadline=deadline)
    branch = git(root, "symbolic-ref", "--short", "HEAD", optional=True, deadline=deadline)
    index = git(root, "ls-files", "--stage", "-z", deadline=deadline)
    paths = git(root, "ls-files", "--cached", "--others", "--exclude-standard", "-z", deadline=deadline)
    state = hashlib.sha256()
    state.update(head or b"unborn")
    for entry in index.split(b"\0"):
        if entry and not artifact_path(os.fsdecode(entry.split(b"\t", 1)[-1])):
            state.update(entry + b"\0")
    deadline = min(deadline, time.monotonic() + 20)
    count = 0
    for raw in sorted(set(paths.split(b"\0")) - {b""}):
        if time.monotonic() > deadline:
            raise ValueError(f"Source fingerprint exceeded 20s in {root}")
        relative = os.fsdecode(raw)
        if artifact_path(relative):
            continue
        path = root / relative
        if any(parent.is_symlink() for parent in path.parents if parent != root and parent.is_relative_to(root)):
            raise ValueError(f"Cannot bind source evidence through a linked directory: {path}")
        state.update(raw + b"\0")
        if not path.exists() and not path.is_symlink():
            state.update(b"deleted\0")
            continue
        info = path.lstat()
        state.update(str(stat.S_IMODE(info.st_mode)).encode() + b"\0")
        if path.is_symlink():
            state.update(b"link\0" + os.fsencode(os.readlink(path)) + b"\0")
        elif path.is_file():
            content = hashlib.sha256()
            with path.open("rb") as source:
                for chunk in iter(lambda: source.read(1024 * 1024), b""):
                    if time.monotonic() > deadline:
                        raise ValueError(f"Source fingerprint exceeded 20s in {root}")
                    content.update(chunk)
            state.update(content.digest())
        else:
            raise ValueError(f"Cannot fingerprint {path}; Git submodules/special files need separate acceptance evidence")
        count += 1
    status = os.fsdecode(git(root, "status", "--porcelain=v1", "-z", "--untracked-files=all", deadline=deadline))
    return {"directory": str(directory), "root": str(root), "bound": True,
            "head": head.decode().strip() if head else None,
            "branch": branch.decode().strip() if branch else None,
            "fingerprint": state.hexdigest(), "files_hashed": count,
            "status_porcelain_z": status}


def sources(checks, deadline=None):
    return [snapshot(directory, deadline) for directory in dict.fromkeys(check.directory for check in checks)]


def same_sources(before, after):
    return len(before) == len(after) and all(
        old.get("bound") == new.get("bound") and old.get("directory") == new.get("directory") and
        old.get("root") == new.get("root") and old.get("fingerprint") == new.get("fingerprint")
        for old, new in zip(before, after))


def save_receipt(config, receipt):
    directory = config.parent / "logs" / "verification"
    for candidate in (directory, *directory.parents):
        if candidate.is_symlink():
            raise ValueError(f"Receipt directory must not traverse a symlink: {directory}")
        if candidate == config.parent:
            break
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    destination = directory / f"{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S')}-{uuid.uuid4().hex}.json"
    fd, temporary = tempfile.mkstemp(prefix=".receipt-", dir=directory)
    try:
        with os.fdopen(fd, "w") as output:
            json.dump(receipt, output, indent=2)
            output.write("\n")
        os.replace(temporary, destination)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    return destination


def inspect_receipt(path):
    receipt = json.loads(Path(path).read_text())
    if not isinstance(receipt, dict) or receipt.get("version") != 1:
        raise ValueError("Unsupported verification receipt")
    reasons = []
    if not receipt.get("passed"):
        reasons.append("Verification did not pass")
    if not receipt.get("source_bound"):
        reasons.append("Evidence is not bound to Git source contents")
    config = Path(receipt["config"])
    if digest(config.read_bytes()) != receipt.get("config_sha256"):
        reasons.append("Verification configuration has changed")
    before = receipt.get("sources_after", [])
    current = [snapshot(Path(source["directory"])) for source in before]
    if not before or not same_sources(before, current):
        reasons.append("Checkout contents, index or HEAD have changed")
    warnings = [f"Non-blocking check {'failed' if c.get('advisory_failure') else 'skipped (no time left)'}: "
                f"{' '.join(c.get('argv', []))}"
                for c in receipt.get("checks", []) if c.get("advisory_failure") or c.get("skipped") == "time budget"]
    return {"receipt": str(Path(path).resolve()), "current": not reasons, "reasons": reasons,
            "scope": receipt.get("scope", "all"), "warnings": warnings,
            "note": "Local evidence only; ignored dependencies/environment and external services are outside the fingerprint. "
                    "Review the diff against the task's base ref and rerun after integration."}
