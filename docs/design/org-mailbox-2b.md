# org-mailbox slice 2b: ask, reply, notify

Draft for ada · `remy/org-mailbox-2b` (from b940272, slice 2a) · Remy

## 1. Goal and scope

Add `ask`, `reply` and `notify` to `Mailbox` (ADR-0003 §2.2 claim-before-commit; the locked notify contract), and close the 2a lows. No port signature changes, no SQL, no new dependencies.

Out of scope:
- cross-office routing (stays `unavailable`, mailbox.rs:186-188)
- the CLI's move to the office (apps/org-agent/src/office.rs:8)
- dead-lettering

New public methods:
- `ask(bound, MsgAskParams, deadline: Instant, &Cancellation) -> MsgAskResult`
- `reply(bound, MsgReplyParams) -> SendResult`
- `notify(bound, MsgNotifyParams) -> NotifyResult`

All three return `unavailable` without triage, as defer does (mailbox.rs:559-561). `held_waits()` counts asks. New private modules: `notify_text.rs`, `notify_memory.rs`, `mailbox/{ask,notify}.rs`.

**Locks.**
- Store calls never run under a registry lock (mailbox.rs:43-45).
- Order: `triage_writes` before `snoozes`.
- The registry is released before any slot lock is taken. `notify_memory` is a leaf.
- New: `release_due_snoozes` holds `triage_writes` across take, `check_notify` and restore (D7).

## 2. Ask slots (waiters.rs)

`RegistryState` (waiters.rs:27-31) gains `asks: HashMap<ask_id, {responder, claimed, closed, slots}>`. Each slot is `Waiting | Delivered(Message) | Taken | Cancelled`; Message's `Debug` redacts bodies (msg.rs:384-392).

Under the registry mutex:
- `register_ask`: one slot per held call; a re-ask adds one. Drop marks it Cancelled.
- `claim_reply(id, from)`: succeeds at most once. `from` must be the responder (on an unclosed entry) or `office`, and some slot must be Waiting.
- `complete(msg) -> bool`: moves Waiting slots to Delivered.
- `abandon()` (also on Drop): unclaims and wakes, so the waiters re-check the store.
- `close_ask(id) -> bool`: the asker's deadline. False if a reply already claimed.
- Cancel (waiters.rs:133-146): Waiting → Cancelled. Delivered is left alone.

## 3. msg.ask

1. **Validate** as send does (mailbox.rs:114-145):
   - `wait_ms ≤ MAX_WAIT_MS`
   - `ttl_ms` in `1..=MAX_TTL_MS`, defaulting to 120 s for an agent and 24 h for a person (msg.rs:166-169)
   - priority defaults to `urgent`
2. **Existing id** (`get_message`):
   - another sender, kind or hash: `already_exists`
   - a match: a re-ask, which keeps its `expires_at` (ADR l.547-555)
3. **Register, then commit.** `register_ask(id, recipient)` and attach the cancellation, then commit `Unread`. When inserted, wake the recipient and publish (mailbox.rs:246-253). On error, return it; the client re-asks.
4. **Check.** `find_reply_for_ask` (ports.rs:107-116) catches an answer that landed before a re-ask registered.
5. **Block** until the first of: the caller's deadline, `wait_ms`, or `expires_at + min(ttl/4, 2 s)` (as at mailbox.rs:437-443). Each wake takes a Delivered slot, or else checks again.
   - `wait_ms` or the caller's deadline: `{reply: None, expires_at}`
   - cancel: `not_bound`

**Answer.**
- A notice: `timeout`, or `unknown_recipient` per its `meta.reason`.
- A reply: returned. If it's still unread, `claim_read(asker, Some(id))` first, so the hook won't repeat it.

**Deadline.** Call `close_ask`.
- False: a reply is mid-commit, so keep blocking. The first of claim and close wins.
- True: check once more, then `expire(recipient, …, Ignore)` (mailbox.rs:641-688):
  - an agent's request becomes `expired`, with one notice
  - a person's is flagged `expired_waiting_person` and stays unread
- Either way, return `timeout`.

**Notice claim.** `notify_expired` (mailbox.rs:722-775) first claims `(original, office)`. If claimed, it commits the notice `Read`, then calls `complete` or `abandon`. A held asker gets `timeout`, never an unread duplicate.

**Asker disconnects.**
- Before a claim: the reply commits `Unread`.
- Between claim and complete: `complete` returns false, and the replier runs the **fallback**: `update_state_local(Unread)` on the reply (allowed from read, ports.rs:141), wake the asker's inbox waiters, publish (ADR l.292).
- After complete: the ask thread sees Delivered while cancelled, runs the same fallback, and returns `not_bound`.

The slot mutex makes exactly one side run it. A lost socket write is recovered by a re-ask (ADR l.554).

## 4. msg.reply

1. **Parent** (`get_message(to_id)`):
   - missing, or not addressed to the caller (the asker or a third party): `not_found` (msg.rs:554)
   - a notice: `invalid_request`
