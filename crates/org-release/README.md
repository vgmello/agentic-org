# org-release

Pure verification for ADR-0009's `agentic-org-release-v1` profile. The host's release
module (`crates/org-host/src/release`) uses it, and no command reaches that yet:
installing from a signed release is increment L9 of the
[implementation plan](../../.backlog/docs/doc-5%20-%20v1-Implementation-Plan.md). The library
has no filesystem, archive, environment, process, platform-admission or activation
adapter. Its six direct external dependencies are pinned existing workspace
versions: ring 0.17.14, serde 1.0.229, serde_json 1.0.154, sha2 0.10.9,
caseless 0.2.2 and unicode-normalization 0.1.26. It has no internal crate dependencies.

`TrustedKeys::from_registry_bytes` validates strict version-1 public registry
bytes, including required nullable `max_sequence`, identity hashes and inclusive
sequence intervals. Its input must come from a trusted build/package input.
Registry validity does not establish provenance. Absent or empty registries fail;
manifest, CLI or runtime environment keys must never become a fallback.

`verify_manifest` accepts the original manifest bytes and exactly 64 raw signature
bytes. It returns an immutable `VerifiedManifest`, preserving the exact signed
bytes and SHA256, original decoded UTF-8 paths, closed inventory and signed
macOS CodeDirectory hash expectations. Path collision detection uses the same
NFC/full-default-casefold/NFC predicate as host identity normalization; accepted
paths retain their original bytes. Each field is decoded directly to its wire
type without JSON Value or float-mediated integer conversion. All structural
keys, including nullable keys, must occur exactly once and no unknown keys are
accepted. Platform/architecture/role values must be JSON strings.

The nested UI execution profile exports `MAX_UI_BYTES: u64 = 64 * 1024 * 1024`
and `MAX_UI_ENTRIES: usize = 4096`. During manifest verification, checked totals
limit signed UI file bytes and regular UI files plus unique proper directory
prefixes beneath `ui/`, excluding the root. Thus `ui/index.html` and
`ui/_app/a.js` count as three loader entries. Shared prefixes count once, with
no payload bytes assigned to directories. Excess UI bytes return the existing
`PayloadSizeLimit`; excess loader entries return `InvalidInventory`. Whole-release,
manifest and per-file limits also remain enforced. Consumers must use these
constants and independently check actual input before staging or preload.

The host or the web process composes it with held-descriptor/platform operations:

```rust,ignore
use org_release::{Architecture, Platform, TrustedKeys, TrustedPolicy, verify_manifest};

// Registry, target and freshness floor are trusted build-owned facts.
let keys = TrustedKeys::from_registry_bytes(BUILD_PINNED_REGISTRY)?;
let policy = TrustedPolicy::new(Platform::Macos, Architecture::Aarch64, 1, BUILD_FLOOR)?;
let release = verify_manifest(&exact_manifest_bytes, &raw_signature, &policy, &keys)?;

// Compare against the protected durable pin before activation. This digest is
// over the exact bytes, not a serialization of the parsed manifest.
let manifest_digest = release.manifest_sha256();
release.verify_inventory(actual_regular_payload_paths.iter().map(String::as_str))?;
let expected = release.file("agentic-org-web").ok_or(AdapterError::MissingWeb)?;
let signed_cdhash = expected.macos_cdhash(); // Compare to platform-reported loaded-image identity.

let mut payload = expected.payload_verifier();
while let Some(chunk) = held_descriptor_adapter.next_chunk()? {
    payload.update(&chunk)?;
}
payload.finish()?; // Exact size and SHA256, with overrun/underrun refusal.
```

`file(path)` is an efficient exact-path lookup; normalized aliases do not match.
`verify_inventory` compares regular-payload names only; adapters must exclude the
exact control files, reject unsafe types/metadata/container features and check
all payload contents. Hashing in bounded chunks avoids whole-asset allocation.
An overrun poisons the streaming verifier even if its error is ignored.

The independently enforced profile limits can overlap: a 16,384-entry manifest
may exceed the separate 2 MiB signed-byte cap. Passing either limit never waives
the other. Generated signing keys in unit tests are synthetic fixtures only.
Host/web staging, descriptor identity, architecture/code-signing checks, loaded
image binding, durable publication/pinning and production build trust are outside
this crate and remain necessary before release activation or readiness claims.
