//! The production `/api/v1` backend until stored browser sessions land (FC-159,
//! ADR-0013). It serves only the public constant `GET /api/v1/version`. Every
//! other `/api/v1` path, including the `/api/v1/session` routes, is a missing
//! API route and gets a JSON 404 (web-api-v1 §2: "Missing API routes are JSON
//! 404, never SPA HTML"). Nothing forwards to the host or a local socket, and no
//! request can sign in.
use crate::{
    body::BusinessBody,
    path::RequestPath,
    service::{ApiBackend, ApiReply, PreparedCall},
};
use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::gateway::{GatewayErrorCode, Version};

pub struct VersionApi;

pub enum Route {
    Version,
}

fn invalid() -> ApiReply {
    ApiReply::error(
        StatusCode::BAD_REQUEST,
        GatewayErrorCode::InvalidRequest,
        "Invalid request.",
    )
}

impl ApiBackend for VersionApi {
    type Route = Route;
    type Call = Route;
    fn route(&self, request: &Parts, path: &RequestPath) -> Result<Route, ApiReply> {
        if path.segments() != ["api", "v1", "version"] {
            return Err(ApiReply::error(
                StatusCode::NOT_FOUND,
                GatewayErrorCode::NotFound,
                "Not found.",
            ));
        }
        if request.method != Method::GET || request.uri.query().is_some() {
            return Err(invalid());
        }
        Ok(Route::Version)
    }
    fn prepare(&self, route: Route, body: BusinessBody) -> Result<PreparedCall<Route>, ApiReply> {
        if !body.as_bytes().is_empty() {
            return Err(invalid());
        }
        Ok(PreparedCall::read(route, 0))
    }
    fn execute(&self, call: Route) -> ApiReply {
        match call {
            Route::Version => ApiReply::json(
                StatusCode::OK,
                serde_json::json!({ "result": Version::default() }),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        assets::BuildFiles,
        ingress::RequestAdmission,
        service::{ApiLimits, Gateway},
        Hsts, StaticUi,
    };
    use http_body_util::{BodyExt, Full};
    use hyper::{body::Bytes, header, Request};
    use org_host_control::listen::ListenOrigin;

    const ORIGIN: &str = "http://127.0.0.1:8080";
    const HOST: &str = "127.0.0.1:8080";

    fn service() -> Gateway<VersionApi> {
        let origin = ListenOrigin::parse(ORIGIN).unwrap();
        let files = BuildFiles::from_files([("index.html".into(), b"<script>0</script>".to_vec())])
            .unwrap();
        Gateway::new(
            StaticUi::new(files, origin.clone(), Hsts::Off),
            VersionApi,
            ApiLimits::default(),
            RequestAdmission::new(origin),
        )
        .unwrap()
    }

    fn send(
        service: &Gateway<VersionApi>,
        method: Method,
        target: &str,
        body: &str,
    ) -> (StatusCode, header::HeaderMap, serde_json::Value) {
        let mut builder = Request::builder().method(method.clone()).uri(target);
        builder = builder.header("host", HOST);
        if method != Method::GET {
            builder = builder.header("origin", ORIGIN);
        }
        let request = builder
            .body(Full::new(Bytes::copy_from_slice(body.as_bytes())))
            .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let response = runtime.block_on(service.respond(request));
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = runtime
            .block_on(response.into_body().collect())
            .unwrap()
            .to_bytes();
        (
            status,
            headers,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[test]
    fn version_is_the_public_constant() {
        let (status, headers, body) = send(&service(), Method::GET, "/api/v1/version", "");
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            serde_json::json!({"result":{"api_version":1,"contract_revision":org_proto::gateway::CONTRACT_REVISION}})
        );
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        assert!(headers.get(header::SET_COOKIE).is_none());
    }

    #[test]
    fn version_refuses_other_methods_queries_and_bodies() {
        let service = service();
        for (method, target, body) in [
            (Method::POST, "/api/v1/version", "{}"),
            (Method::GET, "/api/v1/version?x=1", ""),
        ] {
            let (status, _, body) = send(&service, method, target, body);
            assert_eq!(status, StatusCode::BAD_REQUEST, "{target}");
            assert_eq!(body["error"]["code"], "invalid_request");
        }
    }

    /// Browser sign-in waits for stored sessions (FC-159): every session route
    /// is a missing API route, a JSON 404 that sets no cookie.
    #[test]
    fn session_and_other_api_routes_are_json_404_and_never_sign_in() {
        let service = service();
        for (method, target, body) in [
            (
                Method::POST,
                "/api/v1/session/sign-in",
                r#"{"login":"OWNER","password":"generated unit credential"}"#,
            ),
            (Method::GET, "/api/v1/session", ""),
            (Method::POST, "/api/v1/session/refresh", "{}"),
            (Method::POST, "/api/v1/session/sign-out", "{}"),
            (Method::GET, "/api/v1/capabilities", ""),
            (Method::GET, "/api/v1/organizations", ""),
            (Method::GET, "/api/v1/tasks", ""),
        ] {
            let (status, headers, body) = send(&service, method, target, body);
            assert_eq!(status, StatusCode::NOT_FOUND, "{target}");
            assert_eq!(body["error"]["code"], "not_found", "{target}");
            assert!(headers.get(header::SET_COOKIE).is_none(), "{target}");
            assert_eq!(headers[header::CACHE_CONTROL], "no-store", "{target}");
        }
    }

    #[test]
    fn the_web_process_sends_no_office_channel_request() {
        // Source-level guard: the host connection carries only hello, tls.fetch
        // and the serving notification.
        for (name, source) in [
            ("api.rs", include_str!("api.rs")),
            ("host.rs", include_str!("host.rs")),
            ("process.rs", include_str!("process.rs")),
        ] {
            for forbidden in [
                ["office", ".open_channel"].concat(),
                ["Office", "OpenChannel"].concat(),
                ["selected", "_tasks"].concat(),
                ["org", "_client"].concat(),
                ["account", ".verify"].concat(),
                ["session", ".renew"].concat(),
            ] {
                assert!(!source.contains(&forbidden), "{name} contains {forbidden}");
            }
        }
    }
}
