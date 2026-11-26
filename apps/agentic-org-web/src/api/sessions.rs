//! Caller sessions: authentication state, lookup scopes, validation, caching and revalidation.

use super::*;

impl SessionApi {
    /// The accepted socket's peer; absent only off the real transport (fails closed).
    pub(super) fn peer(request: &Parts) -> Result<IpAddr, ApiReply> {
        request
            .extensions
            .get::<PeerAddress>()
            .map(|p| p.0.ip())
            .ok_or_else(unavailable)
    }

    pub(crate) fn is_auth_enabled(&self) -> bool {
        self.auth_enabled.load(Ordering::Acquire)
    }

    pub(crate) fn set_auth_enabled(&self, enabled: bool) {
        self.auth_enabled.store(enabled, Ordering::Release);
    }

    pub(crate) fn auth(&self, caller: &CallerAuth) -> BrowserAuth {
        match caller {
            CallerAuth::Pair(pair) => BrowserAuth {
                cookie_digest: pair.cookie_digest,
                proof_digest: pair.proof_digest,
                origin: self.origin.clone(),
                local_authless: false,
            },
            CallerAuth::LocalAuthless => BrowserAuth::local_authless(self.origin.clone()),
        }
    }

    /// The lookup's query names this instance and nothing else (web-api-v1 §5): checked
    /// before any host call. Ids need no escaping, so an escape is refused.
    pub(super) fn lookup_query(query: Option<&str>) -> Result<LookupQuery, ApiReply> {
        let query = query
            .filter(|q| !q.contains(['%', '+']))
            .ok_or_else(invalid)?;
        let fields = query
            .split('&')
            .map(|pair| pair.split_once('='))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(invalid)?;
        LookupQuery::from_fields(fields).map_err(|_| invalid())
    }

    pub(super) fn host_lookup_scope(&self, query: Option<&str>) -> Result<(), ApiReply> {
        Self::lookup_query(query)?
            .scope(OperationKind::Host, self.host.instance())
            .map(drop)
            .map_err(|_| invalid())
    }

    /// `GET /api/v1/operations/org/{id}` requires `instance_id` (matching this host) and
    /// `org_id`, with no `office_id` or unknown/repeated fields (web-api-v1 §5).
    pub(super) fn org_lookup_scope(&self, query: Option<&str>) -> Result<String, ApiReply> {
        match Self::lookup_query(query)?
            .scope(OperationKind::Org, self.host.instance())
            .map_err(|_| invalid())?
        {
            Scope::Organization { org_id, .. } => {
                if !org_office_launch::is_storage_id(&org_id) {
                    return Err(invalid());
                }
                Ok(org_id)
            }
            _ => Err(invalid()),
        }
    }

    /// A protected route needs a cookie+proof pair when authentication is enabled,
    /// or falls back to local-authless mode when this instance has auth disabled.
    pub(crate) fn caller_auth(&self, request: &Parts) -> Result<CallerAuth, ApiReply> {
        if let Some(pair) = PresentedPair::read(&self.cookie, &request.headers) {
            return Ok(CallerAuth::Pair(pair));
        }
        if !self.is_auth_enabled() {
            return Ok(CallerAuth::LocalAuthless);
        }
        Err(signed_out())
    }

    /// Routes that strictly require an explicit cookie+proof pair (e.g., sign-out).
    pub(super) fn pair(&self, request: &Parts) -> Result<PresentedPair, ApiReply> {
        PresentedPair::read(&self.cookie, &request.headers).ok_or_else(signed_out)
    }

