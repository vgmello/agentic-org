//! Browser-facing authority checks. Never trust proxy headers or infer configuration.

use hyper::header::{CONTENT_TYPE, HOST, ORIGIN};
use hyper::http::request::Parts;
use hyper::Method;

use org_host_control::listen::ListenOrigin;

use crate::origin::authority;
use crate::path::RequestPath;

pub(crate) struct RequestAdmission {
    origin: ListenOrigin,
}

impl RequestAdmission {
    pub(crate) fn new(origin: ListenOrigin) -> Self {
        Self { origin }
    }

    /// DNS-rebinding defence for every request, before routing or body polling.
    pub(crate) fn accepts_target_and_host(&self, request: &Parts) -> bool {
        // An origin server admits origin-form only. Refuse absolute-form even
        // when its authority agrees, avoiding HTTP's Host-override semantics.
        if request.method == Method::CONNECT
            || request.uri.authority().is_some()
            || !request.uri.path().starts_with('/')
        {
            return false;
        }
        let mut hosts = request.headers.get_all(HOST).iter();
        hosts.next().is_some_and(|host| {
            // Only hostname/IPv6 hex letters can differ in case; the serialized
            // port remains byte-exact, including omission of the default port.
            host.as_bytes()
                .eq_ignore_ascii_case(authority(&self.origin).as_bytes())
        }) && hosts.next().is_none()
    }

    /// The `/api/v1` request rule (web-api-v1 §2 "Request shape", §4), applied
    /// after Host and before routing or reading the body. Order: `OPTIONS`,
    /// `Sec-Fetch-Site`, `Origin`, then the POST/PATCH media type. The one exception
    /// to the JSON media type is the attachment upload (`POST
    /// /api/v1/organizations/{org}/attachments`), whose route admits only its own
    /// closed list of file types; it passes every other row here.
    pub(crate) fn check_api(&self, request: &Parts, path: &RequestPath) -> Result<(), ApiRefusal> {
        if request.method == Method::OPTIONS {
            return Err(ApiRefusal::Options);
        }
        // Defence in depth: a browser that names the request cross-site (or
        // same-site from another origin) is refused even with a matching Origin.
        let mut fetch_sites = request.headers.get_all("sec-fetch-site").iter();
        if let Some(site) = fetch_sites.next() {
            if !matches!(site.as_bytes(), b"same-origin" | b"none") || fetch_sites.next().is_some()
            {
                return Err(ApiRefusal::CrossSite);
            }
        }
        if !self.accepts_api_origin(request) {
            return Err(ApiRefusal::Origin);
        }
        let json_body = match request.method {
            Method::POST => !is_attachment_upload(path),
            Method::PATCH => true,
            _ => false,
        };
        if json_body && !json_content_type(request) {
            return Err(ApiRefusal::MediaType);
        }
        Ok(())
    }

    /// API reads without Origin still require the route's own proof check.
    pub(crate) fn accepts_api_origin(&self, request: &Parts) -> bool {
        let mut origins = request.headers.get_all(ORIGIN).iter();
        match origins.next() {
            None => matches!(request.method, Method::GET | Method::HEAD),
            Some(origin) => {
                origin.as_bytes() == self.origin.as_str().as_bytes() && origins.next().is_none()
            }
        }
    }
}

/// Why an `/api/v1` request is refused before routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiRefusal {
    /// 405: no preflight is answered; no `Access-Control-*` header is sent.
    Options,
    /// 403: `Sec-Fetch-Site` other than `same-origin` or `none`.
    CrossSite,
    /// 403: an unsafe method without one exact `Origin`, or a mismatching one.
    Origin,
    /// 415: a POST whose `Content-Type` is not JSON.
    MediaType,
}

/// Exactly `/api/v1/organizations/{org}/attachments`: the streamed upload route.
fn is_attachment_upload(path: &RequestPath) -> bool {
    matches!(path.segments(), [api, v1, orgs, _, attachments]
        if api == "api" && v1 == "v1" && orgs == "organizations" && attachments == "attachments")
}

