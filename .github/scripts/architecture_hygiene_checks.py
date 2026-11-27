#!/usr/bin/env python3
"""Workspace Cargo dependency & orphan Rust source file hygiene checks (Check #5).

Invoked by `.github/scripts/check_architecture_health.py` on every CI and local run.
Pure Python 3 stdlib, zero external dependencies, deterministic fail-closed checks:

1. Unused / redundant internal workspace dependencies:
   - Rejects `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`, and
     `[target.*.*]` workspace crate entries with zero code/macro/binary references.
   - Rejects non-optional `[dependencies]` referenced only in test code (move to
     `[dev-dependencies]`).
   - Rejects redundant `[dev-dependencies]` entries that duplicate an unconditional
     `[dependencies]` entry without enabling extra features or re-enabling default
     features.
   - Preserves zero-false-positive Cargo idioms: self dev-deps (`path = "."`),
     feature-activating / default-feature-enabling / target-widening dev-deps,
     `#[macro_export]` invocations, and `CARGO_BIN_EXE_<bin>` references.

2. Orphan / unreferenced `.rs` files:
   - Discovers every Cargo crate root (`[lib]`, `src/lib.rs`, `src/main.rs`,
     `src/bin/*`, `[[bin]]`, `build.rs`, `tests/*`, `[[test]]`, `examples/*`,
     `[[example]]`, `benches/*`, `[[bench]]`).
   - Traverses `mod <name>;`, inline `mod <name> { ... }`, visibility-qualified
     `pub(...) mod`, unconditional `#[path = "..."]`, multi-/conditional
     `#[cfg_attr(..., path = "...")]` (preserving default fallback), and balanced
     `include!`/`include_str!`/`include_bytes!` (including `concat!(env!("CARGO_MANIFEST_DIR"), ...)`).
   - Rejects any `src/**/*.rs` unreachable from crate src roots and any other `.rs`
     file unreachable from src/test/example/bench roots.
"""

from __future__ import annotations

import os
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

_MACRO_EXPORT_RE = re.compile(
    r"#\s*\[\s*macro_export(?:\s*\([^)]*\))?\s*\]"
    r"(?:\s*#\[[^\]]*\])*\s*macro_rules\s*!\s*([A-Za-z_][A-Za-z0-9_]*)"
)
_MOD_DECL_RE = re.compile(r"\bmod\s+(?:r#)?([A-Za-z_][A-Za-z0-9_]*)\s*([;{])")
_MOD_TOKEN_RE = re.compile(r"\bmod\s+(?:r#)?([A-Za-z_][A-Za-z0-9_]*)\s*([;{])|[{}]")
_DIRECT_PATH_ATTR_RE = re.compile(r'^\s*#\s*\[\s*path\s*=\s*"([^"]+)"\s*\]\s*$')
_CFG_ATTR_HEAD_RE = re.compile(r"^\s*#\s*\[\s*cfg_attr\s*\(")
_PATH_KV_RE = re.compile(r'\bpath\s*=\s*"([^"]+)"')
_INCLUDE_CALL_RE = re.compile(r"\binclude(?:_str|_bytes)?\s*!\s*\(")
_RAW_STRING_RE = re.compile(r'"([^"]+)"')

_SECTION_KINDS = (
    ("dependencies", "normal"),
    ("dev-dependencies", "dev"),
    ("dev_dependencies", "dev"),
    ("build-dependencies", "build"),
    ("build_dependencies", "build"),
)


@dataclass(frozen=True)
class _WorkspaceDep:
    alias: str
    target_pkg: str
    kind: str  # "normal" | "dev" | "build"
    features: frozenset[str]
    optional: bool
    default_features: bool
    target_scoped: bool


@dataclass(frozen=True)
class _MergedDepSpec:
    features: frozenset[str]
    optional: bool
    default_features: bool
    target_scoped: bool


@dataclass
class _CrateTextBuckets:
    prod_masked: list[str] = field(default_factory=list)
    prod_raw: list[str] = field(default_factory=list)
    test_masked: list[str] = field(default_factory=list)
    test_raw: list[str] = field(default_factory=list)
    build_masked: list[str] = field(default_factory=list)
    build_raw: list[str] = field(default_factory=list)


def _strip_ranges(text: str, ranges: list[tuple[int, int]]) -> tuple[str, str]:
    """Return `(without_ranges, only_ranges)` preserving positions cleanly."""
    if not ranges:
        return text, ""
    kept: list[str] = []
    removed: list[str] = []
    cursor = 0
    for start, end in ranges:
        if start > cursor:
            kept.append(text[cursor:start])
        removed.append(text[start:end])
        cursor = max(cursor, end)
    if cursor < len(text):
        kept.append(text[cursor:])
    return "".join(kept), "\n".join(removed)


