# Browser session ownership

This module owns the browser cookie/proof pair for one gateway origin. P6a adds
its foundation without enabling production routes; P6b composes it with the UI.
The gateway owns cookies, session authorization and authoritative permissions.
All HTTP uses the same-origin bounded JSON transport and web-api-v1 §2/§4/§5
(api_version 1, minimum contract_revision 2); there is no local-server fallback.

`createSessionController` receives storage, gateway, read transport, Web Locks,
BroadcastChannel, clock and state callback explicitly. It publishes session views
without proofs. Its `read` operation is GET-only: on 401 it re-reads storage and
retries once only if the same account/instance has a different proof. A 403 is
returned unchanged. Pair-changing operations are explicit and never retried.

Per Rowan's approved P6 pins (01M7A060R1K0137394QPXM2HEH and
01M7A0R7NV7A9F097A032NQFH8), IndexedDB `org-session` / store `session` / key
`current` holds `{instance_id,account_id,login,proof,rotated_at_ms}`. Reads and
writes validate it; transaction completion precedes state/broadcast publication.
Every pair change takes `org-session:refresh`. BroadcastChannel `org-session`
messages are `{kind:'signed-in',account_id}`, `{kind:'rotated'}` or
`{kind:'signed-out'}`; recipients re-read storage. Proofs never go in broadcasts,
URLs, localStorage/sessionStorage or published state. Missing proof has no
cookie-only recovery. Another account clears prior views and requires Reload.

Until the final P4 trigger policy lands, refresh is an explicit locked
`refresh(triggered_at_ms)` only: no timer or guessed scheduling. The gateway
invalidates an authenticated logout's pair before replying and clears the cookie
half on success, on an unauthenticated 401 and on the 503 (pool refusal or
timeout) that follows, so those replies clear the record before notification
and then surface any refusal. A 400/403 refused before authentication changes
nothing and, like a failure without a reply (HQ unreachable), preserves storage
and state and is rethrown ("Couldn't sign out"). Documented limit: a refusal
after authentication (a 400 body or a 413 too-large body) has already
invalidated the binding and cleared the cookie, but the UI keeps its record
until the next 401. Pre- and post-authentication 400s are indistinguishable by
status, and the UI always sends `{}`, so only a foreign request meets this.
Dispose detaches notifications and rejects new operations/late read results.
Already-dispatched pair changes may finish persisting, without publishing to the
disposed view. HTTP errors expose status, validated duration and session reason,
never the server message. Browser storage failures use a generic error.
