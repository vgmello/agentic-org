#!/usr/bin/env python3
"""Reproducible Agentic Org release packager and Ed25519 manifest signer.

Produces strict USTAR regular-files-only `.tar.gz` release bundles compatible with
`org_host::release::ArchiveBundle` (`agentic-org-release-v1`), SHA-256 checksum files,
and optional Debian `.deb` development packages when `--signing-key` is omitted.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

EXECUTABLES = ("agentic-org", "agentic-org-office", "agentic-org-web", "agent-os")
RELEASE_FORMAT = "agentic-org-release-v1"
ED25519_SPKI_PREFIX = bytes.fromhex("302a300506032b6570032100")
RELEASE_ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,95}$")
MAX_FILES = 16_384
MAX_PAYLOAD_BYTES = 2 * 1024 * 1024 * 1024
MAX_EXECUTABLE_BYTES = 512 * 1024 * 1024
MAX_UI_ASSET_BYTES = 64 * 1024 * 1024
MAX_UI_BYTES = 64 * 1024 * 1024
MAX_UI_ENTRIES = 4096


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def run_openssl(args: list[str]) -> bytes:
    proc = subprocess.run(
        ["openssl", *args],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"openssl {' '.join(args)} failed ({proc.returncode}): "
            f"{proc.stderr.decode('utf-8', 'replace').strip()}"
        )
    return proc.stdout


def raw_ed25519_pubkey_from_key(key_pem: Path) -> bytes:
    spki = run_openssl(["pkey", "-in", str(key_pem), "-pubout", "-outform", "DER"])
    if len(spki) != 44 or not spki.startswith(ED25519_SPKI_PREFIX):
        raise RuntimeError(
            f"expected 44-byte Ed25519 SPKI DER public key, got {len(spki)} bytes"
        )
    return spki[-32:]


def sign_ed25519(key_pem: Path, payload: bytes) -> bytes:
    with tempfile.TemporaryDirectory(prefix="agentic-org-sign-") as td:
        msg = Path(td) / "manifest.json"
        sig = Path(td) / "manifest.sig"
        msg.write_bytes(payload)
        run_openssl(
            [
                "pkeyutl",
                "-sign",
                "-inkey",
                str(key_pem),
                "-rawin",
                "-in",
                str(msg),
                "-out",
                str(sig),
            ]
        )
        raw = sig.read_bytes()
    if len(raw) != 64:
        raise RuntimeError(f"expected 64-byte Ed25519 signature, got {len(raw)}")
    return raw


def cmd_keygen(args: argparse.Namespace) -> int:
    if args.min_sequence < 1 or args.max_sequence < args.min_sequence:
        raise SystemExit(
            "sequence bounds must satisfy 1 <= min_sequence <= max_sequence"
        )
    out_dir = Path(args.out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    os.chmod(out_dir, 0o700)
    key_path = out_dir / "publisher.key.pem"
    reg_path = out_dir / "release-trust-registry.json"
    if key_path.exists() and not args.force:
        raise SystemExit(
            f"refusing to overwrite existing key {key_path}; pass --force to replace"
        )
    old_umask = os.umask(0o077)
    try:
        run_openssl(["genpkey", "-algorithm", "ED25519", "-out", str(key_path)])
        os.chmod(key_path, 0o600)
    finally:
        os.umask(old_umask)
    pub_raw = raw_ed25519_pubkey_from_key(key_path)
    key_id = hashlib.sha256(pub_raw).hexdigest()
    registry = {
        "version": 1,
        "keys": [
            {
                "key_id": key_id,
                "public_key_hex": pub_raw.hex(),
                "min_sequence": args.min_sequence,
                "max_sequence": args.max_sequence,
            }
        ],
    }
    reg_path.write_text(json.dumps(registry, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"key_path": str(key_path), "registry_path": str(reg_path), "key_id": key_id}))
    return 0


def macos_cdhash_for(path: Path) -> str | None:
    proc = subprocess.run(
        ["codesign", "-dvvv", str(path)],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if proc.returncode != 0:
        return None
    combined = proc.stdout + "\n" + proc.stderr
    match = re.search(r"^CDHash=([0-9a-fA-F]{40})\b", combined, re.MULTILINE)
    return match.group(1).lower() if match else None


def collect_ui_files(ui_dir: Path) -> list[tuple[str, Path]]:
    if not ui_dir.is_dir() or not (ui_dir / "index.html").is_file():
        raise SystemExit(f"UI build directory {ui_dir} must exist and contain index.html")
    entries: list[tuple[str, Path]] = []
    for root, dirs, files in os.walk(ui_dir):
        dirs.sort()
        files.sort()
        for fname in files:
            if fname == ".DS_Store" or fname.startswith("._"):
                continue
            full = Path(root) / fname
            if full.is_symlink() or not full.is_file():
                continue
            rel = full.relative_to(ui_dir).as_posix()
            entries.append((f"ui/{rel}", full))
    entries.sort(key=lambda item: item[0])
    return entries


def add_ustar_file(tf: tarfile.TarFile, arcname: str, data: bytes, executable: bool) -> None:
    info = tarfile.TarInfo(name=arcname)
    info.size = len(data)
    info.mode = 0o500 if executable else 0o400
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""
    info.mtime = 0
    info.type = tarfile.REGTYPE
    tf.addfile(info, io.BytesIO(data))


def write_deterministic_ustar_gz(
    dest: Path, files: list[tuple[str, bytes, bool]]
) -> None:
    # Deterministic USTAR regular-files-only archive with zero directory entries,
    # compatible with `org_host::release::ArchiveBundle::open` (ADR-0009).
    buf = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=buf, mtime=0) as gz:
        with tarfile.open(fileobj=gz, mode="w", format=tarfile.USTAR_FORMAT) as tf:
            for arcname, data, executable in sorted(files, key=lambda x: x[0]):
                add_ustar_file(tf, arcname, data, executable)
    dest.write_bytes(buf.getvalue())


def build_deb(
    out_dir: Path,
    release_id: str,
    arch: str,
    bin_dir: Path,
    ui_dir: Path,
) -> Path | None:
    dpkg = shutil.which("dpkg-deb")
    if not dpkg:
        return None
    deb_arch = {"x86_64": "amd64", "aarch64": "arm64"}.get(arch)
    if not deb_arch:
        return None
    deb_version = re.sub(r"[^0-9A-Za-z.+~-]", "-", release_id).lstrip("v")
    if not deb_version or not deb_version[0].isdigit():
        deb_version = f"0.0.0+{deb_version or 'dev'}"
    deb_path = out_dir / f"agentic-org_{deb_version}_{deb_arch}.deb"
    with tempfile.TemporaryDirectory(prefix="agentic-org-deb-") as td:
        root = Path(td) / "pkg"
        opt_dir = root / "opt" / "agentic-org" / "current"
        bin_link_dir = root / "usr" / "local" / "bin"
        ctrl_dir = root / "DEBIAN"
        opt_dir.mkdir(parents=True)
        bin_link_dir.mkdir(parents=True)
        ctrl_dir.mkdir(parents=True)
        for exe in EXECUTABLES:
            dst = opt_dir / exe
            shutil.copy2(bin_dir / exe, dst)
            os.chmod(dst, 0o755)
        for rel, src in collect_ui_files(ui_dir):
            dst = opt_dir / rel
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dst)
            os.chmod(dst, 0o644)
        for link_bin in ("agentic-org", "agent-os"):
            os.symlink(f"/opt/agentic-org/current/{link_bin}", bin_link_dir / link_bin)
        (ctrl_dir / "control").write_text(
            f"Package: agentic-org\n"
            f"Version: {deb_version}\n"
            f"Section: utils\n"
            f"Priority: optional\n"
            f"Architecture: {deb_arch}\n"
            f"Maintainer: Agentic Org Contributors <security@agentic-org.dev>\n"
            f"Description: Agentic Org host, office worker, web service, and agent-os CLI\n",
            encoding="utf-8",
        )
        subprocess.run([dpkg, "--build", "--root-owner-group", str(root), str(deb_path)], check=True)
    return deb_path


def collect_payloads(bin_dir: Path, ui_dir: Path) -> list[tuple[str, Path, bool]]:
    payloads: list[tuple[str, Path, bool]] = []
    for exe in EXECUTABLES:
        exe_path = bin_dir / exe
        if not exe_path.is_file():
            raise SystemExit(f"missing required executable: {exe_path}")
        payloads.append((exe, exe_path, True))
    for rel, path in collect_ui_files(ui_dir):
        payloads.append((rel, path, False))
    return payloads


def payload_manifest_entry(
    rel: str, path: Path, is_exec: bool, platform: str, signed: bool
) -> tuple[tuple[str, bytes, bool], dict[str, object]]:
    data = path.read_bytes()
    cdhash = None
    if is_exec and platform == "macos" and signed:
        cdhash = macos_cdhash_for(path)
        if not cdhash:
            raise SystemExit(f"signed macOS release requires codesign CDHash on {path}")
    entry = {
        "path": rel,
        "size": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
        "role": "executable" if is_exec else "data",
        "macos_cdhash": cdhash,
    }
    return (rel, data, is_exec), entry


def build_manifest_entries(
    payloads: list[tuple[str, Path, bool]], platform: str, signed: bool
) -> tuple[list[tuple[str, bytes, bool]], list[dict[str, object]]]:
    tar_entries: list[tuple[str, bytes, bool]] = []
    manifest_files: list[dict[str, object]] = []
    for rel, path, is_exec in payloads:
        tar_item, meta = payload_manifest_entry(rel, path, is_exec, platform, signed)
        tar_entries.append(tar_item)
        manifest_files.append(meta)
    return tar_entries, manifest_files


def signed_metadata_entries(
    args: argparse.Namespace, manifest_files: list[dict[str, object]]
) -> list[tuple[str, bytes, bool]]:
    if not args.signing_key:
        sums = "".join(
            f"{f['sha256']}  {f['path']}\n" for f in manifest_files
        ).encode("utf-8")
        return [("SHA256SUMS", sums, False)]
    key_path = Path(args.signing_key).resolve()
    key_id = hashlib.sha256(raw_ed25519_pubkey_from_key(key_path)).hexdigest()
    manifest = {
        "format": RELEASE_FORMAT,
        "release_id": args.release_id,
        "sequence": args.sequence,
        "platform": args.platform,
        "architecture": args.arch,
        "installer_contract": 1,
        "key_id": key_id,
        "files": manifest_files,
    }
    manifest_bytes = (json.dumps(manifest, indent=2) + "\n").encode("utf-8")
    sig_bytes = sign_ed25519(key_path, manifest_bytes)
    return [("release.v1.json", manifest_bytes, False), ("release.v1.sig", sig_bytes, False)]


def maybe_build_deb(
    args: argparse.Namespace, out_dir: Path, bin_dir: Path, ui_dir: Path
) -> Path | None:
    if not (args.deb and args.platform == "linux"):
        return None
    deb_path = build_deb(out_dir, args.release_id, args.arch, bin_dir, ui_dir)
    if deb_path:
        deb_sha = sha256_file(deb_path)
        (out_dir / f"{deb_path.name}.sha256").write_text(
            f"{deb_sha}  {deb_path.name}\n", encoding="utf-8"
        )
    return deb_path


def validate_manifest_bounds(
    release_id: str, sequence: int, manifest_files: list[dict[str, object]]
) -> None:
    if not RELEASE_ID_RE.match(release_id):
        raise SystemExit(
            f"invalid --release-id `{release_id}`: must match {RELEASE_ID_RE.pattern}"
        )
    if sequence < 1:
        raise SystemExit(f"invalid --sequence {sequence}: must be >= 1")
    if len(manifest_files) > MAX_FILES:
        raise SystemExit(f"file count {len(manifest_files)} exceeds limit {MAX_FILES}")
    total_payload = 0
    ui_bytes = 0
    ui_dirs: set[str] = set()
    ui_files = 0
    for entry in manifest_files:
        path = str(entry["path"])
        size = int(entry["size"])
        role = str(entry["role"])
        total_payload += size
        if role == "executable":
            if size > MAX_EXECUTABLE_BYTES:
                raise SystemExit(f"executable {path} exceeds {MAX_EXECUTABLE_BYTES} bytes")
        else:
            if size > MAX_UI_ASSET_BYTES:
                raise SystemExit(f"UI asset {path} exceeds {MAX_UI_ASSET_BYTES} bytes")
            ui_bytes += size
            ui_files += 1
            parts = path.split("/")
            for depth in range(2, len(parts)):
                ui_dirs.add("/".join(parts[:depth]))
    if total_payload > MAX_PAYLOAD_BYTES:
        raise SystemExit(f"total payload {total_payload} exceeds {MAX_PAYLOAD_BYTES} bytes")
    if ui_bytes > MAX_UI_BYTES:
        raise SystemExit(f"aggregate UI bytes {ui_bytes} exceeds {MAX_UI_BYTES} bytes")
    if ui_files + len(ui_dirs) > MAX_UI_ENTRIES:
        raise SystemExit(
            f"UI entries ({ui_files + len(ui_dirs)}) exceeds limit {MAX_UI_ENTRIES}"
        )


def cmd_package(args: argparse.Namespace) -> int:
    if args.deb and args.signing_key:
        raise SystemExit(
            "--deb is only supported for unsigned dev bundles; signed Linux releases "
            "must install via `agentic-org release install` / `scripts/install.sh`."
        )
    bin_dir = Path(args.bin_dir).resolve()
    ui_dir = Path(args.ui_dir).resolve()
    out_dir = Path(args.out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    payloads = collect_payloads(bin_dir, ui_dir)
    tar_entries, manifest_files = build_manifest_entries(
        payloads, args.platform, bool(args.signing_key)
    )
    validate_manifest_bounds(args.release_id, args.sequence, manifest_files)
    tar_entries.extend(signed_metadata_entries(args, manifest_files))

    suffix = "" if args.signing_key else "-dev"
    archive_name = (
        f"agentic-org-{args.release_id}-{args.platform}-{args.arch}{suffix}.tar.gz"
    )
    archive_path = out_dir / archive_name
    write_deterministic_ustar_gz(archive_path, tar_entries)
    archive_sha = sha256_file(archive_path)
    sha_path = out_dir / f"{archive_name}.sha256"
    sha_path.write_text(f"{archive_sha}  {archive_name}\n", encoding="utf-8")

    deb_path = maybe_build_deb(args, out_dir, bin_dir, ui_dir)
    result = {
        "archive": str(archive_path),
        "sha256": archive_sha,
        "sha256_file": str(sha_path),
        "signed": bool(args.signing_key),
        "release_id": args.release_id,
        "sequence": args.sequence,
        "platform": args.platform,
        "arch": args.arch,
        "file_count": len(manifest_files),
        "deb": str(deb_path) if deb_path else None,
    }
    print(json.dumps(result, indent=2))
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="Agentic Org release signing & packaging tool")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_key = sub.add_parser("keygen", help="Generate Ed25519 publisher key and trust registry")
    p_key.add_argument("--out-dir", required=True)
    p_key.add_argument("--min-sequence", type=int, default=1)
    p_key.add_argument("--max-sequence", type=int, default=1_000_000)
    p_key.add_argument("--force", action="store_true")
    p_key.set_defaults(func=cmd_keygen)

    p_pkg = sub.add_parser("package", help="Build deterministic release tarball (and optional dev .deb)")
    p_pkg.add_argument("--bin-dir", required=True)
    p_pkg.add_argument("--ui-dir", required=True)
    p_pkg.add_argument("--release-id", required=True)
    p_pkg.add_argument("--sequence", type=int, default=1)
    p_pkg.add_argument("--platform", choices=("linux", "macos"), required=True)
    p_pkg.add_argument("--arch", choices=("x86_64", "aarch64"), required=True)
    p_pkg.add_argument("--out-dir", required=True)
    p_pkg.add_argument("--signing-key", default="")
    p_pkg.add_argument("--deb", action="store_true")
    p_pkg.set_defaults(func=cmd_package)

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
