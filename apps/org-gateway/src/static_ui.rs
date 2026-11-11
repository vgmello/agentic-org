//! Serve a validated UI snapshot with the response headers of each resource class.

use std::net::IpAddr;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::header::{
    HeaderMap, HeaderValue, ACCEPT, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, ETAG, HOST,
    IF_NONE_MATCH, STRICT_TRANSPORT_SECURITY,
};
use hyper::http::uri::Authority;
use hyper::{Method, Request, Response, StatusCode};

use crate::assets::{is_html, is_json, BuildFiles};
use crate::csp;
use crate::path::RequestPath;
use crate::ConfiguredOrigin;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Hsts {
    #[default]
    Off,
    On,
    Clear,
}

pub struct StaticUi {
    files: BuildFiles,
    origin: ConfiguredOrigin,
    hsts: Option<HeaderValue>,
}

impl StaticUi {
    pub fn new(files: BuildFiles, origin: ConfiguredOrigin, hsts: Hsts) -> Self {
        let hsts = if hsts_eligible(origin.is_https(), origin.authority()) {
            match hsts {
                Hsts::Off => None,
                Hsts::On => Some(HeaderValue::from_static("max-age=31536000")),
                Hsts::Clear => Some(HeaderValue::from_static("max-age=0")),
            }
        } else {
            None
        };
        Self {
            files,
            origin,
            hsts,
        }
    }

    pub fn serve<B>(&self, request: &Request<B>) -> Response<Full<Bytes>> {
        let mut response = match RequestPath::parse(request.uri().path()) {
            Err(_) => json_error(
                StatusCode::BAD_REQUEST,
                INVALID_PATH,
                request.method() == Method::HEAD,
            ),
            Ok(path) => self.serve_static(request.method(), request.headers(), &path),
        };
        self.finish_response(request.headers(), &mut response);
        response
    }

    pub(crate) fn serve_static(
        &self,
        method: &Method,
        headers: &HeaderMap,
        path: &RequestPath,
    ) -> Response<Full<Bytes>> {
        let head = method == Method::HEAD;
        if !matches!(*method, Method::GET | Method::HEAD) || path.is_api() {
            return json_error(StatusCode::NOT_FOUND, NOT_FOUND, head);
        }
        let key = if path.segments().is_empty() {
            "index.html".to_owned()
        } else {
            path.asset_key()
        };
        if let Some(bytes) = self.files.file(&key) {
            self.asset(&key, bytes, headers, head)
        } else if !path.is_app_asset() && is_navigation(headers) {
            let entry = self
                .files
                .file("index.html")
                .expect("startup validated index.html");
            self.asset("index.html", entry, headers, head)
        } else {
            json_error(StatusCode::NOT_FOUND, NOT_FOUND, head)
        }
    }

    fn asset(
        &self,
        key: &str,
        bytes: &Bytes,
        request: &HeaderMap,
        head: bool,
    ) -> Response<Full<Bytes>> {
        let tag = self.files.etag(key);
        let status = if tag.is_some_and(|tag| matches_etag(request, tag)) {
            StatusCode::NOT_MODIFIED
        } else {
            StatusCode::OK
        };
        let mut response = response(
            status,
            bytes.clone(),
            head,
            mime_type(key),
            cache_policy(key),
        );
        let headers = response.headers_mut();
        headers.insert(
            "content-security-policy",
            self.files
                .content_security_policy(key, self.origin.is_https())
                .expect("asset already came from the startup snapshot"),
        );
        if let Some(tag) = tag {
            headers.insert(ETAG, tag.clone());
        }
        response
    }

    pub(crate) fn finish_response(
        &self,
        request: &HeaderMap,
        response: &mut Response<Full<Bytes>>,
    ) {
        let headers = response.headers_mut();
        if !headers.contains_key("content-security-policy") {
            headers.insert("content-security-policy", csp::non_html_policy());
        }
        for (name, value) in [
            ("x-content-type-options", "nosniff"),
            ("referrer-policy", "same-origin"),
            ("x-frame-options", "DENY"),
            ("cross-origin-opener-policy", "same-origin"),
            ("cross-origin-resource-policy", "same-origin"),
            ("permissions-policy", "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), hid=(), bluetooth=()"),
        ] {
            headers.insert(name, HeaderValue::from_static(value));
        }
        let mut hosts = request.get_all(HOST).iter();
        if let Some(hsts) = &self.hsts {
            if hosts
                .next()
                .is_some_and(|host| host.as_bytes() == self.origin.authority().as_bytes())
                && hosts.next().is_none()
            {
                headers.insert(STRICT_TRANSPORT_SECURITY, hsts.clone());
            }
        }
    }
}