def _resolve_dep_pkg(
    alias: str, val: Any, workspace_dep_renames: dict[str, str]
) -> tuple[str, frozenset[str], bool, bool]:
    pkg = alias
    feats: set[str] = set()
    optional = False
    default_features = True
    if isinstance(val, dict):
        if isinstance(val.get("package"), str):
            pkg = val["package"]
        if val.get("workspace") is True and pkg in workspace_dep_renames:
            pkg = workspace_dep_renames[pkg]
        raw_feats = val.get("features")
        if isinstance(raw_feats, list):
            feats.update(f for f in raw_feats if isinstance(f, str))
        optional = bool(val.get("optional") is True)
        if val.get("default-features") is False or val.get("default_features") is False:
            default_features = False
    return pkg, frozenset(feats), optional, default_features


def _collect_table_deps(
    table: Any,
    kind: str,
    target_scoped: bool,
    workspace_pkgs: set[str],
    workspace_dep_renames: dict[str, str],
) -> list[_WorkspaceDep]:
    if not isinstance(table, dict):
        return []
    out: list[_WorkspaceDep] = []
    for alias, val in table.items():
        pkg, feats, optional, default_features = _resolve_dep_pkg(
            alias, val, workspace_dep_renames
        )
        if pkg in workspace_pkgs:
            out.append(
                _WorkspaceDep(alias, pkg, kind, feats, optional, default_features, target_scoped)
            )
    return out


def _collect_section_group_deps(
    container: dict[str, Any],
    target_scoped: bool,
    workspace_pkgs: set[str],
    workspace_dep_renames: dict[str, str],
) -> list[_WorkspaceDep]:
    deps: list[_WorkspaceDep] = []
    for key, kind in _SECTION_KINDS:
        deps.extend(
            _collect_table_deps(
                container.get(key), kind, target_scoped, workspace_pkgs, workspace_dep_renames
            )
        )
    return deps


def _collect_member_workspace_deps(
    data: dict[str, Any],
    workspace_pkgs: set[str],
    workspace_dep_renames: dict[str, str],
) -> list[_WorkspaceDep]:
    deps = _collect_section_group_deps(data, False, workspace_pkgs, workspace_dep_renames)
    targets = data.get("target")
    if not isinstance(targets, dict):
        return deps
    for tdata in targets.values():
        if isinstance(tdata, dict):
            deps.extend(
                _collect_section_group_deps(tdata, True, workspace_pkgs, workspace_dep_renames)
            )
    return deps


def _lib_ident_for(pkg_name: str, data: dict[str, Any]) -> str:
    lib = data.get("lib")
    if isinstance(lib, dict) and isinstance(lib.get("name"), str) and lib["name"]:
        return lib["name"].replace("-", "_")
    return pkg_name.replace("-", "_")


def _discover_bin_names(pkg_name: str, member_dir: Path, data: dict[str, Any]) -> frozenset[str]:
    names: set[str] = {pkg_name}
    pkg = data.get("package") or {}
    bins = data.get("bin")
    if isinstance(bins, list):
        for item in bins:
            if isinstance(item, dict) and isinstance(item.get("name"), str) and item["name"]:
                names.add(item["name"])
    bin_dir = member_dir / "src/bin"
    if pkg.get("autobins") is not False and bin_dir.is_dir():
        for entry in sorted(bin_dir.iterdir()):
            if entry.is_file() and entry.suffix == ".rs":
                names.add(entry.stem)
            elif entry.is_dir() and (entry / "main.rs").is_file():
                names.add(entry.name)
    return frozenset(names)


def _resolve_build_script(member_dir: Path, data: dict[str, Any]) -> Path | None:
    pkg = data.get("package") or {}
    build_cfg = pkg.get("build")
    if build_cfg is False:
        return None
    if isinstance(build_cfg, str) and build_cfg:
        cand = (member_dir / build_cfg).resolve()
        return cand if cand.is_file() else None
    default = (member_dir / "build.rs").resolve()
    return default if default.is_file() else None


def _load_masked_rs(
    rs_path: Path,
    resolved: Path,
    mask_fn: Callable[[str], tuple[str, list[tuple[int, int]]]],
    cached_files: dict[Path, Any] | None,
) -> tuple[str, str, list[tuple[int, int]] | None]:
    entry = cached_files.get(resolved) if cached_files is not None else None
    if entry is not None:
        raw = entry.raw
        if entry.masked is None:
            entry.masked, _ = mask_fn(raw)
        return raw, entry.masked, entry.cfg_test_ranges
    raw = rs_path.read_text(encoding="utf-8", errors="replace")
    masked, _ = mask_fn(raw)
    return raw, masked, None


def _rel_posix_fast(path: Path, base: Path, base_prefix: str) -> str:
    raw = path.as_posix()
    if raw.startswith(base_prefix):
        return raw[len(base_prefix) :]
    return path.relative_to(base).as_posix()


