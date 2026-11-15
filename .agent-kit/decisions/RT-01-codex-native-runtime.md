# RT-01: Codex native runtime prerequisites

- Status: PROPOSED; architecture INCOMPLETE, implementation not ready.
- Scope: [registered native adapter contract](../tasks/v1-codex-runtime-contract.md).
- Architecture owner: Codex lead; actual author session
  `01a1de15-c766-7c73-a794-f98194d43804`, observed `gpt-6.1-sol`, effort `xhigh`.
- Revision 2, source baseline `4dd45f5ccd616cb5c54900196bd88b56ebe30011`;
  evidence inspected 2026-11-15.

## Intent and premises

Implement the native sans-I/O Codex adapter without weakening the selected v1
execution or credential boundaries. No new daemon, spawning in an adapter,
credentials in plans/events, personal-login fallback or unreviewed environment
map is permitted. The overall vertical v1 goal remains outstanding.

| Premise | Authority/evidence | State and consequence |
| --- | --- | --- |
| RT-P1: connection bindings and private auth contexts govern production launch | Selected organization-data documents 11 §§3–4 and 14; agent-sessions selected execution/security amendments | VERIFIED requirement; historical normal-home S0 observations do not override it. |
| RT-P2: the existing port can carry ordinary session settings and protocol lifecycle | `org-runtime` SessionSpec, LaunchContext, Session and testkit | VERIFIED source capability, not proof of a native implementation. |
| RT-P3: selected connection-auth/environment mapping remains a prerequisite | LaunchContext lacks a selected binding/auth context; ProbeContext has no such context; `org-supervise::Environment` and runtime mapper reject CODEX_HOME; document 11's final implementation-boundary section explicitly requires an adapter/environment-contract extension | VERIFIED missing prerequisite. Ada owns the port; River owns supervisor mapping. Resolve the intended equivalent mechanism with them before implementation planning. |
| RT-P4: installed native isolation and model-tool capabilities must be proven | Selected document 11 AE-14; agent-sessions security and S0 requirements | UNVERIFIED for full 0.169.0 live behavior. Generated schemas and standalone commands are insufficient. |
| RT-P5: existing offline probes still work on the installed version | Existing `spikes/codex/probe.mjs` re-run with synthetic temporary HOME/CODEX_HOME, no sign-in or model calls; private version/schema/source hashes and sanitized output retained | VERIFIED narrowly: 22 records, 18 verified-true, no verified-false; informational resume reports no rollout. No exact-root or office-ready claim. |

Recheck these premises when the owning port, environment mapper, selected design,
probe or native binary changes. The full stable contract remains unchanged.

## Proposed direction and alternatives

Use `org-runtime-codex -> org-runtime`; native JSON codecs, launch-policy mapping
and session state remain private to the adapter. At project-crate scope this adds
one outgoing dependency, initially no consumers (Ce=1, Ca=0, I=1). Serde/JSON are
external serialization dependencies. Supervision, storage and other adapters
remain outside this dependency direction.

Prefer a narrow owner-reviewed connection-auth/environment boundary, or an
explicitly equivalent office-supplied context with proven configuration
isolation. The office alone resolves credentials. It must keep launch/probe
account evidence associated with admission and binding-revision checks. A
non-secret path/reference may suffice; this record does not select new fields.

Reject ambient personal authentication, unrestricted environment maps, raw
credentials in plans, hidden adapter-side binding state and a universally
unsupported stub presented as a completed adapter. Missing implementation of a
planned prerequisite does not invalidate the isolation requirement.

Before READY, obtain intended-account/binding evidence, hostile configuration
isolation, model-shell/file-tool fixed-deny and exact-root enforcement, office
socket/Git tests, real start/resume/control/approval/stop/usage fixtures and
deterministic shared-testkit conformance. These are required future checks, not
executed results. Native conversation resume does not prove checkpoint
portability, side-effect replay safety or billing accuracy.

## Review and disposition

Architecture author: **BLOCKED / INCOMPLETE**. No detailed implementation plan or
adapter code is authorized by this assessment. A fresh independent architecture
review is still required after the owner-controlled prerequisite and essential
feasibility evidence are available. It has not been run, and there is no READY
verdict. Private author result and actual CLI metadata are retained separately.

Question sent to the boundary owners: which reviewed launch/probe contract selects
the connection-specific authentication and office-only configuration namespace,
while the supervisor resolves credentials and excludes them from ordinary tools?
An equivalent guaranteed mechanism is acceptable; no particular field addition
is prescribed here.

## Bounded feasibility evidence, revision 2

The lead ran an additional disposable probe on the same installed Codex 0.169.0
binary. Every child used a synthetic temporary HOME/CODEX_HOME and repository;
the only configured MCP commands were `/usr/bin/false`. No authentication or
`turn/start` occurred. Private source, sanitized output and SHA-256 evidence are
retained under `.agent-kit/logs/v1-codex-runtime/config-isolation-*`.

Eight cases establish these narrow observations:

- `mcp_servers={}` does not clear effective home or trusted-project server
  entries. Both synthetic entries remain enabled. It is therefore insufficient
  as a configuration-isolation mechanism.
- Explicit `enabled=false` pins suppress the two known entries in effective
  configuration, requiring an inventory. An untrusted-project override suppresses
  this project layer; it does not remove home-level MCP definitions.
- A controlled empty home plus the CLI untrusted-project override produces no
  synthetic MCP entries or project developer instruction in `config/read`.
  This is configuration evidence, not proof of no startup effects or native-tool
  behavior.
- A `:workspace`-derived named profile allowed creation of a synthetic sibling
  under `/private/tmp` outside its two explicit write roots. A custom profile
  with `/` read-only and exact workspace/scratch write rules, without inheritance,
  allowed both intended writes and refused creation at that sibling path.
  These are standalone `command/exec` results, not model-tool conformance.
- These test profiles require an explicit `default_permissions` selection when
  no legacy sandbox selection is configured. The initial missing-default attempt
  failed at startup; the final probe pins the exact named profile.

This strengthens RT-P4's need for installed-version testing and rules out two
insufficient shortcuts. A controlled configuration namespace and a custom profile
are candidates for subsequent owner-reviewed design and live conformance checks;
neither is adopted as a production solution here. RT-P1–P3 remain unchanged.
The architecture author's r1 assessment has not been represented as validation of
r2. Architecture remains INCOMPLETE; no implementation or READY is claimed.
