# org-mailbox

The office's messaging library for directed messages between members
([ADR-0003](../../docs/adr/0003-agent-messaging-office-first.md)): the domain rules over
storage and directory ports. Database implementations, sockets, binding and
authorization stay outside it. Dependencies: `org-proto`, `serde_json`, `sha2`; no
database and no async runtime.

Increment B6 of the [implementation plan](../../docs/implementation-plan.md) replaces this
model with the conversations (DMs, group DMs and channels) of the
[messaging data contract](../../docs/design/organization-data/15-messaging-and-chat.md).
Nothing here is carried over into that model.

## What it does today

`Mailbox` implements:

- **send:** validates sizes and kinds per `org_proto::msg`, resolves the recipient
  (trimmed of ASCII whitespace; `owner` first; an exact id beats a handle; `office`
  refused), refuses an offboarded or `Unknown`-status agent (`unknown_recipient`) while
  a paused one still receives, computes a content hash and commits with this office's
  origin. A retry with the same id and content returns the first result. Sending to an
  agent placed in another office is `unavailable`.
- **inbox:** headers newest first, unread only unless `include_all`.
- **read:** claims a message once (the named one, or the oldest unread) and marks it read.
- **wait:** registers, checks, then blocks, so a commit between the check and the block
  still wakes the waiter. It returns the oldest unread message without claiming it.
- **defer**, **done** and **expire_due**, over a `TriageStore`.

The office worker calls `send`, `inbox`, `read` and `wait` from
`apps/agentic-org-office/src/msg_calls.rs`; the store and directory adapters are in
`crates/org-office/src/messaging.rs`. The server passes the bound member id from its
authenticated binding, never from request JSON, and the mailbox rechecks that member
on every call: an offboarded, removed or moved member gets `not_bound`.

## Ports

`MemberDirectory`, `MessageStore`, `TriageStore`, `SyncStore`, `ThreadStore`,
`MessageEvents` and `SessionAuth` (`src/ports.rs`). The directory returns members with
their stored status, and the mailbox applies the policy. `peek_next_unread` is
read-only: it reports expired ids and deletes nothing.

## Held calls

`Mailbox::wait` takes a deadline and a `Cancellation`. The server keeps the matching
`CancelHandle` per connection, runs each held call on its own worker thread and
enforces the per-connection held-call limit. Cancelling the handle (a closed connection
or a re-bind) ends blocked waits with `not_bound` and removes their registrations. A
wait never blocks past the earlier of its deadline and `timeout_ms`, but a store call
already running cannot be interrupted, so stores must bound their own calls. No mailbox
lock is held across a store call. `held_waits()` reports live registrations.

## Events and errors

After each commit, and after each claim, the mailbox calls `MessageEvents`. Events
never carry bodies. `MailboxError` maps one to one to `org_proto::ErrorCode`.

## Testkit and the conformance suite

The `testkit` feature exports `InMemoryStore`, `InMemoryDirectory`, a standard fixture
(`testkit::fixture`) and the conformance suites (`testkit::conformance`,
`testkit::triage_conformance`): store, directory and mailbox cases over any backend. To
run them against another backend:

```rust
fn open(members: &[Member], position: Position) -> (MyStore, MyDirectory) { /* fresh, seeded */ }
org_mailbox::mailbox_conformance_tests!(open);
```