def _route_crate_source(
    buckets: _CrateTextBuckets,
    resolved: Path,
    rel: str,
    raw: str,
    masked: str,
    cached_ranges: list[tuple[int, int]] | None,
    build_script: Path | None,
    is_test_path: Callable[[str], bool],
    cfg_test_fn: Callable[[str], list[tuple[int, int]]],
) -> None:
    if build_script is not None and resolved == build_script:
        buckets.build_masked.append(masked)
        buckets.build_raw.append(raw)
        return
    if is_test_path(rel) or not rel.startswith("src/"):
        buckets.test_masked.append(masked)
        buckets.test_raw.append(raw)
        return
    ranges = (
        cached_ranges
        if cached_ranges is not None
        else (cfg_test_fn(masked) if "cfg" in masked else [])
    )
    prod_m, test_m = _strip_ranges(masked, ranges)
    prod_r, test_r = _strip_ranges(raw, ranges)
    buckets.prod_masked.append(prod_m)
    buckets.prod_raw.append(prod_r)
    if test_m:
        buckets.test_masked.append(test_m)
        buckets.test_raw.append(test_r)


def _bucket_crate_sources(
    member_dir: Path,
    rs_files: list[Path],
    build_script: Path | None,
    is_test_path: Callable[[str], bool],
    mask_fn: Callable[[str], tuple[str, list[tuple[int, int]]]],
    cfg_test_fn: Callable[[str], list[tuple[int, int]]],
    cached_files: dict[Path, Any] | None = None,
) -> tuple[_CrateTextBuckets, dict[Path, tuple[str, str]], frozenset[str], list[tuple[Path, str]]]:
    buckets = _CrateTextBuckets()
    cache: dict[Path, tuple[str, str]] = {}
    exported_macros: set[str] = set()
    resolved_entries: list[tuple[Path, str]] = []
    member_prefix = f"{member_dir.as_posix().rstrip('/')}/"
    for rs_path in rs_files:
        resolved = rs_path.resolve() if os.path.islink(rs_path) else rs_path
        raw, masked, cached_ranges = _load_masked_rs(rs_path, resolved, mask_fn, cached_files)
        cache[resolved] = (raw, masked)
        rel = _rel_posix_fast(rs_path, member_dir, member_prefix)
        resolved_entries.append((resolved, rel))
        if rel.startswith("src/") and "macro_export" in raw:
            exported_macros.update(_MACRO_EXPORT_RE.findall(masked))
        _route_crate_source(
            buckets, resolved, rel, raw, masked, cached_ranges, build_script, is_test_path, cfg_test_fn
        )
    return buckets, cache, frozenset(exported_macros), resolved_entries


def _is_word_char(ch: str) -> bool:
    return ch.isalnum() or ch == "_"


def _at_word_boundary(text: str, start: int, end: int) -> bool:
    left_ok = start == 0 or not _is_word_char(text[start - 1])
    right_ok = end == len(text) or not _is_word_char(text[end])
    return left_ok and right_ok


def _skip_ws_forward(text: str, pos: int) -> int:
    n = len(text)
    while pos < n and text[pos].isspace():
        pos += 1
    return pos


def _skip_ws_backward(text: str, pos: int) -> int:
    while pos > 0 and text[pos - 1].isspace():
        pos -= 1
    return pos


def _has_keyword_before(text: str, pos: int, kw: str) -> int | None:
    start = pos - len(kw)
    if start < 0 or text[start:pos] != kw:
        return None
    if start > 0 and _is_word_char(text[start - 1]):
        return None
    return start


def _preceded_by_use_or_extern_crate(text: str, pos: int) -> bool:
    k = _skip_ws_backward(text, pos)
    if k == pos:
        return False
    if _has_keyword_before(text, k, "use") is not None:
        return True
    crate_start = _has_keyword_before(text, k, "crate")
    if crate_start is None:
        return False
    e = _skip_ws_backward(text, crate_start)
    return e < crate_start and _has_keyword_before(text, e, "extern") is not None


def _ident_used_at(text: str, pos: int, end: int) -> bool:
    if not _at_word_boundary(text, pos, end):
        return False
    if pos >= 2 and text[pos - 2 : pos] == "::":
        return True
    j = _skip_ws_forward(text, end)
    if text[j : j + 2] == "::":
        return True
    return _preceded_by_use_or_extern_crate(text, pos)


def _ident_is_used(ident: str, masked_blob: str) -> bool:
    ident_len = len(ident)
    pos = masked_blob.find(ident)
    while pos != -1:
        if _ident_used_at(masked_blob, pos, pos + ident_len):
            return True
        pos = masked_blob.find(ident, pos + 1)
    return False


