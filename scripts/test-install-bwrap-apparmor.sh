#!/usr/bin/env bash
# Deterministic self-test for install.sh's minimal scoped bwrap AppArmor profile
# generation, staging install, conflict handling, idempotency, symlink guards,
# and unprivileged guidance — runs cleanly without sudo or mutating /etc.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd -P)"
repo_root="$(cd "${here}/.." && pwd -P)"
install_sh="${here}/install.sh"
template_profile="${repo_root}/packaging/apparmor/agentic-org-bwrap"

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/test-install-bwrap-apparmor.XXXXXX")"
chmod 0700 "$tmp_root"
trap 'rm -rf "$tmp_root"' EXIT

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

assert_contains() {
  local haystack="$1" needle="$2" label="$3"
  [[ "$haystack" == *"$needle"* ]] || fail "${label}: expected to contain '${needle}', got:\n${haystack}"
}

assert_not_contains() {
  local haystack="$1" needle="$2" label="$3"
  [[ "$haystack" != *"$needle"* ]] || fail "${label}: must NOT contain '${needle}', got:\n${haystack}"
}

assert_never_disables_sysctl_globally() {
  local text="$1" label="$2"
  if printf '%s\n' "$text" | grep -Eq 'apparmor_restrict_unprivileged_userns[[:space:]]*=[[:space:]]*0'; then
    fail "${label}: output must NEVER advise setting apparmor_restrict_unprivileged_userns = 0:\n${text}"
  fi
}

file_mode() {
  local path="$1"
  if stat -c '%a' "$path" >/dev/null 2>&1; then
    stat -c '%a' "$path"
  else
    stat -f '%Lp' "$path"
  fi
}

# 1. --print-bwrap-apparmor matches packaging/apparmor/agentic-org-bwrap byte-for-byte.
[[ -f "$template_profile" ]] || fail "missing canonical profile template: ${template_profile}"
printed_file="${tmp_root}/printed-profile"
bash "$install_sh" --print-bwrap-apparmor > "$printed_file"
cmp -s "$printed_file" "$template_profile" \
  || fail "--print-bwrap-apparmor drifted from packaging/apparmor/agentic-org-bwrap"

printed_text="$(cat "$printed_file")"
assert_contains "$printed_text" "abi <abi/4.0>," "profile abi"
assert_contains "$printed_text" "include <tunables/global>" "profile tunables"
assert_contains "$printed_text" "profile agentic-org-bwrap /{usr/,}bin/bwrap flags=(unconfined) {" "profile header"
assert_contains "$printed_text" "userns," "profile userns rule"
assert_contains "$printed_text" "include if exists <local/agentic-org-bwrap>" "profile local include"
assert_never_disables_sysctl_globally "$printed_text" "printed profile"

# 2. If apparmor_parser is installed, syntax-check the profile without touching /etc or kernel.
if command -v apparmor_parser >/dev/null 2>&1; then
  apparmor_parser --skip-kernel-load --skip-cache --warn=all "$printed_file" \
    || fail "apparmor_parser rejected canonical bwrap profile"
fi

# 3. Conflicting and extraneous CLI flags fail cleanly with exit 2.
rc=0
bash "$install_sh" --print-bwrap-apparmor --install-bwrap-apparmor >/dev/null 2>&1 || rc=$?
[[ "$rc" -eq 2 ]] || fail "expected exit 2 for conflicting flags, got ${rc}"

rc=0
bash "$install_sh" --print-bwrap-apparmor --bin-dir "${tmp_root}/bin" >/dev/null 2>&1 || rc=$?
[[ "$rc" -eq 2 ]] || fail "expected exit 2 for --print-bwrap-apparmor --bin-dir, got ${rc}"

rc=0
bash "$install_sh" --install-bwrap-apparmor --no-restart >/dev/null 2>&1 || rc=$?
[[ "$rc" -eq 2 ]] || fail "expected exit 2 for standalone --install-bwrap-apparmor --no-restart, got ${rc}"

rc=0
bash "$install_sh" --install-bwrap-apparmor --bin-dir "${tmp_root}/bin" >/dev/null 2>&1 || rc=$?
[[ "$rc" -eq 2 ]] || fail "expected exit 2 for standalone --install-bwrap-apparmor --bin-dir, got ${rc}"

