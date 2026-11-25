"""Shared settings validation and installation metadata; no third-party packages."""

import hashlib
import fnmatch
import json
from pathlib import Path
from typing import NamedTuple

KIT_VERSION = "0.10.0"
MANIFEST = ".starter-kit-manifest.json"


def protected_name(name):
    if name in {".env.example", ".env.sample", ".env.template"}:
        return False
    return any(fnmatch.fnmatchcase(name, pattern) for pattern in
               (".env", ".env.*", "*.pem", "*.key", "credentials.json"))


def digest(data):
    return hashlib.sha256(data).hexdigest()


def codex_instruction_file(project):
    """Select the root instruction file managed by the kit; blank overrides fall through.

    Return invalid/linked overrides for callers to reject rather than silently
    installing into a lower-priority file. Scoped and configured fallback guidance
    still needs inspection during adoption.
    """
    override = project / "AGENTS.override.md"
    if override.is_symlink() or (override.exists() and
            (not override.is_file() or override.read_bytes().strip())):
        return override
    return project / "AGENTS.md"


def runtime_file(relative):
    parts = Path(relative).parts
    return str(relative) == ".agents/agent-kit/WORKFLOW.md" or any(
        part in {"runtime", "hooks", "agents", "skills", "rules", "templates", "policy", "reference"} for part in parts[:-1])


def installed_mode(project):
    """The installed policy selects a mode; never infer it from CLI availability."""
    value = json.loads((project / ".agents/agent-kit/policy/model-routing.json").read_text())
    mode = value.get("mode")
    if mode not in {"hybrid", "codex-only"}:
        raise ValueError("Install a supported hybrid or codex-only profile")
    return mode


def validate_settings(value, label):
    def require(condition, message):
        if not condition:
            raise ValueError(f"{label}: {message}")

    require(isinstance(value, dict), "settings must be an object")
    for key in ("env", "permissions", "hooks"):
        if key in value:
            require(isinstance(value[key], dict), f"{key} must be an object")
    for key, val in value.get("env", {}).items():
        require(isinstance(val, str), f"env.{key} must be a string")
    for key in ("allow", "deny", "ask", "additionalDirectories"):
        permissions = value.get("permissions", {})
        if key in permissions:
            require(isinstance(permissions[key], list) and
                    all(isinstance(item, str) for item in permissions[key]),
                    f"permissions.{key} must be an array of strings")
    for event, groups in value.get("hooks", {}).items():
        require(isinstance(groups, list), f"hooks.{event} must be an array")
        for i, group in enumerate(groups):
            prefix = f"hooks.{event}[{i}]"
            require(isinstance(group, dict), f"{prefix} must be an object")
            if "matcher" in group:
                require(isinstance(group["matcher"], str), f"{prefix}.matcher must be a string")
            require(isinstance(group.get("hooks"), list), f"{prefix}.hooks must be an array")
            for j, hook in enumerate(group["hooks"]):
                item = f"{prefix}.hooks[{j}]"
                require(isinstance(hook, dict), f"{item} must be an object")
                kind = hook.get("type")
                require(isinstance(kind, str) and bool(kind), f"{item}.type is required")
                # Validate known hook payloads, without rejecting future hook types/fields.
                field = {"command": "command", "prompt": "prompt", "agent": "prompt",
                         "http": "url"}.get(kind)
                if field:
                    require(isinstance(hook.get(field), str) and bool(hook[field].strip()),
                            f"{item}.{field} must be a nonempty string")
                if "timeout" in hook:
                    require(type(hook["timeout"]) in (int, float) and hook["timeout"] > 0,
                            f"{item}.timeout must be positive")
                for key in ("async", "once"):
                    if key in hook:
                        require(type(hook[key]) is bool, f"{item}.{key} must be a boolean")
    return value


def read_settings(path):
    return validate_settings(json.loads(path.read_text()), path)


def read_manifest(path):
    if not path.exists():
        return {"version": 1, "kit_version": KIT_VERSION, "files": {}}
    data = json.loads(path.read_text())
    if (not isinstance(data, dict) or type(data.get("version")) is not int or
            data.get("version") != 1 or not isinstance(data.get("files"), dict)):
        raise ValueError(f"Invalid installation manifest: {path}")
    for relative, checksum in data["files"].items():
        name = Path(relative)
        if (name.is_absolute() or ".." in name.parts or not isinstance(checksum, str)
                or len(checksum) != 64 or any(c not in "0123456789abcdef" for c in checksum)):
            raise ValueError(f"Invalid file entry in installation manifest: {path}")
    return data