const NOT_FOUND: &[u8] = br#"{"error":{"code":"not_found","message":"Not found."}}"#;
const INVALID_PATH: &[u8] =
    br#"{"error":{"code":"invalid_request","message":"Invalid request path."}}"#;

fn json_error(status: StatusCode, body: &'static [u8], head: bool) -> Response<Full<Bytes>> {
    response(
        status,
        Bytes::from_static(body),
        head,
        "application/json; charset=utf-8",
        "no-store",
    )
}

pub(crate) fn response(
    status: StatusCode,
    bytes: Bytes,
    head: bool,
    mime: &'static str,
    cache: &'static str,
) -> Response<Full<Bytes>> {
    let length = bytes.len();
    let body = if head || status == StatusCode::NOT_MODIFIED {
        Bytes::new()
    } else {
        bytes
    };
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, mime)
        .header(CACHE_CONTROL, cache)
        .header(CONTENT_LENGTH, length)
        .body(Full::new(body))
        .expect("fixed response headers")
}

fn hsts_eligible(https: bool, authority: &str) -> bool {
    let Ok(authority) = authority.parse::<Authority>() else {
        return false;
    };
    let host = authority
        .host()
        .trim_end_matches('.')
        .trim_matches(['[', ']'])
        .to_ascii_lowercase();
    https && host != "localhost" && !host.ends_with(".localhost") && host.parse::<IpAddr>().is_err()
}

fn is_navigation(headers: &HeaderMap) -> bool {
    let mut mode = headers.get_all("sec-fetch-mode").iter();
    if let Some(value) = mode.next() {
        return value == "navigate" && mode.next().is_none();
    }
    if ["sec-fetch-site", "sec-fetch-dest", "sec-fetch-user"]
        .into_iter()
        .any(|header| headers.contains_key(header))
    {
        return false;
    }
    headers
        .get_all(ACCEPT)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|part| {
            let mut parts = part.split(';');
            let html = parts
                .next()
                .is_some_and(|media| media.trim().eq_ignore_ascii_case("text/html"));
            html && parts.all(|parameter| {
                let Some((name, value)) = parameter.split_once('=') else {
                    return true;
                };
                !name.trim().eq_ignore_ascii_case("q")
                    || value
                        .trim()
                        .parse::<f32>()
                        .is_ok_and(|quality| quality > 0.0 && quality <= 1.0)
            })
        })
}

fn matches_etag(headers: &HeaderMap, etag: &HeaderValue) -> bool {
    headers
        .get_all(IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|part| {
            let part = part.trim();
            part == "*" || part.strip_prefix("W/").unwrap_or(part).as_bytes() == etag.as_bytes()
        })
}