def _macro_is_used(macro: str, masked_blob: str) -> bool:
    macro_len = len(macro)
    pos = masked_blob.find(macro)
    while pos != -1:
        end = pos + macro_len
        if _at_word_boundary(masked_blob, pos, end):
            j = _skip_ws_forward(masked_blob, end)
            if j < len(masked_blob) and masked_blob[j] == "!":
                return True
        pos = masked_blob.find(macro, pos + 1)
    return False


def _bin_env_is_used(bin_names: frozenset[str], raw_blob: str) -> bool:
    if "CARGO_BIN_EXE_" not in raw_blob:
        return False
    for name in bin_names:
        if f"CARGO_BIN_EXE_{name}" not in raw_blob:
            continue
        if re.search(rf"(?<![A-Za-z0-9_])CARGO_BIN_EXE_{re.escape(name)}(?![A-Za-z0-9_-])", raw_blob):
            return True
    return False


def _is_dep_referenced(
    ident: str,
    macros: frozenset[str],
    bin_names: frozenset[str],
    masked_blob: str,
    raw_blob: str,
) -> bool:
    if _bin_env_is_used(bin_names, raw_blob):
        return True
    if ident in masked_blob and _ident_is_used(ident, masked_blob):
        return True
    return any(macro in masked_blob and _macro_is_used(macro, masked_blob) for macro in macros)


def _merge_deps_by_kind(
    deps: list[_WorkspaceDep],
) -> tuple[
    dict[tuple[str, str], _MergedDepSpec],
    dict[tuple[str, str], _MergedDepSpec],
    dict[tuple[str, str], _MergedDepSpec],
]:
    by_kind: dict[str, dict[tuple[str, str], _MergedDepSpec]] = {
        "normal": {},
        "dev": {},
        "build": {},
    }
    for dep in deps:
        slot = by_kind[dep.kind]
        key = (dep.alias, dep.target_pkg)
        if key in slot:
            old = slot[key]
            slot[key] = _MergedDepSpec(
                features=old.features | dep.features,
                optional=old.optional and dep.optional,
                default_features=old.default_features or dep.default_features,
                target_scoped=old.target_scoped and dep.target_scoped,
            )
        else:
            slot[key] = _MergedDepSpec(
                features=dep.features,
                optional=dep.optional,
                default_features=dep.default_features,
                target_scoped=dep.target_scoped,
            )
    return by_kind["normal"], by_kind["dev"], by_kind["build"]


def _dep_ident(alias: str, target_pkg: str, lib_idents: dict[str, str]) -> str:
    return alias.replace("-", "_") if alias != target_pkg else lib_idents[target_pkg]


def _check_normal_deps(
    pkg_name: str,
    manifest_rel: str,
    normal_map: dict[tuple[str, str], _MergedDepSpec],
    prod_blobs: tuple[str, str],
    test_blobs: tuple[str, str],
    lib_idents: dict[str, str],
    exported_macros: dict[str, frozenset[str]],
    bin_names_map: dict[str, frozenset[str]],
) -> list[str]:
    errors: list[str] = []
    for (alias, target_pkg), spec in sorted(normal_map.items()):
        ident = _dep_ident(alias, target_pkg, lib_idents)
        macros = exported_macros[target_pkg]
        bins = bin_names_map[target_pkg]
        if _is_dep_referenced(ident, macros, bins, prod_blobs[0], prod_blobs[1]):
            continue
        in_test = _is_dep_referenced(ident, macros, bins, test_blobs[0], test_blobs[1])
        if in_test and not spec.optional:
            errors.append(
                f"Unused production dependency in {manifest_rel}: `{alias}` (`{target_pkg}`) "
                f"is only referenced in test code; move it to [dev-dependencies]."
            )
        elif not in_test:
            errors.append(
                f"Unused workspace dependency in {manifest_rel}: [dependencies] `{alias}` "
                f"(`{target_pkg}`) has zero references in `{pkg_name}`."
            )
    return errors


def _is_redundant_dev_dep(norm: _MergedDepSpec, dev: _MergedDepSpec) -> bool:
    if norm.optional:
        return False
    if norm.target_scoped and not dev.target_scoped:
        return False
    if not norm.default_features and dev.default_features:
        return False
    return dev.features <= norm.features


