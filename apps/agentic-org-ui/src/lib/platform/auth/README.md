# Browser session ownership

This module owns the browser's cookie and proof pair for one web origin, as
[web API v1](../../../../../../docs/design/web-api-v1.md) and
[ADR-0013](../../../../../../docs/adr/0013-stored-browser-sessions.md) define it. It is
not yet composed into the application: only its own tests use it. Increment A3
connects it to sign-in. `agentic-org-web` owns cookies, session authorization and
permissions; all HTTP uses the same-origin bounded JSON transport, with no
local-server fallback.

`createSessionController` receives storage, the web API client, the read transport,
Web Locks, BroadcastChannel, a clock and a state callback explicitly. It publishes
session views without proofs. Its `read` operation is GET-only: on 401 it re-reads
storage and retries once, only if the same account and instance has a different
proof. A 403 is returned unchanged. `submit` is one POST on the same terms: the caller's body
carries the operation ID, and it is sent again only after such a 401, which proves it was not admitted. Operations that change the pair are explicit and
never retried.

IndexedDB database `org-session`, store `session`, key `current` holds
`{instance_id,account_id,login,proof,rotated_at_ms}`. Reads and writes validate it;
the transaction completes before state or a broadcast is published. Every pair change
takes the lock `org-session:refresh`. BroadcastChannel `org-session` messages are
`{kind:'signed-in',account_id}`, `{kind:'rotated'}` or `{kind:'signed-out'}`;
recipients re-read storage. Proofs never go in broadcasts, URLs,
localStorage/sessionStorage or published state. A missing proof has no cookie-only
recovery. Another account clears earlier views and requires a reload.

Refresh is an explicit locked `refresh(triggered_at_ms)` only: there is no timer.
A sign-out clears the record before notifying only on a confirmed end: success or an
unauthenticated 401. Every other failure, including a 503 that may precede dispatch,
keeps storage, state and the proof so the person can try again (ADR-0013, SIGN-29). Dispose detaches notifications and rejects new operations and late read
results. Pair changes already dispatched may finish persisting, without publishing to
the disposed view. HTTP errors expose the status, a validated duration and the
session reason, never the server's message. Browser storage failures use a generic
error.