2. **Recipient**: the parent's sender, by id. It can't receive: `unknown_recipient`. Another office owns it: `unavailable`.
3. **Priority** when omitted: `high` if a person answers a flagged or overdue request (ADR l.432), else `normal`.
4. **Replay** as in send (mailbox.rs:146-180): skip steps 5–6 and run step 7.
5. **Claim**: for a request, `claim_reply(parent, bound)`. If claimed, commit `Read`; otherwise `Unread`.
6. **Commit**:
   - error: `abandon` (the asker re-checks, which covers a commit that did land) and return the error
   - not inserted: `abandon`
   - claimed: `complete`, then the fallback if it returns false
   - in every case that commits, wake the asker and publish
7. **Parent → read**. If it's `Unread` or `Snoozed`: `update_state_local(Read)` under `triage_writes`, drop its timer (as done does, mailbox.rs:530-543), publish. Other states are left alone. A failure returns `storage_error`; a retry replays the reply (no second delivery) and finishes.

Edge cases:
- **A late reply to an expired agent request** is allowed (msg.rs:551-552) and delivered `Unread`; the request stays expired. A re-ask gets `timeout`: the notice came first.
- **An overdue request that hasn't expired yet**: the reply wins; the later expiry sends no notice (mailbox.rs:693-698).
- **`msg.send` with `reply_to` set to a request**, from its recipient to its asker, also runs steps 5–6, because `find_reply_for_ask` counts it.

## 5. msg.notify

1. **`session_id`**, trimmed. Over `MAX_ID_BYTES`: `too_large`. Empty or with control characters: `invalid_request` (meta.rs:1554-1565).
2. **Breaks.** `notify_memory` keeps `{in_break, break_session}` per member, ported from meta.rs:362-392. There's no port and no `break_seq`.
3. **One `check_notify` call** (ports.rs:174-199):
   - A check that starts a break: `after_task=true`. Every after-task snooze predates the break (meta.rs:1220).
   - An after-task check inside a break: `after_task=false`, `sweep_before=now+1ms`, `Sweep` relabelled as `New`. Phase A releases nothing here either (meta.rs:1134-1136).
   - Otherwise: `after_task=false`, `sweep_before = now − sweep_after_ms`.
4. **Releases.** `remove_if_due` drops the timers of `Due` releases. This is race-free, because a re-defer is always in the future. Then wake and publish.
5. **Expiry.** `check_notify` doesn't expire (ports.rs:187), so `expire(…, Ignore)` each overdue, unflagged request, up to `MAX_EXPIRED_IDS`. An agent's request drops out; a person's stays, flagged.
6. **Dedup.** Skip `(member, session, id)` already reported at this `triage_gen`. At most 10 000 entries, 24 h TTL (ADR l.322), keyed by member too.
7. **Order.** Urgent first, then by id (meta.rs:1472). `count` covers everything. `items` are capped at `NOTIFY_MAX_ITEMS` and recorded (msg.rs:739-740; Q1).
8. **Items.**
   - `from`: the sender's handle from a cached `resolve_member`; the id if there's no handle; `office` for notices.
   - `from_name`: through `notify_display_name`.
   - `subject`: sanitized to 80 characters, or `(no subject)`.
   - A flagged or overdue request is marked `(expired)`.
9. **Text.** A port of `NotifyReport::build` (meta.rs:1569-1606). It shows `min(items, NOTIFY_MAX_LINES)` lines, with `more = count − shown`. Phase A caps at 10 (meta.rs:43), so the text is byte-identical.

**Exit codes** (main.rs:1239-1247, office.rs:103-124):
- 0: `count == 0`
- 11: an urgent item
- 10: any other item
- 1: `invalid_request`, `too_large` or `not_bound`
- 4: `storage_error`
- 5: `unavailable` (the adapters treat it as nothing)

**Formatter port.** Copy the sanitizer, `notify_display_name` and the fixed strings verbatim (meta.rs:46-47, 129-153, 487-594). Shared golden files (raw items in, text out) are read by path in each crate's test, so the crates don't depend on each other (Q4). They cover bidi, zero-width, escapes, quotes, a forged name, truncation, `(no subject)`, `(expired)` and 12 items.

## 6. Port changes

No signature changes. Three doc-only changes, each with a conformance case:
- `claim_read(None)` never returns an expired message. The memory store already skips them (testkit/memory.rs:192-199); ports.rs:64-69 doesn't say so (Q5).
- `find_reply_for_ask`'s "earliest" means commit order.
- ports.rs:38-45: `triage_writes` also covers the timer-driven `check_notify`.

## 7. Error mapping