rc=0
bash "$install_sh" --install-bwrap-apparmor --store "${tmp_root}/store" >/dev/null 2>&1 || rc=$?
[[ "$rc" -eq 2 ]] || fail "expected exit 2 for standalone --install-bwrap-apparmor --store, got ${rc}"

# Following tests exercise Linux staging paths (skip cleanly on non-Linux hosts).
if [[ "$(uname -s)" != "Linux" ]]; then
  echo "ok (non-Linux host: skipped Linux staging install checks)"
  exit 0
fi

# 4. Standalone --install-bwrap-apparmor into staged directory: atomic write, 0644, no temp leftovers.
stage_dir="${tmp_root}/apparmor-stage"
mkdir -p "$stage_dir"
out="$(
  AGENTIC_ORG_APPARMOR_DIR="$stage_dir" \
  AGENTIC_ORG_SKIP_APPARMOR_PARSER=1 \
  bash "$install_sh" --install-bwrap-apparmor 2>&1
)"
assert_contains "$out" "Installed scoped bwrap AppArmor profile:" "first install output"
assert_never_disables_sysctl_globally "$out" "first install output"
[[ -f "${stage_dir}/agentic-org-bwrap" && ! -L "${stage_dir}/agentic-org-bwrap" ]] \
  || fail "installed profile missing or symlink"
cmp -s "${stage_dir}/agentic-org-bwrap" "$template_profile" \
  || fail "installed profile content differs from canonical template"
mode="$(file_mode "${stage_dir}/agentic-org-bwrap")"
[[ "$mode" == "644" ]] || fail "expected mode 644, got ${mode}"
leftovers="$(find "$stage_dir" -maxdepth 1 -name '.agentic-org-bwrap.*')"
[[ -z "$leftovers" ]] || fail "leftover temp files in staging dir: ${leftovers}"

# 5. Idempotency: second run reports already up to date and preserves content & mode.
out2="$(
  AGENTIC_ORG_APPARMOR_DIR="$stage_dir" \
  AGENTIC_ORG_SKIP_APPARMOR_PARSER=1 \
  bash "$install_sh" --install-bwrap-apparmor 2>&1
)"
assert_contains "$out2" "already up to date:" "idempotent second install"
cmp -s "${stage_dir}/agentic-org-bwrap" "$template_profile" \
  || fail "idempotent run modified profile content"
[[ "$(file_mode "${stage_dir}/agentic-org-bwrap")" == "644" ]] \
  || fail "idempotent run altered mode"

# 6. Conflicting Ubuntu bwrap-userns-restrict is disabled via disable/ symlink and prior own disable symlink is cleared.
conflict_dir="${tmp_root}/apparmor-conflict"
mkdir -p "${conflict_dir}/disable"
cat > "${conflict_dir}/bwrap-userns-restrict" <<'EOF'
abi <abi/4.0>,
profile bwrap /usr/bin/bwrap {
  allow userns,
}
profile unpriv_bwrap {
  audit deny capability,
}
EOF
ln -s "${conflict_dir}/agentic-org-bwrap" "${conflict_dir}/disable/agentic-org-bwrap"
out_conflict="$(
  AGENTIC_ORG_APPARMOR_DIR="$conflict_dir" \
  AGENTIC_ORG_SKIP_APPARMOR_PARSER=1 \
  bash "$install_sh" --install-bwrap-apparmor 2>&1
)"
assert_contains "$out_conflict" "Disabled conflicting capability-stripping profile:" "conflict disable message"
[[ -L "${conflict_dir}/disable/bwrap-userns-restrict" ]] \
  || fail "expected disable/bwrap-userns-restrict symlink"
[[ "$(readlink "${conflict_dir}/disable/bwrap-userns-restrict")" == "${conflict_dir}/bwrap-userns-restrict" ]] \
  || fail "disable/bwrap-userns-restrict points to unexpected target"
[[ ! -L "${conflict_dir}/disable/agentic-org-bwrap" ]] \
  || fail "stale disable/agentic-org-bwrap symlink should be removed"
cmp -s "${conflict_dir}/agentic-org-bwrap" "$template_profile" \
  || fail "conflict install did not write canonical profile"