fn cache_policy(key: &str) -> &'static str {
    if is_json(key) {
        "no-store"
    } else if !is_html(key) && immutable_chunk(key) {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

fn immutable_chunk(key: &str) -> bool {
    let Some(relative) = key.strip_prefix("_app/immutable/") else {
        return false;
    };
    if relative.starts_with("workers/") {
        return false;
    }
    let file = relative.rsplit('/').next().unwrap_or_default();
    let Some((stem, extension)) = file.rsplit_once('.') else {
        return false;
    };
    // SvelteKit's client build uses bare hashes for JS chunks and appends a hash
    // to named entries, nodes and other bundled assets.
    let hash = match stem.rsplit_once('.') {
        Some((_, hash)) => hash,
        None if relative.starts_with("chunks/") && extension == "js" => stem,
        None => return false,
    };
    hash.len() == 8
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn mime_type(key: &str) -> &'static str {
    let extension = key
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    match extension.as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("txt") => "text/plain; charset=utf-8",
        Some("webmanifest") => "application/manifest+json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    const HTML: &[u8] = b"<!doctype html><script>0</script>";
    const HTML_CSP: &str = "default-src 'none'; script-src 'self' 'sha256-X+zrZv/IbzjZUnhsbWlsecLbwjndTpG0ZynXOif7V+k='; style-src 'self' 'unsafe-inline'; img-src 'self'; font-src 'self'; connect-src 'self'; manifest-src 'self'; worker-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'";
    const ASSET_CSP: &str =
        "default-src 'none'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'";

    fn ui(origin: &str, hsts: Hsts) -> StaticUi {
        let files = BuildFiles::from_files([
            ("index.html".into(), HTML.to_vec()),
            ("favicon.svg".into(), b"<svg/>".to_vec()),
            (
                "_app/immutable/chunks/app.a1b2C3d4.js".into(),
                b"export {};".to_vec(),
            ),
            (
                "_app/immutable/chunks/B0ewgYNx.js".into(),
                b"export {};".to_vec(),
            ),
            (
                "_app/immutable/chunks/fixed.js".into(),
                b"export {};".to_vec(),
            ),
            (
                "_app/immutable/workers/w-a1b2C3d4.js".into(),
                b"onconnect = null;".to_vec(),
            ),
            ("_app/version.json".into(), b"{}".to_vec()),
        ])
        .unwrap();
        StaticUi::new(files, ConfiguredOrigin::parse(origin).unwrap(), hsts)
    }

    fn request(method: Method, path: &str, headers: &[(&str, &str)]) -> Request<()> {
        let mut request = Request::builder().method(method).uri(path);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        request.body(()).unwrap()
    }

    fn bytes(response: Response<Full<Bytes>>) -> Bytes {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(response.into_body().collect())
            .unwrap()
            .to_bytes()
    }

    fn class_headers(headers: &HeaderMap, csp: &str, cache: &str, content_type: &str) {
        for (name, value) in [
            ("content-security-policy", csp), ("cache-control", cache), ("content-type", content_type),
            ("referrer-policy", "same-origin"), ("x-content-type-options", "nosniff"),
            ("x-frame-options", "DENY"), ("cross-origin-opener-policy", "same-origin"),
            ("cross-origin-resource-policy", "same-origin"),
            ("permissions-policy", "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), hid=(), bluetooth=()"),
        ] {
            assert_eq!(headers.get(name).unwrap(), value, "{name}");
        }
        assert!(!headers.contains_key("access-control-allow-origin"));
    }

    #[test]
    fn html_never_uses_conditional_validators_and_has_transport_specific_csp() {
        for (origin, expected) in [
            ("http://127.0.0.1:4321", HTML_CSP.to_owned()),
            (
                "https://gateway.test:4321",
                format!("{HTML_CSP}; upgrade-insecure-requests"),
            ),
        ] {
            let ui = ui(origin, Hsts::Off);
            for path in ["/", "/nested/route"] {
                let response = ui.serve(&request(
                    Method::GET,
                    path,
                    &[
                        ("sec-fetch-mode", "navigate"),
                        ("if-none-match", "*"),
                        ("if-modified-since", "Wed, 21 Oct 2015 07:28:00 GMT"),
                    ],
                ));
                assert_eq!(response.status(), StatusCode::OK);
                class_headers(
                    response.headers(),
                    &expected,
                    "no-cache",
                    "text/html; charset=utf-8",
                );
                assert!(!response.headers().contains_key(ETAG));
                assert!(!response.headers().contains_key("last-modified"));
                assert_eq!(bytes(response).as_ref(), HTML);
            }
        }
    }

    #[test]
    fn only_hashed_non_worker_chunks_are_immutable_and_304_retains_its_class() {
        let ui = ui("http://127.0.0.1:4321", Hsts::Off);
        for (path, cache, content_type) in [
            (
                "/_app/immutable/chunks/app.a1b2C3d4.js",
                "public, max-age=31536000, immutable",
                "text/javascript; charset=utf-8",
            ),
            (
                "/_app/immutable/chunks/B0ewgYNx.js",
                "public, max-age=31536000, immutable",
                "text/javascript; charset=utf-8",
            ),
            (
                "/_app/immutable/chunks/fixed.js",
                "no-cache",
                "text/javascript; charset=utf-8",
            ),
            (
                "/_app/immutable/workers/w-a1b2C3d4.js",
                "no-cache",
                "text/javascript; charset=utf-8",
            ),
            ("/favicon.svg", "no-cache", "image/svg+xml"),
        ] {
            let first = ui.serve(&request(Method::GET, path, &[]));
            assert_eq!(first.status(), StatusCode::OK);
            class_headers(first.headers(), ASSET_CSP, cache, content_type);
            let tag = first.headers()[ETAG].to_str().unwrap();
            let conditional = format!("W/{tag}");
            let second = ui.serve(&request(
                Method::GET,
                path,
                &[("if-none-match", &conditional)],
            ));
            assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
            assert_eq!(second.headers(), first.headers());
            assert!(bytes(second).is_empty());
        }
        let version = ui.serve(&request(
            Method::GET,
            "/_app/version.json",
            &[("if-none-match", "*")],
        ));
        assert_eq!(version.status(), StatusCode::OK);
        assert!(!version.headers().contains_key(ETAG));
        class_headers(
            version.headers(),
            ASSET_CSP,
            "no-store",
            "application/json; charset=utf-8",
        );
    }

    #[test]
    fn only_navigation_in_nonreserved_paths_gets_the_spa() {
        let ui = ui("http://127.0.0.1:4321", Hsts::Off);
        for path in ["/api", "/api/v1", "/api/v2/x", "/_app", "/_app/missing.js"] {
            let response = ui.serve(&request(
                Method::GET,
                path,
                &[("sec-fetch-mode", "navigate")],
            ));
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            class_headers(
                response.headers(),
                ASSET_CSP,
                "no-store",
                "application/json; charset=utf-8",
            );
        }
        for path in ["/_apple", "/apiary", "/nested/route"] {
            let response = ui.serve(&request(
                Method::HEAD,
                path,
                &[("accept", "text/html,application/xhtml+xml")],
            ));
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
            assert_eq!(response.headers()[CONTENT_LENGTH], HTML.len().to_string());
            assert!(bytes(response).is_empty());
        }
        for (method, headers) in [
            (Method::GET, vec![]),
            (
                Method::GET,
                vec![("sec-fetch-mode", "no-cors"), ("accept", "text/html")],
            ),
            (
                Method::GET,
                vec![("sec-fetch-dest", "image"), ("accept", "text/html")],
            ),
            (Method::GET, vec![("accept", "text/html;q=0,*/*;q=1")]),
            (Method::POST, vec![("sec-fetch-mode", "navigate")]),
        ] {
            let response = ui.serve(&request(method, "/missing", &headers));
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            class_headers(
                response.headers(),
                ASSET_CSP,
                "no-store",
                "application/json; charset=utf-8",
            );
        }
    }

    #[test]
    fn raw_path_refusals_have_the_complete_json_error_header_set() {
        let ui = ui("http://127.0.0.1:4321", Hsts::Off);
        for path in ["/bad%zz", "/bad//path", "/bad/%2e"] {
            let response = ui.serve(&request(
                Method::GET,
                path,
                &[("sec-fetch-mode", "navigate")],
            ));
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            class_headers(
                response.headers(),
                ASSET_CSP,
                "no-store",
                "application/json; charset=utf-8",
            );
            assert_eq!(response.headers().len(), 10);
            assert_eq!(bytes(response).as_ref(), INVALID_PATH);
        }
    }

    #[test]
    fn hsts_requires_https_an_eligible_name_and_an_exact_single_host() {
        for authority in [
            "localhost",
            "LOCALHOST",
            "localhost.",
            "localhost:8443",
            "x.localhost",
            "x.localhost:8443",
            "127.0.0.1",
            "127.0.0.1:8443",
            "[::1]",
            "[::1]:8443",
            "192.168.1.10",
            "192.168.1.10:8443",
            "[fd00::1]",
            "[fd00::1]:8443",
        ] {
            assert!(!hsts_eligible(true, authority), "{authority}");
        }
        assert!(!hsts_eligible(false, "gateway.test:8443"));
        assert!(hsts_eligible(true, "gateway.test:8443"));
        for (mode, expected) in [
            (Hsts::Off, None),
            (Hsts::On, Some("max-age=31536000")),
            (Hsts::Clear, Some("max-age=0")),
        ] {
            let ui = ui("https://gateway.test:8443", mode);
            for (host, expected) in [
                ("gateway.test:8443", expected),
                ("alias.test:8443", None),
                ("gateway.test", None),
            ] {
                let response = ui.serve(&request(Method::GET, "/api/missing", &[("host", host)]));
                assert_eq!(
                    response
                        .headers()
                        .get(STRICT_TRANSPORT_SECURITY)
                        .map(|v| v.to_str().unwrap()),
                    expected
                );
            }
        }
        let ui = ui("https://gateway.test:8443", Hsts::On);
        let response = ui.serve(&request(
            Method::GET,
            "/",
            &[("host", "gateway.test:8443"), ("host", "gateway.test:8443")],
        ));
        assert!(!response.headers().contains_key(STRICT_TRANSPORT_SECURITY));
    }
}
