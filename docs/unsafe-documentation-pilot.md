# Unsafe documentation pilot inventory

> **Status:** Resolved. All 90 snapshot findings below (plus workspace `allow_attributes_without_reason` diagnostics and 6 additional redundant `unsafe` sites) were audited: unnecessary `unsafe` blocks were replaced with safe Rust or existing safe crate helpers (`std::process::id()`, `org_socket::effective_uid()`, `org_socket::rights::set_buffer_sizes`, `org_proto::zero_bytes`, struct literal initialization for `libc::proc_fdinfo`), scattered `mem::zeroed()` / `kSec*` constant reads were consolidated, newly unsafe-free crates were locked with `#![forbid(unsafe_code)]` (`org-office-launch`, `org-client`, `agentic-org` bin) and `#![deny(unsafe_code)]` (`agentic-org` lib), `crates/org-platform/tests/rust_hygiene.rs` and `.agent-kit/runtime/kit_rust_hygiene.py` were wired into workspace tests and `kit_hooks.py check`, and all remaining genuinely required FFI `unsafe` blocks carry direct `// SAFETY:` invariants.

The expanded Rust quality pilot reported 90 distinct `undocumented_unsafe_blocks`
findings at commit `33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6` across macOS and Linux ARM64. The table
deduplicates sites reported by multiple targets or platforms. Its links point to
that snapshot. To collect current findings during cleanup, run Clippy on each
supported platform in warning mode so findings in a dependency do not interrupt
analysis of dependent workspace crates.

```sh
cargo clippy --locked --workspace --all-targets --message-format=short
```

The required CI check promotes these warnings to errors.

