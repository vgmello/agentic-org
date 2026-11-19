# ADR-0013 · Browser sessions are stored at the host and survive restarts

Status: **accepted by the owner 2026-11-17** during the architecture review (conflict
plan entry F-01). Implementation pending. Amends
[web-api-v1.md](../design/web-api-v1.md) section 4 and the session-end
rules in [server installation and access UX](../specs/server-installation-and-access-ux.md)
section 4a.4. Where those say that a restart ends sessions, this ADR governs.

"Web" in this ADR means the `agentic-org-web` process; code identifiers still call it
the gateway until they are renamed.

## Context

- A browser session exists today only in memory, in two places: the web process's
  link between a browser's cookie and proof and its host session, and the host's own
  session list (`crates/org-host/src/service/authentication.rs`, a map bounded at 1024).
- The host ends every session when its control connection to the web process closes.
  So a crash, a restart or an update of either process signs every person out.
- Updates will be routine. Signing everyone out on each one is not acceptable.
- The reasons for the in-memory design were sound and are kept: the web process faces
  the network, so it holds no database key and no session-signing key, persists
  nothing and opens no database, and ending a session must be immediate. (It does
  hold, in memory, the TLS private key the host supplies.)
- This ADR is about browser sessions. Native sessions of the local CLI on
  `host.sock` share the same in-memory list today and are not changed by it.
- Signed tokens were considered. They remove server-side session state, but a signed
  token cannot be withdrawn before it expires without adding state back. They are
  deferred to v2 (decision 7).

## Decision

1. **A browser session is a durable record in the host database.** It holds a session
   ID, the account, creation, last-activity and expiry times, the instance binding,
   the origin binding (scheme, host and port), and SHA-256 digests of the cookie value
   and of the proof value. The raw cookie and proof values are never stored and never
   reach the host.
2. **The cookie and proof contract does not change.** The instance-named HttpOnly cookie and
   the `X-Org-Session-Proof` header keep their current form, binding and rules. The
   session view has no absolute expiry field, because sessions have no absolute lifetime.
3. **The web process keeps a cache in memory.** It creates the cookie and proof
   values at sign-in, as it does today, and sends the host only their digests with the
   origin binding. For a request whose cookie it has not seen, including the first
   request after a restart, it sends the same digests and binding to the host once
   over the control connection. On success it caches the result, and every later
   request is answered from memory. It does not call the host per request.
   **Expiry (owner decision 2026-11-17).** No forced periodic sign-in. A session ends
   after 30 days without use (sliding idle expiry); there is no absolute lifetime, so a
   session in use is never ended by age. The web process tracks activity in memory and
   reports it to the host at a bounded interval while a session is active (at most
   every 5 minutes), so the stored last-activity time is never staler than that. After
   a restart, idleness is judged from the stored value.
   **Refresh.** While the app is in use the UI refreshes the session in the background.
   A refresh rotates the cookie and proof, keeps the session ID, and replaces both
   stored digests together at the host.
   **Step-up.** The password is asked again only for sensitive actions, never
   periodically; the [installation and access UX](../specs/server-installation-and-access-ux.md)
   names them (changing the password, handing over the administrator role).
4. **Restarts do not end sessions.** A restart of the web process, a restart of the
   host, an update, or a lost control connection leaves stored sessions valid. A cache
   entry is trusted only while the control connection that validated it is alive;
   after a reconnect the web process validates again.
5. **Ending a session is still immediate.** On sign-out, expiry, password change, or
   when the account's access is removed, the host deletes the record first and then
   tells the web process to drop its cache entry and close that session's stream. A
   deleted record can never be validated again. When an account gains access to a
   further organization, or the administrator role or an organization's ownership
   changes hands, the session continues and the host pushes a session-changed message
   on the same channel, so the cached session view is never stale.
6. **The host owns sessions, and the web process still stores nothing and signs
   nothing.** Accounts,
   password verification, lockout and revocation are the host's, per instance. The
   web process only relays a sign-in to the host and caches the answer. If it held a
   signing key or session store, a compromise of the network-facing process would let
   an attacker create sessions for any account, and it has no durable storage of its
   own in which to keep one across restarts.
7. **Signed tokens are a v2 option, not a v1 requirement.** They become worthwhile if
   there is more than one web process or the web process runs on a different machine
   from the host. If adopted, the host signs with a private key it keeps, the web
   process verifies with the public key, tokens are short-lived, and a per-account
   marker in the host database makes password change and sign-out-everywhere immediate.
8. **Limits stay bounded internally.** There is no per-account cap (owner decision
   2026-11-17); the host keeps only an internal host-wide safety bound. The
   per-connection cap no longer limits browser sessions, because they outlive
   connections.
9. **Native sessions stay connection-bound.** A session of the local CLI on
   `host.sock` still ends with its connection. In particular the recovery
   acknowledgement permission of ADR-0011, which must expire with the live session and
   connection, is unchanged.

## Testing

- API and integration tests sign in the way a browser does: create a disposable
  instance under a temporary root with a test account, call
  `POST /api/v1/session/sign-in`, keep the returned cookie and proof, and send both on
  every later call. No test-only bypass, fixed token or mock backend exists in a
  production build.
- A test helper that performs that sign-in and attaches the cookie and proof is the
  supported way to script the API in v1. Tokens for non-browser clients are a v2 topic
  together with decision 7.

## Acceptance criteria

| ID | Passing evidence |
| --- | --- |
| SES-1 | After the web process is restarted, a signed-in browser's next request succeeds without signing in again. |
| SES-2 | After the host is restarted, the same holds once the service is ready. |
| SES-3 | Sign-out, expiry, password change and removing the account's access each make the next request fail with `not_authorized`, including when the web process has the session cached. |
| SES-4 | The host database contains no raw cookie or proof value; a copy of it cannot be used to act as a signed-in person. |
| SES-5 | The web process opens no database, persists nothing and holds no session-signing key; it validates an unknown cookie with exactly one host call and then serves from memory. |
| SES-7 | A session ends after 30 days without use, including across a restart, with stored last-activity never more than the reporting interval behind; a session in use is never ended by age, and a refresh keeps its ID. |
| SES-8 | A native CLI session on `host.sock` still ends when its connection closes. |
| SES-6 | The host-wide safety bound is enforced and reported without revealing whether an account exists; there is no per-account cap. |

## Consequences

- People stay signed in across updates and crashes.
- Session records are on the host's disk, which the earlier design avoided. They hold
  only digests, so reading the file does not yield a usable session.
- The host gains one table and one validation operation. For browser sessions, the
  earlier work that binds a session's life to the control connection must be changed,
  not extended.
