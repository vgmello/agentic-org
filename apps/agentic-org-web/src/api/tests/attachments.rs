//! `TASK-98.5`: the attachment and file routes through the full gateway (ingress rule,
//! routing, session pair, header checks), against the scripted host. What each slice sends
//! to the office is covered against a scripted office in `attachment_routes/tests.rs`.

use super::*;

fn upload_path() -> String {
    format!("/api/v1/organizations/{ORG}/attachments")
}

const FILE: &str = "0193a5c0-0000-7000-8000-0000000000bb";

/// Sends `body` with exactly `headers` (no implied JSON type), plus the pair if given.
fn deliver(
    gateway: &WebService<SessionApi>,
    method: Method,
    target: &str,
    pair: Option<&Pair>,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> Answer {
    let mut builder = Request::builder()
        .method(method)
        .uri(target)
        .header("host", HOST);
    if let Some(pair) = pair {
        builder = builder
            .header(header::COOKIE, &pair.cookie)
            .header(PROOF_HEADER, &pair.proof);
    }
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let mut request = builder.body(Full::new(Bytes::from(body))).unwrap();
    request
        .extensions_mut()
        .insert(PeerAddress(PEER.parse::<SocketAddr>().unwrap()));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let response = runtime.block_on(gateway.respond(request));
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = runtime
        .block_on(response.into_body().collect())
        .unwrap()
        .to_bytes();
    Answer {
        status,
        headers,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

/// An upload of `body` declared as `mime` named `name`, with its real length.
fn upload(
    gateway: &WebService<SessionApi>,
    pair: Option<&Pair>,
    mime: &str,
    body: Vec<u8>,
) -> Answer {
    let length = body.len().to_string();
    deliver(
        gateway,
        Method::POST,
        &upload_path(),
        pair,
        &[
            ("origin", ORIGIN),
            ("content-type", mime),
            ("x-attachment-filename", "file.pdf"),
            ("content-length", &length),
        ],
        body,
    )
}

#[test]
fn blocked_and_unknown_upload_types_are_415_before_any_host_or_office_call() {
    let (gateway, calls) = gateway(default_script());
    let pair = sign_in(&gateway).pair();
    for mime in [
        "multipart/form-data; boundary=----x",
        "text/html",
        "image/svg+xml",
        "application/javascript",
        "application/octet-stream",
        "application/x-unknown",
        "application/x-www-form-urlencoded",
    ] {
        let answer = upload(
            &gateway,
            Some(&pair),
            mime,
            b"<svg onload=alert(1)>".to_vec(),
        );
        assert_eq!(answer.status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{mime}");
        assert_eq!(answer.body["error"]["code"], "invalid_request");
    }
    assert_eq!(calls.methods(), ["session.sign_in"]);
}

#[test]
fn oversized_or_unsized_uploads_are_refused_from_their_headers_alone() {
    let (gateway, calls) = gateway(default_script());
    let pair = sign_in(&gateway).pair();
    let send = |mime: &str, length: Option<&str>| {
        let mut headers = vec![
            ("origin", ORIGIN),
            ("content-type", mime),
            ("x-attachment-filename", "file.png"),
        ];
        if let Some(length) = length {
            headers.push(("content-length", length));
        }
        // The body is tiny: the declared length alone decides, before any byte is read.
        deliver(
            &gateway,
            Method::POST,
            &upload_path(),
            Some(&pair),
            &headers,
            b"x".to_vec(),
        )
    };
    let too_large = send("image/png", Some("10485761"));
    assert_eq!(too_large.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(too_large.body["error"]["code"], "too_large");
    assert_eq!(
        send("application/pdf", Some("26214401")).status,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(send("image/png", None).status, StatusCode::LENGTH_REQUIRED);
    // A declared length the body does not carry is refused before the office is asked.
    assert_eq!(
        send("image/png", Some("5")).status,
        StatusCode::LENGTH_REQUIRED
    );
    let unnamed = deliver(
        &gateway,
        Method::POST,
        &upload_path(),
        Some(&pair),
        &[
            ("origin", ORIGIN),
            ("content-type", "image/png"),
            ("content-length", "1"),
        ],
        b"x".to_vec(),
    );
    assert_eq!(unnamed.status, StatusCode::BAD_REQUEST);
    assert_eq!(calls.methods(), ["session.sign_in"]);
}

#[test]
fn uploads_and_downloads_take_only_the_full_pair_from_this_origin() {
    let (gateway, calls) = gateway(default_script());
    let pair = sign_in(&gateway).pair();
    let pdf = b"%PDF-1.7\n".to_vec();
    assert_eq!(
        upload(&gateway, None, "application/pdf", pdf.clone()).status,
        StatusCode::UNAUTHORIZED
    );
    let download = format!("/api/v1/organizations/{ORG}/attachments/{FILE}");
    for headers in [
        vec![("cookie", pair.cookie.as_str())],
        vec![(PROOF_HEADER, pair.proof.as_str())],
        vec![],
    ] {
        let answer = deliver(&gateway, Method::GET, &download, None, &headers, Vec::new());
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{headers:?}");
        let mut upload_headers = headers.clone();
        upload_headers.extend([
            ("origin", ORIGIN),
            ("content-type", "application/pdf"),
            ("x-attachment-filename", "a.pdf"),
            ("content-length", "9"),
        ]);
        let answer = deliver(
            &gateway,
            Method::POST,
            &upload_path(),
            None,
            &upload_headers,
            pdf.clone(),
        );
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{headers:?}");
    }
    // CSRF: the upload keeps the API's exact-Origin and Sec-Fetch-Site rules.
    for headers in [
        vec![("content-type", "application/pdf")],
        vec![
            ("origin", "http://attacker.test"),
            ("content-type", "application/pdf"),
        ],
        vec![
            ("origin", ORIGIN),
            ("content-type", "application/pdf"),
            ("sec-fetch-site", "cross-site"),
        ],
    ] {
        let answer = deliver(
            &gateway,
            Method::POST,
            &upload_path(),
            Some(&pair),
            &headers,
            pdf.clone(),
        );
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{headers:?}");
    }
    // Ids and queries are checked before the session is used.
    for target in [
        format!("/api/v1/organizations/{ORG}/attachments/not-a-uuid"),
        format!("/api/v1/organizations/{ORG}/attachments/{FILE}?download=1"),
    ] {
        let answer = deliver(&gateway, Method::GET, &target, Some(&pair), &[], Vec::new());
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{target}");
    }
    assert_eq!(calls.methods(), ["session.sign_in"]);
}

#[test]
fn a_local_authless_instance_never_serves_attachments_without_the_pair() {
    let (gateway, calls) = gateway_with_auth_enabled(
        default_script(),
        org_host_control::listen::ListenMode::HttpLoopback,
        false,
    );
    let download = format!("/api/v1/organizations/{ORG}/attachments/{FILE}");
    let answer = deliver(&gateway, Method::GET, &download, None, &[], Vec::new());
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        upload(&gateway, None, "image/png", vec![0x89]).status,
        StatusCode::UNAUTHORIZED
    );
    assert!(calls.methods().is_empty());
}

#[test]
fn a_multi_mebibyte_upload_bypasses_the_json_cap_and_reaches_the_offices_lookup() {
    let (gateway, calls) = gateway(default_script());
    let pair = sign_in(&gateway).pair();
    let pdf = vec![b'%'; 2 * 1024 * 1024 + 17];
    // Not 413: the body streams past the 1 MiB business cap. This scripted host links no
    // organization, so the office lookup answers 404 (never an office call).
    let answer = upload(&gateway, Some(&pair), "application/pdf", pdf.clone());
    assert_eq!(answer.status, StatusCode::NOT_FOUND, "{}", answer.body);
    assert_eq!(calls.methods(), ["session.sign_in", "organization.list"]);
    // Every other route keeps the cap.
    let files = format!("/api/v1/organizations/{ORG}/files");
    let length = pdf.len().to_string();
    let capped = deliver(
        &gateway,
        Method::POST,
        &files,
        Some(&pair),
        &[
            ("origin", ORIGIN),
            ("content-type", "application/json"),
            ("content-length", &length),
        ],
        pdf,
    );
    assert_eq!(capped.status, StatusCode::PAYLOAD_TOO_LARGE);
    // A download of a file in an organization the session isn't linked to: 404.
    let download = format!("/api/v1/organizations/{ORG}/attachments/{FILE}");
    let answer = deliver(
        &gateway,
        Method::GET,
        &download,
        Some(&pair),
        &[],
        Vec::new(),
    );
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
}

#[test]
fn file_routes_check_queries_bodies_and_media_before_any_host_call() {
    let (gateway, calls) = gateway(default_script());
    let pair = sign_in(&gateway).pair();
    let files = format!("/api/v1/organizations/{ORG}/files");
    let file = format!("{files}/{FILE}");
    let json = [("origin", ORIGIN), ("content-type", "application/json")];
    let commit = json!({"command_id": "0193a5c0-0000-7000-8000-0000000000ee",
                        "upload_id": FILE, "target_kind": "task", "target_id": FILE});
    for (method, target, headers, body, status) in [
        (
            Method::GET,
            format!("{files}?bogus=1"),
            &[][..],
            "",
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::GET,
            format!("{files}?limit=500"),
            &[][..],
            "",
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::POST,
            files.clone(),
            &json[..],
            "{}",
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::PATCH,
            file.clone(),
            &json[..],
            "{}",
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::PATCH,
            format!("{files}/not-a-uuid"),
            &json[..],
            "{}",
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::PATCH,
            file.clone(),
            &[("origin", ORIGIN), ("content-type", "text/plain")][..],
            "{}",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            Method::PATCH,
            file.clone(),
            &[("content-type", "application/json")][..],
            "{}",
            StatusCode::FORBIDDEN,
        ),
        (
            Method::DELETE,
            file.clone(),
            &[("origin", ORIGIN)][..],
            "",
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let answer = deliver(
            &gateway,
            method.clone(),
            &target,
            Some(&pair),
            headers,
            body.as_bytes().to_vec(),
        );
        assert_eq!(answer.status, status, "{method} {target}");
    }
    let task_target = deliver(
        &gateway,
        Method::POST,
        &files,
        Some(&pair),
        &json,
        commit.to_string().into_bytes(),
    );
    assert_eq!(
        task_target.status,
        StatusCode::BAD_REQUEST,
        "files bind only standalone targets"
    );
    assert_eq!(calls.methods(), ["session.sign_in"]);
    // A valid list reaches the organization lookup (unlinked here: 404).
    let listed = deliver(
        &gateway,
        Method::GET,
        &format!("{files}?target_kind=organization&mime_category=image"),
        Some(&pair),
        &[],
        Vec::new(),
    );
    assert_eq!(listed.status, StatusCode::NOT_FOUND);
    assert_eq!(calls.methods(), ["session.sign_in", "organization.list"]);
}