    /// Inserts a successful reply into the cache on the host reader thread, under
    /// this connection's generation, before any later host frame is applied: a
    /// `session.ended` that follows the reply removes the entry it made.
    pub(crate) fn caching(
        &self,
        pair: &PresentedPair,
    ) -> impl FnOnce(&Result<BrowserSession, Failure>) + Send + 'static {
        let (cache, generation, pair) = (Arc::clone(&self.cache), self.host.generation(), *pair);
        move |reply| {
            if let Ok(session) = reply {
                cache.insert(
                    pair.cookie_digest,
                    pair.proof_digest,
                    session.clone(),
                    generation,
                );
            }
        }
    }

    /// The live session for a caller: cached or `session.validate` for a pair,
    /// or `browser.local_session` in local-authless mode.
    pub(crate) fn validate(&self, caller: CallerAuth) -> Result<BrowserSession, Failure> {
        match caller {
            CallerAuth::Pair(pair) => {
                let generation = self.host.generation();
                if let Some(session) = self.cache.lookup(
                    &pair.cookie_digest,
                    &pair.proof_digest,
                    generation,
                    now_ms(),
                ) {
                    return Ok(session);
                }
                self.host.request_then::<SessionValidate>(
                    &SessionValidateParams {
                        cookie_digest: pair.cookie_digest,
                        proof_digest: pair.proof_digest,
                        origin: self.origin.clone(),
                    },
                    self.caching(&pair),
                )
            }
            CallerAuth::LocalAuthless => {
                let reply = self
                    .host
                    .request::<BrowserLocalSession>(&BrowserLocalSessionParams {
                        origin: self.origin.clone(),
                    });
                if matches!(reply, Err(Failure::Denied)) {
                    self.set_auth_enabled(true);
                }
                reply
            }
        }
    }

    /// Sign-in and registration: mint a fresh pair, let the host create the
    /// session against its digests, then return the view with the proof and set
    /// the cookie. A refused call sets no cookie.
    pub(super) fn establish(
        &self,
        create: impl FnOnce(&PresentedPair) -> Result<BrowserSession, Failure>,
    ) -> ApiReply {
        let Ok(minted) = BrowserPair::mint() else {
            return unavailable();
        };
        let pair = PresentedPair {
            cookie_digest: minted.cookie_digest(),
            proof_digest: minted.proof_digest(),
        };
        let session = match create(&pair) {
            Ok(session) => session,
            Err(e) => return failure(e),
        };
        let Ok(proof) = SessionProof::parse(minted.proof().to_owned()) else {
            return unavailable();
        };
        match view(self.host.instance(), session) {
            Ok(view) => result(SessionEstablished::new(view, proof))
                .with_header(SET_COOKIE, self.cookie.set(&minted)),
            Err(e) => failure(e),
        }
    }

    pub(super) fn clear(&self, reply: ApiReply) -> ApiReply {
        reply.with_header(SET_COOKIE, self.cookie.clear())
    }

    /// Async validation for non-blocking office RPCs and SSE streams.
    pub(crate) async fn validate_async(
        &self,
        caller: CallerAuth,
    ) -> Result<BrowserSession, Failure> {
        match caller {
            CallerAuth::Pair(pair) => {
                let generation = self.host.generation();
                if let Some(session) = self.cache.lookup(
                    &pair.cookie_digest,
                    &pair.proof_digest,
                    generation,
                    now_ms(),
                ) {
                    return Ok(session);
                }
                self.revalidate_session_async(caller).await
            }
            CallerAuth::LocalAuthless => self.revalidate_session_async(caller).await,
        }
    }

    /// Re-validates directly with the host, bypassing the in-memory session cache.
    pub(crate) async fn revalidate_session_async(
        &self,
        caller: CallerAuth,
    ) -> Result<BrowserSession, Failure> {
        match caller {
            CallerAuth::Pair(pair) => {
                self.host
                    .request_then_async::<SessionValidate>(
                        &SessionValidateParams {
                            cookie_digest: pair.cookie_digest,
                            proof_digest: pair.proof_digest,
                            origin: self.origin.clone(),
                        },
                        Some(Box::new(self.caching(&pair))),
                    )
                    .await
            }
            CallerAuth::LocalAuthless => {
                let reply = self
                    .host
                    .request_async::<BrowserLocalSession>(&BrowserLocalSessionParams {
                        origin: self.origin.clone(),
                    })
                    .await;
                if matches!(reply, Err(Failure::Denied)) {
                    self.set_auth_enabled(true);
                }
                reply
            }
        }
    }
}