- **ask**: `invalid_request` (ranges, priority, id, `office`), `too_large`, `unknown_recipient`, `already_exists`, `timeout`, `unavailable`, `not_bound`, `storage_error`.
- **reply**: the same, with `not_found` instead of `timeout`.
- **notify**: `too_large`, `invalid_request` (a bad `session_id`, newly listed), `unavailable`, `not_bound`, `storage_error`.
- **done and defer**: add `invalid_request` (the message is closed) and `unavailable` to msg.rs:672-673 and 709-710.

No new `MailboxError` variants.

## 8. Deferred 2a items

- **D1**: index the timers per member (`HashMap<member, BTreeSet<(at, id)>>`) instead of scanning them all (snoozes.rs:48-71), and add `remove_if_due`.
- **D2**: no releases-only port. Documented cost: each due timer (mailbox.rs:604-605) also reads the member's open urgent messages.
- **D3**: `read(None)` re-peeks while full batches come back, up to `MAX_RECHECKS` (mailbox.rs:390-427).
- **D4**: done or defer on an agent's overdue open request runs `expire(…, Fail)`. done then returns `Ok`; defer returns `invalid_request` (Q2).
- **D5**: `read(id)` of an expired request returns the header with an empty body and meta (mailbox.rs:297-327; Q3).
- **D6**: with D4's pre-read, done on a message that's already done publishes nothing. The remaining race stays documented (mailbox.rs:886-889).
- **D7**: take, release and restore all run under `triage_writes` (mailbox.rs:599-614).
- **D8**: ADR l.279: `with_triage` seeds the timers, and `new` has none.
- **D9**: the restore test (triage_conformance.rs:1909) asserts zero commits.
- **D10**: a case where a reply with a larger ULID commits first and is still the answer.
- **D11**: if defer hits a `Failure`, re-read the message under the lock and set its timer from `snoozed_until`. If that read fails too, keep the old timer.
- **D12**: the error-list changes in §7, through ada.

## 9. Test plan

**Conformance** (`triage_conformance_tests!`, run on the memory store and codex-dev's SQL store):
- every step and edge case in §3–5
- D3–D6, D9, D10, Q5
- Phase A's break rules (port rules.rs:302-551)
- golden text

**Mailbox unit tests** (forced interleavings through a hooked store, like `RacingStore`):
- **Reply vs expiry.** Claim first: the ask gets the reply. Close first: the reply stays `Unread` and the ask gets `timeout`.
- **Reply vs a second reply.** Exactly one is `Read` and delivered.
- **Cancelled waiter.** Cancelled before the claim, between claim and complete, or after complete: the fallback runs once.
- **Failed commit after a claim**, including one that landed: the asker finds the reply.
- **The ask's own expiry.** `timeout`, with the notice `Read`.
- **Units:** `waiters`, `snoozes`, `notify_memory`, `notify_text` (porting rules.rs:716-830 and 985-1015), D11.

## 10. Open questions for ada

1. Items 11–100 are recorded but never shown; Phase A records only what it shows (rules.rs:709-711). Should we match Phase A?
2. Should done on an expired or overdue request return `Ok` or `invalid_request`?
3. A header-only read of an expired request hides the context a late reply needs. Is that OK?
4. Should the golden fixtures live in `crates/org-proto/tests/fixtures/notify/` (yours) or in the testkit?
5. Should `claim_read(None)` be required to skip expired messages? The SQL store must match.
6. An in-break re-check doesn't re-list a normal message that was released and is still unread; Phase A does. Accept that, or add a `TriageStore` method?
7. Is a person's late reply `high` only when no priority is given, or always?

## 10a. ada's decisions (2026-11-09, 01M6ZDZ70CCDKVRBFHJ99RAJYC)

1. Match Phase A: session dedup records only the items shown in the text (at most `NOTIFY_MAX_LINES`). Recording items 11-100 would suppress messages nobody saw.
2. done on an expired or overdue request returns `Ok`; it's idempotent. D4: done expires an agent's overdue open request (one notice) and returns `Ok`.
3. D5 is dropped. An explicit `read <id>` returns the full body even after expiry; expiry changes delivery state, not readability. Only listings and notify stay header-only.
4. The golden notify fixtures live in `crates/org-proto/tests/fixtures/notify/` (ada's contract), in their own commit; the testkit and org-spool load them from there.
5. `claim_read(None)` never returning an expired message becomes a contract requirement with a conformance case. Tell codex-dev when 2b's suite lands.
6. Keep Phase A's behaviour: the in-break re-check re-lists a released, still-unread normal message. Use existing methods if they can; otherwise add one additive `TriageStore` method with a conformance case (codex-dev implements it in SQL).
7. A person's late reply defaults to `high` only when no priority is given; an explicit priority is always respected.

## 11. Commit plan

1. D1
2. D7, D3, D11, D2
3. D9, D10, Q5
4. `notify_text` and the goldens
5. ask slots
6. reply
7. ask and the notice claim
8. notify
9. D4–D6
10. docs: D8, ADR §2.2/§2.3, ports.rs, D12

Each commit: fmt, clippy `-D warnings`, workspace tests green.
