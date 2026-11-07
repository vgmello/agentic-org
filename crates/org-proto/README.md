# org-proto

The org API contract between the desktop app (UI and Tauri shell) and `orgd`: every type that
goes over the wire, the JSON-RPC 2.0 methods with their params and results, the live events and
the error type. The UI's TypeScript types are generated from this crate, so the two can't drift.
Changes here need ada's (`ac-staff-engineer-ada`) review.

## Layout

| Module | What |
| --- | --- |
| `ids` | String newtypes: `MachineId`, `AgentId`, `ProjectKey`, `RoleId`, `SessionId`, `Timestamp` (RFC 3339 UTC), `IsoDate`, `WorkspacePath`. Plain JSON strings on the wire. |
| `model` | Machine, organization, environment, projects, roles, agents, presence, sessions, goals, backlogs, Needs you, activity, documents. |
| `hire` | Hire requests, HR's `Proposed<T>` suggestion, plan, preview and `HireProgress`. |
| `event` | `OrgEvent`, the live event stream. |
| `error` | `ApiError` and the JSON-RPC error codes. |
| `rpc` | `MethodName`, one `Method` impl per method (`methods::*`), params/result types, `Snapshot`, `ApiInfo`, and the JSON-RPC envelope (`Request`, `Response`, `Notification`, `ErrorObject`). |
| `typescript` | `export_typescript(dir)`, the ts-rs export. |

`API_VERSION` and `EVENT_NOTIFICATION` (`"event"`) are at the crate root.

## JSON conventions

- snake_case fields and enum values (`KnownTaskStatus` is kebab-case: `in-progress`).
- Unions are tagged with `kind` (`#[serde(tag = "kind", rename_all = "snake_case")]`), including
  `Tools` (`{kind:"inherit"} | {kind:"list", tools}`) and `Proposed<T>`
  (`{kind:"settled", value} | {kind:"needs_choice", value: T | null}`).
- `Option<T>` is `T | null` and always present. Only optional method params (`agents`'
  `include_offboarded`, `needs_you`'s `include_dismissed`, `activity`'s `limit`, `documents`'
  `kind`, `resume_session`'s `session_id`) are omitted when unset.
- Every entity carries `machine_id`.

## JSON-RPC

- Methods are the snake_case names of the `OrgApi` calls (`needs_you`, `assign_task`, …), plus
  `hello`, `snapshot`, `subscribe` and `unsubscribe`. `params` is always an object; methods
  without params take `{}` (or no `params`). Methods that return nothing return `null`.
- Errors: the error object's `data` is the `ApiError`, `code` is per kind (`error::codes`).
- After `subscribe`, events arrive as notifications `{"method": "event", "params": <OrgEvent>}`.

## TypeScript

The generated files are committed in `app/src/lib/api/generated/` (one file per type, plus an
`index.ts` that re-exports them all with `API_VERSION`, `EVENT_NOTIFICATION`, and `OrgMethods`,
a map from method name to `{ params, result }`). The directory is fully generated; don't edit it.

Regenerate after changing a type:

```sh
cargo run -p org-proto --example export-ts
```

`cargo test -p org-proto` fails (test `committed_typescript_is_up_to_date`) when the committed
files are stale, missing or extra. Keep the directory out of the UI's formatter and linter, or
the test will see their rewrites as drift.

## Tests

```sh
cargo test -p org-proto
cargo clippy -p org-proto --all-targets -- -D warnings
```

`tests/json_shapes.rs` pins the exact JSON of the tricky types (tagged unions, `Proposed<T>`,
`Tools`, null fields, flattened types, `HireProgress`, `OrgEvent`, `ApiError`, params and the
envelope) and reads each one back.