Each finding asks for review of the safety documentation. Review the unsafe
operation and its preconditions, then explain the invariant in a `// SAFETY:`
comment or record a narrowly scoped, reasoned exception for a diagnostic that does
not apply. See the [development guide](../DEVELOPMENT.md#lint-and-formatting)
for the workspace Clippy command.

| Source | Reported on | Diagnostic |
| --- | --- | --- |
| [`apps/agentic-org/src/serve.rs:429:13`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/apps/agentic-org/src/serve.rs#L429) | macOS and Linux ARM64 | Unsafe block |
| [`apps/agentic-org/src/serve.rs:632:13`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/apps/agentic-org/src/serve.rs#L632) | macOS and Linux ARM64 | Unsafe block |
| [`apps/agentic-org/src/setup.rs:381:13`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/apps/agentic-org/src/setup.rs#L381) | macOS and Linux ARM64 | Unsafe block |
| [`apps/agentic-org/tests/binary.rs:296:28`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/apps/agentic-org/tests/binary.rs#L296) | macOS and Linux ARM64 | Unsafe block |
| [`apps/agentic-org/tests/lnx1.rs:430:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/apps/agentic-org/tests/lnx1.rs#L430) | macOS and Linux ARM64 | Unsafe block |
| [`apps/agentic-org/tests/serve_stop.rs:184:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/apps/agentic-org/tests/serve_stop.rs#L184) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-client/src/lifetime_tests.rs:23:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-client/src/lifetime_tests.rs#L23) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-host/src/host_log.rs:204:13`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-host/src/host_log.rs#L204) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-host/src/human/terminal.rs:14:23`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-host/src/human/terminal.rs#L14) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-host/src/human/terminal.rs:40:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-host/src/human/terminal.rs#L40) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-host/src/release/files.rs:201:23`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-host/src/release/files.rs#L201) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-host/src/release/files/output.rs:238:17`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-host/src/release/files/output.rs#L238) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-host/src/release/files/output.rs:256:24`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-host/src/release/files/output.rs#L256) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-host/src/server_check.rs:1241:21`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-host/src/server_check.rs#L1241) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-host/src/server_check.rs:1399:21`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-host/src/server_check.rs#L1399) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-host/src/setup_flow.rs:1898:8`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-host/src/setup_flow.rs#L1898) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-host/src/setup_flow/tests.rs:941:14`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-host/src/setup_flow/tests.rs#L941) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-host/src/setup_flow/tests.rs:942:14`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-host/src/setup_flow/tests.rs#L942) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-host/src/setup_flow/tests.rs:2254:18`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-host/src/setup_flow/tests.rs#L2254) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-host/src/supervisor/mod.rs:2139:13`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-host/src/supervisor/mod.rs#L2139) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-host/src/supervisor/mod.rs:2218:13`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-host/src/supervisor/mod.rs#L2218) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-host/src/supervisor/mod.rs:2265:17`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-host/src/supervisor/mod.rs#L2265) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-office-server/src/startup/tests.rs:1154:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-office-server/src/startup/tests.rs#L1154) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-office-server/src/startup/tests.rs:1193:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-office-server/src/startup/tests.rs#L1193) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-office-server/src/startup/tests.rs:1240:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-office-server/src/startup/tests.rs#L1240) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-office-server/src/tests/transfer_composition.rs:96:22`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-office-server/src/tests/transfer_composition.rs#L96) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-office-server/src/tests/transfer_composition.rs:99:21`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-office-server/src/tests/transfer_composition.rs#L99) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-office-server/src/transfer/channels/tests.rs:585:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-office-server/src/transfer/channels/tests.rs#L585) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-office-server/src/transfer/channels/tests.rs:638:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-office-server/src/transfer/channels/tests.rs#L638) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-platform-host/src/admission/linux/tests.rs:903:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/admission/linux/tests.rs#L903) | Linux ARM64 | Unsafe block |
| [`crates/org-platform-host/src/admission/linux/tests.rs:1457:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/admission/linux/tests.rs#L1457) | Linux ARM64 | Unsafe block |
| [`crates/org-platform-host/src/admission/macos/code.rs:139:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/admission/macos/code.rs#L139) | macOS | Unsafe block |
| [`crates/org-platform-host/src/admission/macos/code.rs:190:29`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/admission/macos/code.rs#L190) | macOS | Unsafe block |
| [`crates/org-platform-host/src/admission/macos/tests.rs:80:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/admission/macos/tests.rs#L80) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/linux/release.rs:764:21`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/linux/release.rs#L764) | Linux ARM64 | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:122:36`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L122) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:123:31`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L123) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:127:35`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L127) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:130:42`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L130) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:132:35`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L132) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:136:28`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L136) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:143:41`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L143) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:144:36`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L144) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:148:28`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L148) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:159:31`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L159) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:160:31`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L160) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:164:30`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L164) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:167:37`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L167) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:168:37`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L168) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:170:23`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L170) | macOS | Unsafe block |
| [`crates/org-platform-host/src/credentials/macos/tests.rs:349:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform-host/src/credentials/macos/tests.rs#L349) | macOS | Unsafe block |
| [`crates/org-platform/src/confinement/macos.rs:99:43`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform/src/confinement/macos.rs#L99) | macOS | Unsafe block |
| [`crates/org-platform/src/image/macos.rs:103:13`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform/src/image/macos.rs#L103) | macOS | Unsafe block |
| [`crates/org-platform/src/process/adapter_tests.rs:304:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform/src/process/adapter_tests.rs#L304) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-platform/src/process/linux/scan.rs:1261:26`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform/src/process/linux/scan.rs#L1261) | Linux ARM64 | Unsafe block |
| [`crates/org-platform/src/process/linux/scan.rs:1262:26`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform/src/process/linux/scan.rs#L1262) | Linux ARM64 | Unsafe block |
| [`crates/org-platform/src/process/linux/scan.rs:1375:13`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform/src/process/linux/scan.rs#L1375) | Linux ARM64 | Unsafe block |
| [`crates/org-platform/src/process/mod.rs:541:51`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform/src/process/mod.rs#L541) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-platform/src/service/linux.rs:856:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform/src/service/linux.rs#L856) | Linux ARM64 | Unsafe block |
| [`crates/org-platform/src/service/linux.rs:857:18`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform/src/service/linux.rs#L857) | Linux ARM64 | Unsafe block |
| [`crates/org-platform/src/service/linux.rs:877:17`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform/src/service/linux.rs#L877) | Linux ARM64 | Unsafe block |
| [`crates/org-platform/src/service/linux.rs:879:17`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform/src/service/linux.rs#L879) | Linux ARM64 | Unsafe block |
| [`crates/org-platform/src/service/linux.rs:894:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform/src/service/linux.rs#L894) | Linux ARM64 | Unsafe block |
| [`crates/org-platform/src/socket/adapter_tests.rs:130:13`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform/src/socket/adapter_tests.rs#L130) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-platform/src/socket/macos/tests.rs:124:17`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-platform/src/socket/macos/tests.rs#L124) | macOS | Unsafe block |
| [`crates/org-socket/src/rights.rs:286:20`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-socket/src/rights.rs#L286) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-socket/src/rights.rs:292:10`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-socket/src/rights.rs#L292) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-socket/src/rights.rs:320:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-socket/src/rights.rs#L320) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-socket/src/rights.rs:321:22`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-socket/src/rights.rs#L321) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-socket/src/rights/send/tests.rs:538:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-socket/src/rights/send/tests.rs#L538) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-socket/src/rights/stream/tests.rs:239:17`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-socket/src/rights/stream/tests.rs#L239) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-socket/src/rights/stream/tests.rs:248:17`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-socket/src/rights/stream/tests.rs#L248) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-socket/src/rights/stream/tests/typed.rs:258:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-socket/src/rights/stream/tests/typed.rs#L258) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-socket/src/rights/tests.rs:108:20`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-socket/src/rights/tests.rs#L108) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-socket/src/rights/tests.rs:147:18`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-socket/src/rights/tests.rs#L147) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-socket/tests/confinement_example.rs:59:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-socket/tests/confinement_example.rs#L59) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-supervise/src/cleanup.rs:79:29`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-supervise/src/cleanup.rs#L79) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-supervise/src/files.rs:128:27`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-supervise/src/files.rs#L128) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-supervise/tests/lifecycle.rs:317:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-supervise/tests/lifecycle.rs#L317) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-supervise/tests/lifecycle.rs:487:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-supervise/tests/lifecycle.rs#L487) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-supervise/tests/recovery_native.rs:287:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-supervise/tests/recovery_native.rs#L287) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-supervise/tests/recovery_native.rs:290:21`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-supervise/tests/recovery_native.rs#L290) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-supervise/tests/recovery_native.rs:573:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-supervise/tests/recovery_native.rs#L573) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-supervise/tests/recovery_native.rs:634:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-supervise/tests/recovery_native.rs#L634) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-supervise/tests/recovery_native.rs:637:26`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-supervise/tests/recovery_native.rs#L637) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-supervise/tests/recovery_native.rs:641:13`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-supervise/tests/recovery_native.rs#L641) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-supervise/tests/spawn.rs:89:17`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-supervise/tests/spawn.rs#L89) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-supervise/tests/spawn.rs:426:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-supervise/tests/spawn.rs#L426) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-supervise/tests/spawn.rs:479:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-supervise/tests/spawn.rs#L479) | macOS and Linux ARM64 | Unsafe block |
| [`crates/org-supervise/tests/spawn.rs:501:9`](https://github.com/vgmello/agentic-org/blob/33c7e5c76ccf2ccff449fe3b3e66d2ff933441d6/crates/org-supervise/tests/spawn.rs#L501) | macOS and Linux ARM64 | Unsafe block |
