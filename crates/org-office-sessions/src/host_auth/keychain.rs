//! The host Claude Code sign-in from the macOS Keychain: one bounded lookup through
//! `org_platform::keychain`, cached only while a launch is resolving host OAuth.

use super::*;

/// Upper bound on macOS Keychain lookup so a locked Keychain never hangs session launch or
/// transcript redaction.
pub(super) const KEYCHAIN_LOOKUP_TIMEOUT: Duration = Duration::from_secs(3);

/// The macOS Keychain item holding the host Claude Code sign-in.
pub(super) const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// How long one Keychain answer (including "no item") is reused within a launch. One launch
/// resolves the host token up to four times (before and after a refresh probe, then once more);
/// a short-lived answer serves them with one `security` run, and every refresh probe drops it
/// ([`invalidate_keychain_cache`]) so a rotated token is re-read.
pub(super) const KEYCHAIN_CACHE_TTL: Duration = Duration::from_secs(2);

/// A launch-scoped, single-flight cache of the host Claude Keychain answer. Answers are kept only
/// while at least one [`KeychainCacheScope`] (a launch resolving host OAuth) is open, and are
/// dropped (zeroized) when the last scope closes, so no token outlives the launches that needed
/// it. The lock is held across a lookup, so concurrent launches wait for one bounded run.
pub(super) struct KeychainTokenCache {
    pub(super) state: Mutex<KeychainCacheState>,
}

pub(super) struct KeychainCacheState {
    pub(super) scopes: usize,
    pub(super) entry: Option<(Instant, Option<ClaudeOAuthToken>)>,
}

/// Keeps Keychain answers cacheable until dropped.
pub(super) struct KeychainCacheScope<'a> {
    pub(super) cache: &'a KeychainTokenCache,
}

impl KeychainTokenCache {
    pub(super) const fn new() -> Self {
        Self {
            state: Mutex::new(KeychainCacheState {
                scopes: 0,
                entry: None,
            }),
        }
    }

    pub(super) fn scope(&self) -> KeychainCacheScope<'_> {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).scopes += 1;
        KeychainCacheScope { cache: self }
    }

    pub(super) fn get_or_lookup(
        &self,
        ttl: Duration,
        lookup: impl FnOnce() -> Option<ClaudeOAuthToken>,
    ) -> Option<ClaudeOAuthToken> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.scopes == 0 {
            drop(state);
            return lookup();
        }
        if let Some((read_at, token)) = state.entry.as_ref() {
            if read_at.elapsed() < ttl {
                return token.clone();
            }
        }
        // Drop (and so zeroize) an expired answer before looking up a new one.
        state.entry = None;
        let fresh = lookup();
        state.entry = Some((Instant::now(), fresh.clone()));
        fresh
    }

    pub(super) fn invalidate(&self) {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).entry = None;
    }
}

impl Drop for KeychainCacheScope<'_> {
    fn drop(&mut self) {
        let mut state = self.cache.state.lock().unwrap_or_else(|p| p.into_inner());
        state.scopes = state.scopes.saturating_sub(1);
        if state.scopes == 0 {
            state.entry = None;
        }
    }
}

pub(super) static CLAUDE_KEYCHAIN_CACHE: KeychainTokenCache = KeychainTokenCache::new();

/// Drops the cached Keychain answer (after a refresh probe may have rotated the token).
pub(super) fn invalidate_keychain_cache() {
    CLAUDE_KEYCHAIN_CACHE.invalidate();
}

pub(super) fn read_claude_keychain_token() -> Option<ClaudeOAuthToken> {
    CLAUDE_KEYCHAIN_CACHE.get_or_lookup(KEYCHAIN_CACHE_TTL, lookup_claude_keychain_token)
}

/// One bounded Keychain lookup through the platform capability (`org_platform::keychain`):
/// output is drained while `security` runs (no pipe-buffer stall), oversized answers and
/// timeouts are refused, and targets without a keychain adapter answer `None`.
pub(super) fn lookup_claude_keychain_token() -> Option<ClaudeOAuthToken> {
    let secret = org_platform::keychain::find_generic_password(
        CLAUDE_KEYCHAIN_SERVICE,
        MAX_HOST_CLAUDE_CRED_BYTES,
        KEYCHAIN_LOOKUP_TIMEOUT,
    )
    .ok()??;
    parse_claude_oauth_token(secret.as_slice())
}