/// One `Content-Type` of exactly `application/json`, optionally with
/// `charset=utf-8` and nothing else. Blocks form and `text/plain` CSRF.
fn json_content_type(request: &Parts) -> bool {
    let mut types = request.headers.get_all(CONTENT_TYPE).iter();
    let (Some(value), None) = (types.next(), types.next()) else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    let mut parts = value.split(';');
    let essence = parts.next().unwrap_or_default().trim();
    if !essence.eq_ignore_ascii_case("application/json") {
        return false;
    }
    match (parts.next(), parts.next()) {
        (None, _) => true,
        (Some(parameter), None) => parameter.split_once('=').is_some_and(|(name, value)| {
            name.trim().eq_ignore_ascii_case("charset")
                && value.trim().trim_matches('"').eq_ignore_ascii_case("utf-8")
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::Request;

    fn parts(method: Method, target: &str, hosts: &[&str], origins: &[&str]) -> Parts {
        let mut builder = Request::builder().method(method).uri(target);
        for host in hosts {
            builder = builder.header(HOST, *host);
        }
        for origin in origins {
            builder = builder.header(ORIGIN, *origin);
        }
        builder.body(()).unwrap().into_parts().0
    }

    #[test]
    fn host_is_single_case_insensitive_with_an_exact_serialized_port() {
        for (origin, valid, invalid) in [
            ("https://gateway.test", "GATEWAY.TEST", "gateway.test:443"),
            ("http://gateway.test", "GATEWAY.TEST", "gateway.test:80"),
            (
                "https://gateway.test:8443",
                "GATEWAY.TEST:8443",
                "gateway.test:08443",
            ),
            (
                "https://[2001:db8::1]",
                "[2001:DB8::1]",
                "[2001:db8::1]:443",
            ),
            ("http://127.0.0.1:1234", "127.0.0.1:1234", "localhost:1234"),
        ] {
            let admission = RequestAdmission::new(ListenOrigin::parse(origin).unwrap());
            assert!(admission.accepts_target_and_host(&parts(Method::GET, "/", &[valid], &[])));
            for hosts in [
                vec![],
                vec![""],
                vec![invalid],
                vec![valid, valid],
                vec![valid, invalid],
            ] {
                assert!(
                    !admission.accepts_target_and_host(&parts(Method::GET, "/", &hosts, &[])),
                    "{hosts:?}"
                );
            }
        }
    }

    #[test]
    fn only_origin_form_is_admitted_even_when_absolute_authority_agrees() {
        let admission = RequestAdmission::new(ListenOrigin::parse("https://gateway.test").unwrap());
        for (method, target) in [
            (Method::GET, "https://gateway.test/"),
            (Method::GET, "http://attacker.test/"),
            (Method::CONNECT, "gateway.test:443"),
            (Method::OPTIONS, "*"),
            (Method::CONNECT, "/"),
        ] {
            assert!(
                !admission.accepts_target_and_host(&parts(method, target, &["gateway.test"], &[])),
                "{target}"
            );
        }
        // Query-only targets cannot be represented by the HTTP URI parser.
        assert!("?query".parse::<hyper::Uri>().is_err());
        assert!(admission.accepts_target_and_host(&parts(
            Method::GET,
            "/?query",
            &["gateway.test"],
            &[]
        )));
    }

    #[test]
    fn origin_is_exact_single_and_missing_is_only_allowed_for_get_or_head() {
        let origin = "https://gateway.test";
        let admission = RequestAdmission::new(ListenOrigin::parse(origin).unwrap());
        for method in [
            Method::GET,
            Method::HEAD,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ] {
            assert!(admission.accepts_api_origin(&parts(method.clone(), "/api", &[], &[origin])));
            assert_eq!(
                admission.accepts_api_origin(&parts(method.clone(), "/api", &[], &[])),
                matches!(method, Method::GET | Method::HEAD)
            );
            for origins in [
                vec![""],
                vec!["null"],
                vec!["https://attacker.test"],
                vec!["https://GATEWAY.TEST"],
                vec!["HTTPS://gateway.test"],
                vec!["https://gateway.test/"],
                vec!["https://gateway.test:443"],
                vec![origin, origin],
                vec![origin, "null"],
                vec!["https://gateway.test https://attacker.test"],
            ] {
                assert!(
                    !admission.accepts_api_origin(&parts(method.clone(), "/api", &[], &origins)),
                    "{method} {origins:?}"
                );
            }
        }
    }

    fn api(method: Method, headers: &[(&str, &str)]) -> Parts {
        api_at(method, "/api/v1/session", headers)
    }

    fn api_at(method: Method, target: &str, headers: &[(&str, &str)]) -> Parts {
        let mut builder = Request::builder().method(method).uri(target);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(()).unwrap().into_parts().0
    }

    #[test]
    fn the_api_request_rule_refuses_each_row_before_routing() {
        let origin = "http://127.0.0.1:8080";
        let admission = RequestAdmission::new(ListenOrigin::parse(origin).unwrap());
        let json = ("content-type", "application/json");
        let o = ("origin", origin);
        for (method, headers, expected) in [
            (Method::POST, vec![o, json], Ok(())),
            (
                Method::POST,
                vec![o, ("content-type", "application/json; charset=utf-8")],
                Ok(()),
            ),
            (
                Method::POST,
                vec![o, ("content-type", "Application/JSON;Charset=\"UTF-8\"")],
                Ok(()),
            ),
            (Method::GET, vec![], Ok(())),
            (Method::GET, vec![("sec-fetch-site", "same-origin")], Ok(())),
            (Method::GET, vec![("sec-fetch-site", "none")], Ok(())),
            (
                Method::OPTIONS,
                vec![o, ("access-control-request-method", "POST")],
                Err(ApiRefusal::Options),
            ),
            (Method::OPTIONS, vec![], Err(ApiRefusal::Options)),
            (
                Method::GET,
                vec![("sec-fetch-site", "cross-site")],
                Err(ApiRefusal::CrossSite),
            ),
            (
                Method::POST,
                vec![o, json, ("sec-fetch-site", "same-site")],
                Err(ApiRefusal::CrossSite),
            ),
            (
                Method::GET,
                vec![
                    ("sec-fetch-site", "same-origin"),
                    ("sec-fetch-site", "same-origin"),
                ],
                Err(ApiRefusal::CrossSite),
            ),
            (Method::POST, vec![json], Err(ApiRefusal::Origin)),
            (
                Method::POST,
                vec![("origin", "null"), json],
                Err(ApiRefusal::Origin),
            ),
            (
                Method::POST,
                vec![("origin", "http://attacker.test"), json],
                Err(ApiRefusal::Origin),
            ),
            (Method::POST, vec![o, o, json], Err(ApiRefusal::Origin)),
            (
                Method::GET,
                vec![("origin", "http://attacker.test")],
                Err(ApiRefusal::Origin),
            ),
            (Method::POST, vec![o], Err(ApiRefusal::MediaType)),
            (
                Method::POST,
                vec![o, ("content-type", "text/plain")],
                Err(ApiRefusal::MediaType),
            ),
            (
                Method::POST,
                vec![o, ("content-type", "application/x-www-form-urlencoded")],
                Err(ApiRefusal::MediaType),
            ),
            (
                Method::POST,
                vec![o, ("content-type", "multipart/form-data; boundary=x")],
                Err(ApiRefusal::MediaType),
            ),
            (
                Method::POST,
                vec![o, ("content-type", "application/json; charset=latin1")],
                Err(ApiRefusal::MediaType),
            ),
            (
                Method::POST,
                vec![o, ("content-type", "application/json; charset=utf-8; x=y")],
                Err(ApiRefusal::MediaType),
            ),
            (
                Method::POST,
                vec![o, ("content-type", "application/jsonx")],
                Err(ApiRefusal::MediaType),
            ),
            (
                Method::POST,
                vec![o, json, json],
                Err(ApiRefusal::MediaType),
            ),
        ] {
            let request = api(method.clone(), &headers);
            let path = RequestPath::parse(request.uri.path()).unwrap();
            assert_eq!(
                admission.check_api(&request, &path),
                expected,
                "{method} {headers:?}"
            );
        }
    }

    #[test]
    fn only_the_exact_upload_path_skips_the_json_rule_and_patch_needs_json() {
        let origin = "http://127.0.0.1:8080";
        let admission = RequestAdmission::new(ListenOrigin::parse(origin).unwrap());
        let check = |method: Method, target: &str, headers: &[(&str, &str)]| {
            let request = api_at(method, target, headers);
            let path = RequestPath::parse(request.uri.path()).unwrap();
            admission.check_api(&request, &path)
        };
        let upload = "/api/v1/organizations/org-1/attachments";
        let o = ("origin", origin);
        for media in [
            "application/pdf",
            "image/png",
            "text/html",
            "multipart/form-data",
        ] {
            // The route decides the file type; the ingress rule still admits it.
            assert_eq!(
                check(Method::POST, upload, &[o, ("content-type", media)]),
                Ok(())
            );
        }
        // Origin and Sec-Fetch-Site still apply to the upload (CSRF).
        assert_eq!(
            check(Method::POST, upload, &[("content-type", "image/png")]),
            Err(ApiRefusal::Origin)
        );
        assert_eq!(
            check(
                Method::POST,
                upload,
                &[
                    o,
                    ("content-type", "image/png"),
                    ("sec-fetch-site", "cross-site")
                ]
            ),
            Err(ApiRefusal::CrossSite)
        );
        for target in [
            "/api/v1/organizations/org-1/attachments/0193a5c0-0000-7000-8000-000000000001",
            "/api/v1/organizations/org-1/attachment",
            "/api/v1/organizations/org-1/files",
            "/api/v1/organizations/attachments",
            "/api/v2/organizations/org-1/attachments",
        ] {
            assert_eq!(
                check(Method::POST, target, &[o, ("content-type", "image/png")]),
                Err(ApiRefusal::MediaType),
                "{target}"
            );
        }
        let file = "/api/v1/organizations/org-1/files/0193a5c0-0000-7000-8000-000000000001";
        assert_eq!(
            check(
                Method::PATCH,
                file,
                &[o, ("content-type", "application/json")]
            ),
            Ok(())
        );
        for headers in [vec![o], vec![o, ("content-type", "text/plain")]] {
            assert_eq!(
                check(Method::PATCH, file, &headers),
                Err(ApiRefusal::MediaType)
            );
        }
        assert_eq!(
            check(Method::PATCH, upload, &[o, ("content-type", "image/png")]),
            Err(ApiRefusal::MediaType)
        );
    }
}