# Re-running with bwrap-userns-restrict already disabled stays idempotent.
out_conflict2="$(
  AGENTIC_ORG_APPARMOR_DIR="$conflict_dir" \
  AGENTIC_ORG_SKIP_APPARMOR_PARSER=1 \
  bash "$install_sh" --install-bwrap-apparmor 2>&1
)"
assert_contains "$out_conflict2" "already up to date:" "conflict second run idempotent"
assert_not_contains "$out_conflict2" "Disabled conflicting" "conflict second run does not re-disable"

# 7. Symlink safety: symlinked AppArmor dir and symlinked target profile fail closed.
real_dir="${tmp_root}/real-aa"
mkdir -p "$real_dir"
sym_dir="${tmp_root}/sym-aa"
ln -s "$real_dir" "$sym_dir"
rc=0
AGENTIC_ORG_APPARMOR_DIR="$sym_dir" \
AGENTIC_ORG_SKIP_APPARMOR_PARSER=1 \
bash "$install_sh" --install-bwrap-apparmor >/dev/null 2>&1 || rc=$?
[[ "$rc" -ne 0 ]] || fail "expected failure for symlinked AppArmor dir"
[[ ! -e "${real_dir}/agentic-org-bwrap" ]] || fail "symlinked dir must not be written"

victim="${tmp_root}/victim.txt"
printf 'keep-me\n' > "$victim"
ln -s "$victim" "${real_dir}/agentic-org-bwrap"
rc=0
AGENTIC_ORG_APPARMOR_DIR="$real_dir" \
AGENTIC_ORG_SKIP_APPARMOR_PARSER=1 \
bash "$install_sh" --install-bwrap-apparmor >/dev/null 2>&1 || rc=$?
[[ "$rc" -ne 0 ]] || fail "expected failure for symlinked target profile"
[[ "$(cat "$victim")" == "keep-me" ]] || fail "symlinked target was overwritten"

# 8. Unprivileged standalone --install-bwrap-apparmor exits non-zero with scoped sudo command.
unpriv_dir="${tmp_root}/unpriv-aa"
mkdir -p "$unpriv_dir"
rc=0
unpriv_out="$(
  AGENTIC_ORG_APPARMOR_DIR="$unpriv_dir" \
  AGENTIC_ORG_SIMULATE_UNPRIVILEGED=1 \
  bash "$install_sh" --install-bwrap-apparmor 2>&1
)" || rc=$?
[[ "$rc" -ne 0 ]] || fail "expected non-zero exit for unprivileged --install-bwrap-apparmor"
assert_contains "$unpriv_out" "sudo bash scripts/install.sh --install-bwrap-apparmor" "unpriv sudo hint"
assert_contains "$unpriv_out" "--print-bwrap-apparmor" "unpriv print hint"
assert_never_disables_sysctl_globally "$unpriv_out" "unpriv standalone output"

# 9. End-to-end release install hook (using an unsigned dummy archive in isolated temp HOME).
stub_src="${tmp_root}/stub-bins"
mkdir -p "$stub_src"
for bin in agentic-org agentic-org-office agentic-org-web agent-os; do
  cat > "${stub_src}/${bin}" <<EOF
#!/usr/bin/env bash
echo "${bin} 0.0.0-test"
EOF
  chmod 0755 "${stub_src}/${bin}"
done
dummy_archive="${tmp_root}/agentic-org-0.0.0-test.tar.gz"
tar -czf "$dummy_archive" -C "$stub_src" agentic-org agentic-org-office agentic-org-web agent-os

fake_bwrap="${tmp_root}/fake-bwrap"
printf '#!/usr/bin/env bash\nexit 0\n' > "$fake_bwrap"
chmod 0755 "$fake_bwrap"

sysctl_one="${tmp_root}/sysctl-1"
printf '1\n' > "$sysctl_one"
sysctl_zero="${tmp_root}/sysctl-0"
printf '0\n' > "$sysctl_zero"

fake_home="${tmp_root}/fake-home"
fake_bin="${fake_home}/.local/bin"
mkdir -p "$fake_bin"

