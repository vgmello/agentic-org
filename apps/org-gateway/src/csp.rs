//! Build-time policy literals and startup hashing of the exact HTML we serve.

use std::collections::BTreeSet;

use base64::prelude::*;
use hyper::header::HeaderValue;
use scraper::{ElementRef, Html};
use sha2::{Digest, Sha256};

use crate::assets::BuildError;

const HTML_TEMPLATE: &str = include_str!("../../../config/csp/html.txt");
const NON_HTML: &str = include_str!("../../../config/csp/non-html.txt");

pub(crate) fn non_html_policy() -> HeaderValue {
    HeaderValue::from_static(policy_line(NON_HTML))
}

fn policy_line(source: &'static str) -> &'static str {
    source.strip_suffix('\n').unwrap_or(source)
}

pub(crate) fn html_policy(html: &str) -> Result<HeaderValue, BuildError> {
    let document = Html::parse_document(html);
    let mut hashes = BTreeSet::new();
    for element in document.tree.nodes().filter_map(ElementRef::wrap) {
        let node = element.value();
        for (name, value) in node.attrs() {
            if name.starts_with("on") {
                return Err(BuildError::InlineEventHandler);
            }
            if contains_javascript_url(name, value) {
                return Err(BuildError::JavascriptUrl);
            }
        }
        if node.name() == "meta"
            && node
                .attr("http-equiv")
                .is_some_and(|value| value.eq_ignore_ascii_case("content-security-policy"))
        {
            return Err(BuildError::CspMetaTag);
        }
        if node.name() == "script" && node.attr("src").is_none() {
            let mut digest = Sha256::new();
            for text in element.text() {
                digest.update(text.as_bytes());
            }
            hashes.insert(format!(
                "'sha256-{}'",
                BASE64_STANDARD.encode(digest.finalize())
            ));
        }
    }
    if hashes.is_empty() {
        return Err(BuildError::MissingInlineScript);
    }
    let hashes = hashes.into_iter().collect::<Vec<_>>().join(" ");
    let value = policy_line(HTML_TEMPLATE).replace("{script_hashes}", &hashes);
    HeaderValue::from_str(&value).map_err(|_| BuildError::InvalidPolicy)
}

fn contains_javascript_url(name: &str, value: &str) -> bool {
    match name {
        "srcset" | "imagesrcset" => value.split(',').any(is_javascript_url),
        "ping" | "archive" => value.split_ascii_whitespace().any(is_javascript_url),
        "action" | "background" | "cite" | "classid" | "codebase" | "data" | "formaction"
        | "href" | "longdesc" | "manifest" | "poster" | "profile" | "src" | "usemap" => {
            is_javascript_url(value)
        }
        _ => false,
    }
}

fn is_javascript_url(value: &str) -> bool {
    let mut prefix = value
        .bytes()
        .skip_while(|byte| *byte <= b' ')
        .filter(|byte| !matches!(byte, b'\t' | b'\r' | b'\n'));
    b"javascript:".iter().all(|expected| {
        prefix
            .next()
            .is_some_and(|byte| byte.eq_ignore_ascii_case(expected))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECTED_HTML: &str = "default-src 'none'; script-src 'self' {script_hashes}; style-src 'self' 'unsafe-inline'; img-src 'self'; font-src 'self'; connect-src 'self'; manifest-src 'self'; worker-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'";
    const EXPECTED_NON_HTML: &str =
        "default-src 'none'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'";

    #[test]
    fn compiled_policy_sources_match_the_reviewed_literals() {
        assert_eq!(policy_line(HTML_TEMPLATE), EXPECTED_HTML);
        assert_eq!(policy_line(NON_HTML), EXPECTED_NON_HTML);
        assert_eq!(non_html_policy(), EXPECTED_NON_HTML);
    }

    #[test]
    fn startup_hashes_the_html_parsers_exact_utf8_script_text() {
        let html = "<!doctype html><script src='/other.js'></script>\
                    <script>\r\n\t\t{ console.log(\"café\"); }\r\t</script>";
        // printf '\n\t\t{ console.log("café"); }\n\t' | openssl dgst -sha256 -binary | base64
        let expected_hash = "'sha256-faz4EKf+LfhF4xUf6evxoqF3SVxQQpo9t9KMOipFvrI='";
        assert_eq!(
            html_policy(html).unwrap(),
            EXPECTED_HTML.replace("{script_hashes}", expected_hash)
        );
    }

    #[test]
    fn invalid_documents_are_refused_before_their_bytes_can_be_served() {
        assert_eq!(
            html_policy("<script src='app.js'></script>"),
            Err(BuildError::MissingInlineScript)
        );
        for html in [
            "<script>bootstrap()</script><button ONCLICK='go()'>",
            "<script>bootstrap()</script><template><button onfocus='go()'></template>",
        ] {
            assert_eq!(html_policy(html), Err(BuildError::InlineEventHandler));
        }
        for html in [
            "<script>bootstrap()</script><a href='JaVa&#9;Script:go()'>",
            "<script>bootstrap()</script><svg><a xlink:href='javascript:go()'></a></svg>",
            "<script>bootstrap()</script><img srcset='first.png 1x, javascript:go() 2x'>",
        ] {
            assert_eq!(html_policy(html), Err(BuildError::JavascriptUrl));
        }
        assert_eq!(html_policy("<meta http-equiv='Content-Security-Policy' content=\"default-src 'none'\"><script>bootstrap()</script>"), Err(BuildError::CspMetaTag));
    }
}