def _check_dev_deps(
    pkg_name: str,
    manifest_rel: str,
    normal_map: dict[tuple[str, str], _MergedDepSpec],
    dev_map: dict[tuple[str, str], _MergedDepSpec],
    all_blobs: tuple[str, str],
    lib_idents: dict[str, str],
    exported_macros: dict[str, frozenset[str]],
    bin_names_map: dict[str, frozenset[str]],
) -> list[str]:
    errors: list[str] = []
    for (alias, target_pkg), dev_spec in sorted(dev_map.items()):
        if target_pkg == pkg_name:
            continue
        if (alias, target_pkg) in normal_map:
            if _is_redundant_dev_dep(normal_map[(alias, target_pkg)], dev_spec):
                errors.append(
                    f"Redundant workspace dev-dependency in {manifest_rel}: `{alias}` "
                    f"is already in [dependencies] with the same or superset features."
                )
            continue
        ident = _dep_ident(alias, target_pkg, lib_idents)
        if not _is_dep_referenced(
            ident,
            exported_macros[target_pkg],
            bin_names_map[target_pkg],
            all_blobs[0],
            all_blobs[1],
        ):
            errors.append(
                f"Unused workspace dev-dependency in {manifest_rel}: [dev-dependencies] "
                f"`{alias}` (`{target_pkg}`) has zero references in `{pkg_name}`."
            )
    return errors


def _check_build_deps(
    manifest_rel: str,
    build_map: dict[tuple[str, str], _MergedDepSpec],
    build_blobs: tuple[str, str],
    lib_idents: dict[str, str],
    exported_macros: dict[str, frozenset[str]],
    bin_names_map: dict[str, frozenset[str]],
) -> list[str]:
    errors: list[str] = []
    for alias, target_pkg in sorted(build_map):
        ident = _dep_ident(alias, target_pkg, lib_idents)
        if not _is_dep_referenced(
            ident,
            exported_macros[target_pkg],
            bin_names_map[target_pkg],
            build_blobs[0],
            build_blobs[1],
        ):
            errors.append(
                f"Unused workspace build-dependency in {manifest_rel}: [build-dependencies] "
                f"`{alias}` (`{target_pkg}`) has zero references in build script."
            )
    return errors


def _check_member_deps(
    pkg_name: str,
    manifest_rel: str,
    deps: list[_WorkspaceDep],
    buckets: _CrateTextBuckets,
    lib_idents: dict[str, str],
    exported_macros: dict[str, frozenset[str]],
    bin_names_map: dict[str, frozenset[str]],
) -> list[str]:
    normal_map, dev_map, build_map = _merge_deps_by_kind(deps)
    prod_blobs = ("\n".join(buckets.prod_masked), "\n".join(buckets.prod_raw))
    test_blobs = ("\n".join(buckets.test_masked), "\n".join(buckets.test_raw))
    all_blobs = (f"{prod_blobs[0]}\n{test_blobs[0]}", f"{prod_blobs[1]}\n{test_blobs[1]}")
    build_blobs = ("\n".join(buckets.build_masked), "\n".join(buckets.build_raw))
    return [
        *_check_normal_deps(
            pkg_name,
            manifest_rel,
            normal_map,
            prod_blobs,
            test_blobs,
            lib_idents,
            exported_macros,
            bin_names_map,
        ),
        *_check_dev_deps(
            pkg_name,
            manifest_rel,
            normal_map,
            dev_map,
            all_blobs,
            lib_idents,
            exported_macros,
            bin_names_map,
        ),
        *_check_build_deps(
            manifest_rel, build_map, build_blobs, lib_idents, exported_macros, bin_names_map
        ),
    ]


# ---------------------------------------------------------------------------
# Check #5b: Orphan / unreferenced `.rs` source file discovery
# ---------------------------------------------------------------------------


def _add_auto_targets(directory: Path, out: set[Path]) -> None:
    if not directory.is_dir():
        return
    for entry in sorted(directory.iterdir()):
        if entry.is_file() and entry.suffix == ".rs":
            out.add(entry.resolve())
        elif entry.is_dir():
            main_rs = entry / "main.rs"
            if main_rs.is_file():
                out.add(main_rs.resolve())


def _add_explicit_targets(member_dir: Path, entries: Any, out: set[Path]) -> None:
    if not isinstance(entries, list):
        return
    for item in entries:
        if isinstance(item, dict) and isinstance(item.get("path"), str):
            cand = (member_dir / item["path"]).resolve()
            if cand.is_file():
                out.add(cand)


def _discover_lib_root(member_dir: Path, data: dict[str, Any]) -> Path | None:
    lib = data.get("lib")
    if isinstance(lib, dict) and isinstance(lib.get("path"), str):
        cand = (member_dir / lib["path"]).resolve()
        return cand if cand.is_file() else None
    default = (member_dir / "src/lib.rs").resolve()
    return default if default.is_file() else None


def _discover_src_roots(
    member_dir: Path, data: dict[str, Any], build_script: Path | None
) -> set[Path]:
    pkg = data.get("package") or {}
    roots: set[Path] = set()
    lib_root = _discover_lib_root(member_dir, data)
    if lib_root is not None:
        roots.add(lib_root)
    if (member_dir / "src/main.rs").is_file():
        roots.add((member_dir / "src/main.rs").resolve())
    if pkg.get("autobins") is not False:
        _add_auto_targets(member_dir / "src/bin", roots)
    _add_explicit_targets(member_dir, data.get("bin"), roots)
    if build_script is not None:
        roots.add(build_script)
    return roots