# 9a. Unprivileged release install with sysctl=1 and bwrap present succeeds and prints scoped sudo hint.
auto_unpriv_aa="${tmp_root}/auto-unpriv-aa"
mkdir -p "$auto_unpriv_aa"
out_9a="$(
  HOME="$fake_home" \
  AGENTIC_ORG_APPARMOR_DIR="$auto_unpriv_aa" \
  AGENTIC_ORG_USERNS_SYSCTL="$sysctl_one" \
  AGENTIC_ORG_BWRAP_BIN="$fake_bwrap" \
  AGENTIC_ORG_SIMULATE_UNPRIVILEGED=1 \
  bash "$install_sh" --archive "$dummy_archive" --allow-unsigned --bin-dir "$fake_bin" 2>&1
)"
assert_contains "$out_9a" "agentic-org 0.0.0-test" "9a release installed"
assert_contains "$out_9a" "sudo bash scripts/install.sh --install-bwrap-apparmor" "9a prints scoped sudo command"
assert_contains "$out_9a" "--print-bwrap-apparmor" "9a prints --print-bwrap-apparmor command"
assert_never_disables_sysctl_globally "$out_9a" "9a unpriv release install"

# 9b. Privileged/writable release install with sysctl=1 and bwrap present installs profile automatically.
auto_priv_aa="${tmp_root}/auto-priv-aa"
mkdir -p "$auto_priv_aa"
out_9b="$(
  HOME="$fake_home" \
  AGENTIC_ORG_APPARMOR_DIR="$auto_priv_aa" \
  AGENTIC_ORG_USERNS_SYSCTL="$sysctl_one" \
  AGENTIC_ORG_BWRAP_BIN="$fake_bwrap" \
  AGENTIC_ORG_SKIP_APPARMOR_PARSER=1 \
  bash "$install_sh" --archive "$dummy_archive" --allow-unsigned --bin-dir "$fake_bin" 2>&1
)"
assert_contains "$out_9b" "Installed scoped bwrap AppArmor profile:" "9b auto-installed profile"
cmp -s "${auto_priv_aa}/agentic-org-bwrap" "$template_profile" \
  || fail "9b auto-installed profile differs from canonical template"

# 9c. Release install with active unconfined+userns /etc/apparmor.d/bwrap stays quiet.
auto_existing_aa="${tmp_root}/auto-existing-aa"
mkdir -p "$auto_existing_aa"
cat > "${auto_existing_aa}/bwrap" <<'EOF'
abi <abi/4.0>,
include <tunables/global>
profile bwrap /usr/bin/bwrap flags=(unconfined) {
  userns,
}
EOF
out_9c="$(
  HOME="$fake_home" \
  AGENTIC_ORG_APPARMOR_DIR="$auto_existing_aa" \
  AGENTIC_ORG_USERNS_SYSCTL="$sysctl_one" \
  AGENTIC_ORG_BWRAP_BIN="$fake_bwrap" \
  AGENTIC_ORG_SIMULATE_UNPRIVILEGED=1 \
  bash "$install_sh" --archive "$dummy_archive" --allow-unsigned --bin-dir "$fake_bin" 2>&1
)"
assert_not_contains "$out_9c" "--install-bwrap-apparmor" "9c already-active bwrap stays quiet"

# 9d. Release install with bwrap-userns-restrict only STILL prints guidance mentioning bwrap-userns-restrict.
auto_restrict_aa="${tmp_root}/auto-restrict-aa"
mkdir -p "$auto_restrict_aa"
cp "${conflict_dir}/bwrap-userns-restrict" "${auto_restrict_aa}/bwrap-userns-restrict"
out_9d="$(
  HOME="$fake_home" \
  AGENTIC_ORG_APPARMOR_DIR="$auto_restrict_aa" \
  AGENTIC_ORG_USERNS_SYSCTL="$sysctl_one" \
  AGENTIC_ORG_BWRAP_BIN="$fake_bwrap" \
  AGENTIC_ORG_SIMULATE_UNPRIVILEGED=1 \
  bash "$install_sh" --archive "$dummy_archive" --allow-unsigned --bin-dir "$fake_bin" 2>&1
)"
assert_contains "$out_9d" "--install-bwrap-apparmor" "9d warns on bwrap-userns-restrict only"
assert_contains "$out_9d" "disable/bwrap-userns-restrict" "9d includes disable step for bwrap-userns-restrict"
assert_never_disables_sysctl_globally "$out_9d" "9d restrict-only output"

