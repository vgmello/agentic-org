//! Browser-facing authority checks. Never trust proxy headers or infer configuration.

use hyper::header::{HOST, ORIGIN};
use hyper::http::request::Parts;
use hyper::Method;

use org_host_control::listen::ListenOrigin;

use crate::origin::authority;

pub(crate) struct RequestAdmission {
    origin: ListenOrigin,
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