class CheckError(Exception):
    pass


def ancestors_in_repo(start):
    for directory in (start, *start.parents):
        yield directory
        if (directory / ".git").exists():
            break


class Check(NamedTuple):
    """One configured command. `paths` (workspace-relative patterns) limit it to changes it
    covers; None means it always runs."""
    directory: Path
    argv: list
    paths: tuple | None = None
    blocking: bool = True


def patterns_ok(value):
    return isinstance(value, list) and value and all(
        isinstance(p, str) and p and not p.startswith(("/", "./")) and
        all(part not in {"", ".", ".."} for part in p.split("/")) for p in value)


def check_entry(directory, entry):
    if isinstance(entry, list):
        entry = {"argv": entry}
    if not isinstance(entry, dict) or not set(entry) <= {"argv", "paths", "blocking"}:
        raise CheckError("Each check is an argument array or an object with argv, paths and blocking")
    argv = entry.get("argv")
    if (not isinstance(argv, list) or not argv or
            any(not isinstance(arg, str) or not arg or "\x00" in arg for arg in argv)):
        raise CheckError("Each check must be a nonempty array of command arguments")
    paths = entry.get("paths")
    if paths is not None and not patterns_ok(paths):
        raise CheckError("Check paths must be a nonempty list of workspace-relative patterns "
                         "(no leading / or ./, no empty, . or .. parts)")
    blocking = entry.get("blocking", True)
    if type(blocking) is not bool:
        raise CheckError("Check blocking must be true or false")
    return Check(directory, argv, tuple(paths) if paths is not None else None, blocking)


def matches(relative, patterns):
    return any(fnmatch.fnmatchcase(relative, pattern) for pattern in patterns)


def covered(check, changed, root):
    """The changed files (absolute) whose workspace-relative path matches the check's paths."""
    return sorted(p for p in changed if p.is_relative_to(root) and matches(p.relative_to(root).as_posix(), check.paths))


def config_workspace_root(config):
    parent = config.parent
    if parent.name == "agent-kit" and parent.parent.name == ".agents":
        return parent.parent.parent.resolve()
    return parent.parent.resolve()


def load_checks(config, cwd):
    try:
        data = json.loads(config.read_text())
    except (OSError, ValueError) as exc:
        raise CheckError(f"Cannot read {config}: {exc}") from exc
    if not isinstance(data, dict) or type(data.get("version")) is not int or data.get("version") != 1:
        raise CheckError("verification.json must be an object with version: 1")
    projects = data.get("projects")
    if not isinstance(projects, list) or not projects:
        raise CheckError("verification.json requires at least one project with explicit checks")
    if "base" in data and (not isinstance(data["base"], str) or not data["base"] or data["base"].startswith("-")):
        raise CheckError("base must be a Git ref naming the integration branch")
    timeout = data.get("timeout_seconds", 240)
    if type(timeout) is not int or not 1 <= timeout <= 300:
        raise CheckError("timeout_seconds must be an integer from 1 to 300 (total budget)")
    root = config_workspace_root(config)
    checks, roots = [], []
    for project in projects:
        if not isinstance(project, dict) or not isinstance(project.get("path"), str) or not project["path"]:
            raise CheckError("Every project requires a nonempty path")
        directory = (root / project["path"]).resolve()
        if not directory.is_dir():
            raise CheckError(f"Configured project does not exist: {directory}")
        commands = project.get("checks")
        if not isinstance(commands, list) or not commands:
            raise CheckError(f"No checks configured for {directory}; verification cannot pass")
        for entry in commands:
            checks.append(check_entry(directory, entry))
        roots.append(directory)
    if not (cwd.is_relative_to(root) or any(cwd.is_relative_to(p) for p in roots)):
        raise CheckError(f"{cwd} is outside the configured workspace and project roots. "
                         "Configure this worktree explicitly; do not verify another checkout.")
    checkout = next((p for p in ancestors_in_repo(cwd) if (p / ".git").exists()), None)
    if (checkout and not root.is_relative_to(checkout)
            and not any(p.is_relative_to(checkout) for p in roots)):
        raise CheckError(f"No checks target the current checkout {checkout}")
    return checks, timeout