def _discover_extra_roots(member_dir: Path, data: dict[str, Any]) -> set[Path]:
    pkg = data.get("package") or {}
    roots: set[Path] = set()
    for folder, auto_key, table_key in (
        ("tests", "autotests", "test"),
        ("examples", "autoexamples", "example"),
        ("benches", "autobenches", "bench"),
    ):
        if pkg.get(auto_key) is not False:
            _add_auto_targets(member_dir / folder, roots)
        _add_explicit_targets(member_dir, data.get(table_key), roots)
    return roots


def _discover_crate_roots(
    member_dir: Path, data: dict[str, Any], build_script: Path | None
) -> tuple[set[Path], set[Path]]:
    return (
        _discover_src_roots(member_dir, data, build_script),
        _discover_extra_roots(member_dir, data),
    )


def _match_open_paren_backward(code: str, close_idx: int) -> int | None:
    depth = 1
    j = close_idx - 1
    while j >= 0 and depth > 0:
        ch = code[j]
        if ch == ")":
            depth += 1
        elif ch == "(":
            depth -= 1
        j -= 1
    return (j + 1) if depth == 0 else None


def _skip_mod_visibility(masked: str, mod_pos: int) -> int:
    """Step backward past optional `pub` or `pub(...)` before `mod` so outer attrs are found."""
    idx = mod_pos
    while idx > 0 and masked[idx - 1].isspace():
        idx -= 1
    if idx > 0 and masked[idx - 1] == ")":
        open_paren = _match_open_paren_backward(masked, idx - 1)
        if open_paren is None:
            return mod_pos
        idx = open_paren
        while idx > 0 and masked[idx - 1].isspace():
            idx -= 1
    if idx >= 3 and masked[idx - 3 : idx] == "pub":
        before = idx - 4
        if before < 0 or not (masked[before].isalnum() or masked[before] == "_"):
            return idx - 3
    return mod_pos


def _extract_path_attrs(
    raw: str,
    masked: str,
    mod_pos: int,
    prev_attr_fn: Callable[[str, int, int], tuple[int, int] | None],
) -> tuple[list[str], list[str]]:
    """Collect `(unconditional_paths, conditional_cfg_attr_paths)` preceding a `mod` item."""
    unconditional: list[str] = []
    conditional: list[str] = []
    pos = _skip_mod_visibility(masked, mod_pos)
    while True:
        span = prev_attr_fn(masked, pos, 0)
        if span is None:
            break
        attr_raw = raw[span[0] : span[1]]
        direct = _DIRECT_PATH_ATTR_RE.match(attr_raw)
        if direct:
            unconditional.append(direct.group(1))
        elif _CFG_ATTR_HEAD_RE.match(attr_raw):
            conditional.extend(_PATH_KV_RE.findall(attr_raw))
        pos = span[0]
    return unconditional, conditional


def _resolve_explicit_path(file_path: Path, cur_dirs: list[Path], rel_path: str) -> Path | None:
    for base in (*cur_dirs, file_path.parent):
        cand = (base / rel_path).resolve()
        if cand.is_file():
            return cand
    return None


def _resolve_if_file(path: Path) -> Path | None:
    if not path.is_file():
        return None
    return path.resolve() if os.path.islink(path) else path


def _resolve_mod_targets(
    file_path: Path,
    cur_dirs: list[Path],
    mod_name: str,
    unconditional: list[str],
    conditional: list[str],
) -> list[tuple[Path, Path]]:
    found: list[tuple[Path, Path]] = []
    for rel_path in (*unconditional, *conditional):
        cand = _resolve_explicit_path(file_path, cur_dirs, rel_path)
        if cand is not None:
            found.append((cand, cand.parent))
    if unconditional:
        return found
    for cur_dir in cur_dirs:
        file_cand = _resolve_if_file(cur_dir / f"{mod_name}.rs")
        if file_cand is not None:
            found.append((file_cand, cur_dir / mod_name))
        dir_cand = _resolve_if_file(cur_dir / mod_name / "mod.rs")
        if dir_cand is not None:
            found.append((dir_cand, dir_cand.parent))
    return found