# 9e. Release install with sysctl=0 stays quiet.
out_9e="$(
  HOME="$fake_home" \
  AGENTIC_ORG_APPARMOR_DIR="$auto_unpriv_aa" \
  AGENTIC_ORG_USERNS_SYSCTL="$sysctl_zero" \
  AGENTIC_ORG_BWRAP_BIN="$fake_bwrap" \
  AGENTIC_ORG_SIMULATE_UNPRIVILEGED=1 \
  bash "$install_sh" --archive "$dummy_archive" --allow-unsigned --bin-dir "$fake_bin" 2>&1
)"
assert_not_contains "$out_9e" "--install-bwrap-apparmor" "9e sysctl=0 stays quiet"

# 9f. Release install with bwrap absent stays quiet.
out_9f="$(
  HOME="$fake_home" \
  AGENTIC_ORG_APPARMOR_DIR="$auto_unpriv_aa" \
  AGENTIC_ORG_USERNS_SYSCTL="$sysctl_one" \
  AGENTIC_ORG_BWRAP_BIN="none" \
  AGENTIC_ORG_SIMULATE_UNPRIVILEGED=1 \
  bash "$install_sh" --archive "$dummy_archive" --allow-unsigned --bin-dir "$fake_bin" 2>&1
)"
assert_not_contains "$out_9f" "--install-bwrap-apparmor" "9f missing bwrap stays quiet"

# 10. P1-1: Trailing-slash symlink, relative AppArmor dir, and directory-at-target all fail closed in strict=1.
real_dir_10="${tmp_root}/real-aa-10"
mkdir -p "$real_dir_10"
sym_dir_10="${tmp_root}/sym-aa-10"
ln -s "$real_dir_10" "$sym_dir_10"
rc=0
AGENTIC_ORG_APPARMOR_DIR="${sym_dir_10}/" \
AGENTIC_ORG_SKIP_APPARMOR_PARSER=1 \
bash "$install_sh" --install-bwrap-apparmor >/dev/null 2>&1 || rc=$?
[[ "$rc" -ne 0 ]] || fail "10a expected failure for trailing-slash symlinked AppArmor dir"
[[ ! -e "${real_dir_10}/agentic-org-bwrap" ]] || fail "10a trailing-slash symlinked dir must not be written"

rc=0
AGENTIC_ORG_APPARMOR_DIR="relative/apparmor.d" \
AGENTIC_ORG_SKIP_APPARMOR_PARSER=1 \
bash "$install_sh" --install-bwrap-apparmor >/dev/null 2>&1 || rc=$?
[[ "$rc" -ne 0 ]] || fail "10b expected failure for relative AppArmor dir"

dir_target_aa="${tmp_root}/dir-target-aa"
mkdir -p "${dir_target_aa}/agentic-org-bwrap"
rc=0
AGENTIC_ORG_APPARMOR_DIR="$dir_target_aa" \
AGENTIC_ORG_SKIP_APPARMOR_PARSER=1 \
bash "$install_sh" --install-bwrap-apparmor >/dev/null 2>&1 || rc=$?
[[ "$rc" -ne 0 ]] || fail "10c expected failure when target profile path is a directory"
dir_contents="$(find "${dir_target_aa}/agentic-org-bwrap" -mindepth 1)"
[[ -z "$dir_contents" ]] || fail "10c mv moved temp file into existing target directory: ${dir_contents}"

# 11. P1-2: Non-strict release install hook (strict=0) NEVER aborts on symlinked dir, dir target, or parser failure.
out_11a="$(
  HOME="$fake_home" \
  AGENTIC_ORG_APPARMOR_DIR="${sym_dir}/" \
  AGENTIC_ORG_USERNS_SYSCTL="$sysctl_one" \
  AGENTIC_ORG_BWRAP_BIN="$fake_bwrap" \
  bash "$install_sh" --archive "$dummy_archive" --allow-unsigned --bin-dir "$fake_bin" 2>&1
)"
assert_contains "$out_11a" "agentic-org 0.0.0-test" "11a release install succeeds despite symlinked AppArmor dir"
assert_contains "$out_11a" "WARNING:" "11a emits warning on symlinked AppArmor dir"
assert_contains "$out_11a" "--install-bwrap-apparmor" "11a prints manual remediation"

