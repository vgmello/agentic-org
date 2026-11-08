# org-mailbox

The office's message broker library (ADR-0003): the messaging domain over storage and
directory ports. Database implementations, sockets, binding and authorization stay outside it.

## Status: slice 1 done; not release-ready

Slice 1 is local delivery at HQ for a hand-run agent: `msg.send`, `msg.inbox`, `msg.read` and
`msg.wait`. It is tested against the in-memory testkit only. Nothing calls it yet: the office
server doesn't wire it in, and no SQL store runs the conformance suite yet.

What slice 1 does:

- **send** validates sizes and kinds per `org_proto::msg`, resolves the recipient (Addressing:
  trimmed of ASCII whitespace; `owner` first; an exact id beats a handle; `office` refused),
  refuses an offboarded or `Unknown`-status agent (`unknown_recipient`) while a paused one still
  receives, computes the canonical `content_hash` (ADR §3.4) and commits with this office's
  origin. The store allocates `local_seq` and `origin_seq` from one local counter. A retry with
  the same id and content returns the first result. Send never creates an expiring message.
- **inbox** lists headers newest first by `local_seq`, unread only unless `include_all`.
- **read** claims a message once (the named one, or the oldest unread) and marks it read.
- **wait** follows ADR §2.2: register, then check, then block. A commit between the check and
  the block still wakes the waiter. It returns the oldest unread message without claiming it.

The server passes the bound member id from its authenticated binding, never from request JSON.
The Mailbox rechecks that member on every call: an offboarded, removed or moved member gets
`not_bound`.

## Ports

ADR-0003 §4.3, as amended (store split). `MemberDirectory` reads org.db. `MessageStore`,
`TriageStore`, `SyncStore` and `ThreadStore` cover office.db. No transaction spans both
databases. Slice 1 uses only `MessageStore` and `MemberDirectory`. The directory returns
members with their stored status, and the Mailbox applies the policy. `peek_next_unread` is
read-only: it reports expired ids and deletes nothing. `SessionAuth` (binding) is the server's.

## Held calls

`Mailbox::wait` takes a deadline and a `Cancellation`. The server keeps the matching
`CancelHandle` per connection, runs each held call on its own worker thread and enforces the
per-connection held-call limit. Dropping or cancelling the handle (a closed connection or a
re-bind) ends blocked waits with `not_bound` and removes their registrations. A wait never
blocks past the earlier of its deadline and `timeout_ms`, but a store call already running
can't be interrupted, so stores must bound their own calls. No Mailbox lock is held across a
store call: the `WaiterRegistry` lock covers only register and notify. `held_waits()` reports
live registrations.

## Events and errors

After each commit, and after each claim this call made, the Mailbox calls `MessageEvents`
(`NoMessageEvents` in slice 1). Events never carry bodies. `MailboxError` maps 1:1 to
`org_proto::ErrorCode` (`From<MailboxError> for ErrorCode`).

## Testkit and the conformance suite

The `testkit` feature exports `InMemoryStore` (delivery port only), `InMemoryDirectory`, a
standard fixture (`testkit::fixture`) and the generic conformance suite
(`testkit::conformance`): store, directory and Mailbox cases over any backend. To run it
against another backend:

```rust
fn open(members: &[Member], position: Position) -> (MyStore, MyDirectory) { /* fresh, seeded */ }
org_mailbox::mailbox_conformance_tests!(open);
```

## Follow-ups (not in slice 1)

- `ask`, `reply`, the claim-before-commit reply hand-off and `find_reply_for_ask` use.
- Triage: `defer`, `done`, `notify`/`due`, snoozes and the snooze timer heap (`TriageStore`).
- Expiry: acting on peek's `expired_ids` (`update_state_local(Expired)`), dead letters.
- `msg.list`, `msg.get`, `msg.threads`, `msg.human_needs_you` (`ThreadStore`).
- Sync and cross-office routing (`SyncStore`, per-origin streams, relay, `resolve_pending_references`).
- Events wired to subscribers, and body compression (`flate2`).
- Server wiring (`msg.bind` with `SessionAuth`, worker threads, held-call limit), `store.rs`
  running this suite, and `org-agent` switching to the office server.

Codex-dev's draft in-memory triage, sync and thread implementations are in commit `cc67d64`.

## Dependencies

`org-proto`, `serde_json`, `sha2` (Ce = 3). No SQLite, no async runtime.