def _extract_include_targets(
    member_dir: Path,
    file_path: Path,
    raw: str,
    masked: str,
    skip_balanced_fn: Callable[[str, int, str, str], int],
) -> list[Path]:
    if "include" not in masked:
        return []
    targets: list[Path] = []
    for m in _INCLUDE_CALL_RE.finditer(masked):
        close_end = skip_balanced_fn(masked, m.end(), "(", ")")
        if close_end <= m.end() or masked[close_end - 1] != ")":
            continue
        inner = raw[m.end() : close_end - 1]
        lits = _RAW_STRING_RE.findall(inner)
        if not lits:
            continue
        if "CARGO_MANIFEST_DIR" in inner:
            tail = [lit for lit in lits if lit != "CARGO_MANIFEST_DIR"]
            if not tail:
                continue
            rel = "".join(tail).lstrip("/")
            base = member_dir
        else:
            rel = "".join(lits)
            base = file_path.parent
        cand = (base / rel).resolve()
        if cand.is_file() and cand.suffix == ".rs":
            targets.append(cand)
    return targets


def _next_inline_mod_dirs(
    cur_dirs: list[Path], mod_name: str, unconditional: list[str], conditional: list[str]
) -> list[Path]:
    dirs: list[Path] = []
    for cur_dir in cur_dirs:
        for rel_path in (*unconditional, *conditional):
            dirs.append(cur_dir / rel_path)
        if not unconditional:
            dirs.append(cur_dir / mod_name)
    return dirs


def _scan_inline_mod_edges(
    file_path: Path,
    default_dir: Path,
    raw: str,
    masked: str,
    prev_attr_fn: Callable[[str, int, int], tuple[int, int] | None],
) -> list[tuple[Path, Path]]:
    edges: list[tuple[Path, Path]] = []
    brace_depth = 0
    dir_stack: list[tuple[int, list[Path]]] = [(0, [default_dir])]
    for m in _MOD_TOKEN_RE.finditer(masked):
        tok = m.group(0)
        if tok == "{":
            brace_depth += 1
            continue
        if tok == "}":
            brace_depth = max(0, brace_depth - 1)
            while len(dir_stack) > 1 and dir_stack[-1][0] > brace_depth:
                dir_stack.pop()
            continue
        mod_name, delim = m.group(1), m.group(2)
        cur_dirs = dir_stack[-1][1]
        uncond, cond = _extract_path_attrs(raw, masked, m.start(), prev_attr_fn)
        if delim == "{":
            brace_depth += 1
            dir_stack.append((brace_depth, _next_inline_mod_dirs(cur_dirs, mod_name, uncond, cond)))
        else:
            edges.extend(_resolve_mod_targets(file_path, cur_dirs, mod_name, uncond, cond))
    return edges


def _scan_file_mod_edges(
    file_path: Path,
    default_dir: Path,
    raw: str,
    masked: str,
    prev_attr_fn: Callable[[str, int, int], tuple[int, int] | None],
) -> list[tuple[Path, Path]]:
    if "mod" not in masked:
        return []
    mod_matches = list(_MOD_DECL_RE.finditer(masked))
    if not mod_matches or not any(m.group(2) == ";" for m in mod_matches):
        return []
    if all(m.group(2) == ";" for m in mod_matches):
        edges: list[tuple[Path, Path]] = []
        for m in mod_matches:
            uncond, cond = _extract_path_attrs(raw, masked, m.start(), prev_attr_fn)
            edges.extend(_resolve_mod_targets(file_path, [default_dir], m.group(1), uncond, cond))
        return edges
    return _scan_inline_mod_edges(file_path, default_dir, raw, masked, prev_attr_fn)


def _crawl_reachable_rs(
    member_dir: Path,
    roots: set[Path],
    cache: dict[Path, tuple[str, str]],
    prev_attr_fn: Callable[[str, int, int], tuple[int, int] | None],
    skip_balanced_fn: Callable[[str, int, str, str], int],
) -> set[Path]:
    visited: set[Path] = set()
    queue: list[tuple[Path, Path]] = [(root, root.parent) for root in sorted(roots)]
    while queue:
        file_path, default_dir = queue.pop()
        if file_path in visited or file_path not in cache:
            continue
        visited.add(file_path)
        raw, masked = cache[file_path]
        neighbors = [
            (inc, inc.parent)
            for inc in _extract_include_targets(
                member_dir, file_path, raw, masked, skip_balanced_fn
            )
        ]
        neighbors.extend(_scan_file_mod_edges(file_path, default_dir, raw, masked, prev_attr_fn))
        queue.extend(edge for edge in neighbors if edge[0] not in visited)
    return visited