out_11b="$(
  HOME="$fake_home" \
  AGENTIC_ORG_APPARMOR_DIR="$dir_target_aa" \
  AGENTIC_ORG_USERNS_SYSCTL="$sysctl_one" \
  AGENTIC_ORG_BWRAP_BIN="$fake_bwrap" \
  bash "$install_sh" --archive "$dummy_archive" --allow-unsigned --bin-dir "$fake_bin" 2>&1
)"
assert_contains "$out_11b" "agentic-org 0.0.0-test" "11b release install succeeds despite directory-at-target"
assert_contains "$out_11b" "WARNING:" "11b emits warning on non-regular target"

failing_parser_dir="${tmp_root}/failing-parser-bin"
mkdir -p "$failing_parser_dir"
cat > "${failing_parser_dir}/apparmor_parser" <<'EOF'
#!/usr/bin/env bash
if [[ "${1:-}" == "-r" ]]; then
  echo "apparmor_parser: Operation not permitted" >&2
  exit 1
fi
exit 0
EOF
chmod 0755 "${failing_parser_dir}/apparmor_parser"

parser_fail_aa="${tmp_root}/parser-fail-aa"
mkdir -p "$parser_fail_aa"
out_11c="$(
  HOME="$fake_home" \
  PATH="${failing_parser_dir}:${PATH}" \
  AGENTIC_ORG_APPARMOR_DIR="$parser_fail_aa" \
  AGENTIC_ORG_USERNS_SYSCTL="$sysctl_one" \
  AGENTIC_ORG_BWRAP_BIN="$fake_bwrap" \
  bash "$install_sh" --archive "$dummy_archive" --allow-unsigned --bin-dir "$fake_bin" 2>&1
)"
assert_contains "$out_11c" "agentic-org 0.0.0-test" "11c release install succeeds when apparmor_parser -r fails in container"
assert_contains "$out_11c" "WARNING:" "11c warns on apparmor_parser -r failure"
assert_contains "$out_11c" "apparmor_parser -r" "11c prints manual reload steps"

# Explicit --install-bwrap-apparmor (strict=1) STILL fails closed when apparmor_parser -r fails.
rc=0
PATH="${failing_parser_dir}:${PATH}" \
AGENTIC_ORG_APPARMOR_DIR="$parser_fail_aa" \
bash "$install_sh" --install-bwrap-apparmor >/dev/null 2>&1 || rc=$?
[[ "$rc" -ne 0 ]] || fail "11d explicit --install-bwrap-apparmor must fail closed when apparmor_parser -r fails"

# 12. P1-3: Stale kernel bwrap-userns-restrict is unloaded via -R even if disable/ symlink already exists.
parser_log="${tmp_root}/parser.log"
stub_parser_dir="${tmp_root}/stub-parser-bin"
mkdir -p "$stub_parser_dir"
cat > "${stub_parser_dir}/apparmor_parser" <<EOF
#!/usr/bin/env bash
printf '%s\n' "\$*" >> "${parser_log}"
exit 0
EOF
chmod 0755 "${stub_parser_dir}/apparmor_parser"

pre_disabled_aa="${tmp_root}/pre-disabled-aa"
mkdir -p "${pre_disabled_aa}/disable"
cp "${conflict_dir}/bwrap-userns-restrict" "${pre_disabled_aa}/bwrap-userns-restrict"
ln -s "${pre_disabled_aa}/bwrap-userns-restrict" "${pre_disabled_aa}/disable/bwrap-userns-restrict"
PATH="${stub_parser_dir}:${PATH}" \
AGENTIC_ORG_APPARMOR_DIR="$pre_disabled_aa" \
bash "$install_sh" --install-bwrap-apparmor >/dev/null 2>&1
parser_calls="$(cat "$parser_log")"
assert_contains "$parser_calls" "-R ${pre_disabled_aa}/bwrap-userns-restrict" \
  "12 must unconditionally unload bwrap-userns-restrict even when disable/ symlink exists"
assert_contains "$parser_calls" "-r ${pre_disabled_aa}/agentic-org-bwrap" \
  "12 must load agentic-org-bwrap via apparmor_parser -r"

# 13. P1-4: Strict attachment, unconfined requirement, and shadowing /etc/apparmor.d/bwrap detection in install.sh.
wrong_att_aa="${tmp_root}/aa-wrong-att"
mkdir -p "$wrong_att_aa"
cat > "${wrong_att_aa}/agentic-org-bwrap" <<'EOF'
abi <abi/4.0>,
profile agentic-org-bwrap /usr/bin/true flags=(unconfined) {
  userns,
}
EOF
out_13a="$(
  HOME="$fake_home" \
  AGENTIC_ORG_APPARMOR_DIR="$wrong_att_aa" \
  AGENTIC_ORG_USERNS_SYSCTL="$sysctl_one" \
  AGENTIC_ORG_BWRAP_BIN="$fake_bwrap" \
  AGENTIC_ORG_SIMULATE_UNPRIVILEGED=1 \
  bash "$install_sh" --archive "$dummy_archive" --allow-unsigned --bin-dir "$fake_bin" 2>&1
)"
assert_contains "$out_13a" "--install-bwrap-apparmor" "13a wrong attachment /usr/bin/true must NOT satisfy profile check"

enforcing_scoped_aa="${tmp_root}/aa-enforcing-scoped"
mkdir -p "$enforcing_scoped_aa"
cat > "${enforcing_scoped_aa}/agentic-org-bwrap" <<'EOF'
abi <abi/4.0>,
profile agentic-org-bwrap /{usr/,}bin/bwrap {
  userns,
}
EOF
out_13b="$(
  HOME="$fake_home" \
  AGENTIC_ORG_APPARMOR_DIR="$enforcing_scoped_aa" \
  AGENTIC_ORG_USERNS_SYSCTL="$sysctl_one" \
  AGENTIC_ORG_BWRAP_BIN="$fake_bwrap" \
  AGENTIC_ORG_SIMULATE_UNPRIVILEGED=1 \
  bash "$install_sh" --archive "$dummy_archive" --allow-unsigned --bin-dir "$fake_bin" 2>&1
)"
assert_contains "$out_13b" "--install-bwrap-apparmor" "13b enforcing non-unconfined profile must NOT satisfy profile check"

shadow_aa="${tmp_root}/aa-shadow-bwrap"
mkdir -p "$shadow_aa"
cp "$template_profile" "${shadow_aa}/agentic-org-bwrap"
cat > "${shadow_aa}/bwrap" <<'EOF'
abi <abi/4.0>,
profile bwrap /usr/bin/bwrap {
  userns,
}
EOF
out_13c="$(
  HOME="$fake_home" \
  AGENTIC_ORG_APPARMOR_DIR="$shadow_aa" \
  AGENTIC_ORG_USERNS_SYSCTL="$sysctl_one" \
  AGENTIC_ORG_BWRAP_BIN="$fake_bwrap" \
  AGENTIC_ORG_SIMULATE_UNPRIVILEGED=1 \
  bash "$install_sh" --archive "$dummy_archive" --allow-unsigned --bin-dir "$fake_bin" 2>&1
)"
assert_contains "$out_13c" "--install-bwrap-apparmor" "13c shadowing active enforcing bwrap must NOT stay quiet"

# 14. P2-8: Shell-quoted paths in manual remediation steps.
spaced_aa="${tmp_root}/aa dir with space"
mkdir -p "$spaced_aa"
out_14="$(
  HOME="$fake_home" \
  AGENTIC_ORG_APPARMOR_DIR="$spaced_aa" \
  AGENTIC_ORG_USERNS_SYSCTL="$sysctl_one" \
  AGENTIC_ORG_BWRAP_BIN="$fake_bwrap" \
  AGENTIC_ORG_SIMULATE_UNPRIVILEGED=1 \
  bash "$install_sh" --archive "$dummy_archive" --allow-unsigned --bin-dir "$fake_bin" 2>&1
)"
printf -v expected_quoted_target '%q' "${spaced_aa}/agentic-org-bwrap"
assert_contains "$out_14" "sudo tee ${expected_quoted_target} >/dev/null" "14 manual steps shell-quote paths"

echo "ok (all bwrap AppArmor install checks passed)"