def _check_member_orphans(
    repo_root: Path,
    repo_prefix: str,
    member_dir: Path,
    data: dict[str, Any],
    resolved_entries: list[tuple[Path, str]],
    build_script: Path | None,
    cache: dict[Path, tuple[str, str]],
    prev_attr_fn: Callable[[str, int, int], tuple[int, int] | None],
    skip_balanced_fn: Callable[[str, int, str, str], int],
) -> list[str]:
    src_roots, extra_roots = _discover_crate_roots(member_dir, data, build_script)
    reachable_src = _crawl_reachable_rs(
        member_dir, src_roots, cache, prev_attr_fn, skip_balanced_fn
    )
    reachable_all = reachable_src | _crawl_reachable_rs(
        member_dir, extra_roots, cache, prev_attr_fn, skip_balanced_fn
    )
    errors: list[str] = []
    for resolved, rel_crate in resolved_entries:
        if rel_crate.startswith("src/") and resolved not in reachable_src:
            rel_repo = _rel_posix_fast(resolved, repo_root, repo_prefix)
            errors.append(
                f"Orphan Rust source file `{rel_repo}`: unreachable from crate src roots "
                f"(missing `mod` declaration or leftover file)."
            )
        elif resolved not in reachable_all:
            rel_repo = _rel_posix_fast(resolved, repo_root, repo_prefix)
            errors.append(
                f"Orphan Rust file `{rel_repo}`: unreachable from any crate target root "
                f"(src/tests/examples/benches)."
            )
    return errors


@dataclass
class _PreparedMember:
    pkg_name: str
    member_res: Path
    data: dict[str, Any]
    resolved_entries: list[tuple[Path, str]]
    build_script: Path | None
    buckets: _CrateTextBuckets
    cache: dict[Path, tuple[str, str]]


def _prepare_members(
    members: list[tuple[str, Path, dict[str, Any]]],
    *,
    iter_rs: Callable[[Path], list[Path]],
    is_test_path: Callable[[str], bool],
    mask_fn: Callable[[str], tuple[str, list[tuple[int, int]]]],
    cfg_test_fn: Callable[[str], list[tuple[int, int]]],
    source_cache: Any,
) -> tuple[list[_PreparedMember], dict[str, frozenset[str]]]:
    cached_member_files = source_cache.member_files if source_cache is not None else {}
    cached_files = source_cache.files if source_cache is not None else None
    exported_macros: dict[str, frozenset[str]] = {}
    prepared: list[_PreparedMember] = []
    for pkg_name, member_dir, data in members:
        member_res = member_dir.resolve()
        rs_files = cached_member_files.get(pkg_name)
        if rs_files is None:
            rs_files = iter_rs(member_res)
        build_script = _resolve_build_script(member_res, data)
        buckets, cache, pkg_macros, resolved_entries = _bucket_crate_sources(
            member_res, rs_files, build_script, is_test_path, mask_fn, cfg_test_fn, cached_files
        )
        exported_macros[pkg_name] = pkg_macros
        prepared.append(
            _PreparedMember(
                pkg_name, member_res, data, resolved_entries, build_script, buckets, cache
            )
        )
    return prepared, exported_macros


def check_workspace_hygiene(
    repo_root: Path,
    members: list[tuple[str, Path, dict[str, Any]]],
    workspace_dep_renames: dict[str, str],
    *,
    iter_rs: Callable[[Path], list[Path]],
    is_test_path: Callable[[str], bool],
    mask_fn: Callable[[str], tuple[str, list[tuple[int, int]]]],
    cfg_test_fn: Callable[[str], list[tuple[int, int]]],
    prev_attr_fn: Callable[[str, int, int], tuple[int, int] | None],
    skip_balanced_fn: Callable[[str, int, str, str], int],
    source_cache: Any = None,
) -> list[str]:
    """Run Check #5: unused workspace deps and orphan `.rs` source files across all members."""
    repo_root_res = repo_root.resolve()
    repo_prefix = f"{repo_root_res.as_posix().rstrip('/')}/"
    workspace_pkgs = {name for name, _, _ in members}
    lib_idents = {name: _lib_ident_for(name, data) for name, _, data in members}
    bin_names_map = {
        name: _discover_bin_names(name, member_dir, data) for name, member_dir, data in members
    }
    prepared, exported_macros = _prepare_members(
        members,
        iter_rs=iter_rs,
        is_test_path=is_test_path,
        mask_fn=mask_fn,
        cfg_test_fn=cfg_test_fn,
        source_cache=source_cache,
    )
    errors: list[str] = []
    for item in prepared:
        deps = _collect_member_workspace_deps(item.data, workspace_pkgs, workspace_dep_renames)
        manifest_rel = _rel_posix_fast(item.member_res / "Cargo.toml", repo_root_res, repo_prefix)
        errors.extend(
            _check_member_deps(
                item.pkg_name,
                manifest_rel,
                deps,
                item.buckets,
                lib_idents,
                exported_macros,
                bin_names_map,
            )
        )
        errors.extend(
            _check_member_orphans(
                repo_root_res,
                repo_prefix,
                item.member_res,
                item.data,
                item.resolved_entries,
                item.build_script,
                item.cache,
                prev_attr_fn,
                skip_balanced_fn,
            )
        )
    return errors
