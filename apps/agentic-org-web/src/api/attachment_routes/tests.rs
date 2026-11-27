//! The attachment slices against a scripted office: what each route sends over IPC (and how
//! many calls), and what the browser gets back. Routing, sessions and the ingress rules are
//! covered through the full gateway in `api/tests/attachments.rs`.

mod hq_office;

use super::download::{download, DownloadRequest};
use super::files::{self, MimeCategory};
use super::media;
use super::office::{office_reply, AttachmentOffice};
use super::upload::{upload, UploadFile};
use crate::body::stream::{self as body_stream, STREAMED_BODY_TIMEOUT};
use crate::office::OfficeFailure;
use crate::service::{ApiReply, WebReply};

use base64::prelude::*;
use http_body_util::BodyExt;
use hyper::body::{Body, Bytes, Frame};
use hyper::header::{HeaderMap, HeaderValue};
use hyper::StatusCode;
use org_proto::attachments::{ArtifactStatus, AttachmentScope, AttachmentTargetKind};
use org_proto::{ApiError, Call, ErrorCode};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Handler = dyn Fn(&str, &Value) -> Result<Value, ApiError> + Send + Sync;

/// An office that answers from `handler` and records every call, in order.
struct FakeOffice {
    calls: Mutex<Vec<(String, Value)>>,
    handler: Box<Handler>,
}

impl FakeOffice {
    fn new(
        handler: impl Fn(&str, &Value) -> Result<Value, ApiError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            calls: Mutex::default(),
            handler: Box::new(handler),
        }
    }
    fn methods(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|c| c.0.clone())
            .collect()
    }
    fn params(&self, method: &str) -> Vec<Value> {
        let calls = self.calls.lock().unwrap();
        calls
            .iter()
            .filter(|c| c.0 == method)
            .map(|c| c.1.clone())
            .collect()
    }
    fn count(&self, method: &str) -> usize {
        self.params(method).len()
    }
}

impl AttachmentOffice for FakeOffice {
    fn call<C>(
        &self,
        params: C::Params,
        _retry: bool,
        _deadline: Duration,
    ) -> impl Future<Output = Result<C::Result, OfficeFailure>> + Send
    where
        C: Call,
        C::Params: Send + Sync,
        C::Result: Send,
    {
        let params = serde_json::to_value(&params).unwrap();
        self.calls
            .lock()
            .unwrap()
            .push((C::METHOD.to_owned(), params.clone()));
        let reply = (self.handler)(C::METHOD, &params)
            .map(|value| serde_json::from_value::<C::Result>(value).unwrap())
            .map_err(OfficeFailure::Refused);
        async move { reply }
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn body_json(reply: &ApiReply) -> Value {
    serde_json::from_slice(reply.body_bytes()).unwrap()
}

const SHA: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
const UPLOAD_ID: &str = "0193a5c0-0000-7000-8000-0000000000aa";
const FILE_ID: &str = "0193a5c0-0000-7000-8000-0000000000bb";
const CHUNK: usize = 512 * 1024;

/// A body that yields fixed frames, like a browser's upload.
struct Frames(VecDeque<Bytes>);
impl Body for Frames {
    type Data = Bytes;
    type Error = std::io::Error;
    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Frame<Bytes>, std::io::Error>>> {
        std::task::Poll::Ready(self.0.pop_front().map(|b| Ok(Frame::data(b))))
    }
}

/// A staging office: begins, appends at the right offset, commits with `committed`, aborts.
fn staging_office(
    committed: Value,
    refuse: Option<(&'static str, ErrorCode)>,
) -> (Arc<FakeOffice>, Arc<Mutex<Vec<u8>>>) {
    let staged = Arc::new(Mutex::new(Vec::new()));
    let bytes = Arc::clone(&staged);
    let declared: Mutex<Value> = Mutex::new(Value::Null);
    let office = FakeOffice::new(move |method, params| {
        if let Some((refused, code)) = refuse {
            if refused == method {
                return Err(ApiError::new(code, "the office refused this upload"));
            }
        }
        if method == "attachment.upload_begin" {
            *declared.lock().unwrap() = params.clone();
        }
        let begun = declared.lock().unwrap().clone();
        let view = |received: u64, done: bool| {
            let mut view = json!({
                "upload_id": UPLOAD_ID, "filename": begun["filename"].clone(),
                "mime_type": begun["mime_type"].clone(), "byte_count": begun["byte_count"].clone(),
                "received_bytes": received, "committed": done,
                "expires_at": "2026-11-28T00:00:00.000Z",
            });
            if done {
                view.as_object_mut()
                    .unwrap()
                    .extend(committed.as_object().unwrap().clone());
            }
            view
        };
        match method {
            "attachment.upload_begin" => {
                Ok(json!({"upload": view(0, false), "chunk_size_bytes": CHUNK}))
            }
            "attachment.upload_chunk" => {
                let mut staged = bytes.lock().unwrap();
                assert_eq!(params["offset"].as_u64().unwrap(), staged.len() as u64);
                let data = BASE64_STANDARD
                    .decode(params["data_base64"].as_str().unwrap())
                    .unwrap();
                assert!(
                    !data.is_empty() && data.len() <= CHUNK,
                    "{} bytes",
                    data.len()
                );
                staged.extend_from_slice(&data);
                Ok(
                    json!({"upload_id": UPLOAD_ID, "received_bytes": staged.len(), "complete": false}),
                )
            }
            "attachment.upload_commit" => Ok(json!({"upload": view(0, true)})),
            "attachment.upload_abort" => Ok(json!({"upload_id": UPLOAD_ID})),
            other => panic!("unexpected {other}"),
        }
    });
    (Arc::new(office), staged)
}

/// Runs one upload of `bytes` (sent in `frame`-sized frames) declaring `declared` bytes.
fn run_upload(
    office: &Arc<FakeOffice>,
    mime: &'static str,
    name: &str,
    bytes: &[u8],
    declared: u64,
) -> ApiReply {
    runtime().block_on(async {
        let deadline = tokio::time::Instant::now() + STREAMED_BODY_TIMEOUT;
        let (body, feeder) = body_stream::channel(declared, CHUNK, deadline);
        let frames = Frames(
            bytes
                .chunks(64 * 1024)
                .map(Bytes::copy_from_slice)
                .collect(),
        );
        let pump = tokio::spawn(feeder.pump(frames));
        let file = UploadFile {
            filename: name.to_owned(),
            mime,
        };
        let reply = upload(Arc::clone(office), file, body).await;
        pump.await.unwrap();
        reply
    })
}

#[test]
fn an_image_upload_is_begun_sent_and_committed_and_answers_201_with_the_staged_file() {
    let png: Vec<u8> = (0..300 * 1024).map(|i| (i % 251) as u8).collect();
    let (office, staged) = staging_office(
        json!({"byte_count": 1234, "width": 640, "height": 480, "content_sha256": SHA, "mime_type": "image/png"}),
        None,
    );
    let reply = run_upload(&office, "image/png", "shot.png", &png, png.len() as u64);
    assert_eq!(reply.status(), StatusCode::CREATED);
    assert_eq!(
        office.methods(),
        [
            "attachment.upload_begin",
            "attachment.upload_chunk",
            "attachment.upload_commit"
        ]
    );
    assert_eq!(
        office.params("attachment.upload_begin")[0],
        json!({"filename": "shot.png", "mime_type": "image/png", "byte_count": png.len()})
    );
    assert_eq!(*staged.lock().unwrap(), png);
    assert_eq!(
        body_json(&reply),
        json!({"result": {
            "upload_id": UPLOAD_ID, "mime_type": "image/png", "byte_count": 1234,
            "width": 640, "height": 480, "page_count": null, "filename": "shot.png",
            "content_sha256": SHA,
        }})
    );
}

#[test]
fn a_document_over_one_mebibyte_streams_in_512_kib_chunks_at_acknowledged_offsets() {
    let pdf: Vec<u8> = (0..3 * CHUNK + 3).map(|i| (i % 253) as u8).collect();
    let (office, staged) = staging_office(json!({"page_count": 2, "content_sha256": SHA}), None);
    let reply = run_upload(
        &office,
        "application/pdf",
        "spec.pdf",
        &pdf,
        pdf.len() as u64,
    );
    assert_eq!(reply.status(), StatusCode::CREATED, "{}", body_json(&reply));
    let offsets: Vec<u64> = office
        .params("attachment.upload_chunk")
        .iter()
        .map(|p| p["offset"].as_u64().unwrap())
        .collect();
    assert_eq!(
        offsets,
        [0, CHUNK as u64, 2 * CHUNK as u64, 3 * CHUNK as u64]
    );
    assert_eq!(
        *staged.lock().unwrap(),
        pdf,
        "every byte arrives once, in order"
    );
    let result = &body_json(&reply)["result"];
    assert_eq!(result["page_count"], 2);
    assert_eq!(result["mime_type"], "application/pdf");
}

#[test]
fn a_refused_chunk_or_commit_aborts_the_staged_upload_and_keeps_the_office_status() {
    let pdf = vec![7u8; CHUNK + 10];
    let (office, _) = staging_office(
        json!({}),
        Some(("attachment.upload_chunk", ErrorCode::TooLarge)),
    );
    let reply = run_upload(&office, "application/pdf", "a.pdf", &pdf, pdf.len() as u64);
    assert_eq!(reply.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        office.methods(),
        [
            "attachment.upload_begin",
            "attachment.upload_chunk",
            "attachment.upload_abort"
        ]
    );
    assert_eq!(
        office.params("attachment.upload_abort")[0],
        json!({"upload_id": UPLOAD_ID})
    );

    let (office, _) = staging_office(
        json!({}),
        Some(("attachment.upload_commit", ErrorCode::InvalidRequest)),
    );
    let reply = run_upload(&office, "application/pdf", "a.pdf", &pdf, pdf.len() as u64);
    assert_eq!(reply.status(), StatusCode::BAD_REQUEST);
    assert_eq!(office.methods().last().unwrap(), "attachment.upload_abort");

    // Refused at the begin: nothing was staged, so nothing is sent or aborted.
    let (office, _) = staging_office(
        json!({}),
        Some(("attachment.upload_begin", ErrorCode::Unsupported)),
    );
    let reply = run_upload(&office, "application/pdf", "a.pdf", &pdf, pdf.len() as u64);
    assert_eq!(reply.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(office.methods(), ["attachment.upload_begin"]);
}

#[test]
fn a_body_shorter_than_its_declared_length_is_400_and_aborted_never_committed() {
    let pdf = vec![1u8; CHUNK + 1];
    let (office, _) = staging_office(json!({}), None);
    let reply = run_upload(
        &office,
        "application/pdf",
        "a.pdf",
        &pdf,
        pdf.len() as u64 + 5,
    );
    assert_eq!(reply.status(), StatusCode::BAD_REQUEST);
    assert_eq!(office.count("attachment.upload_commit"), 0);
    assert_eq!(office.methods().last().unwrap(), "attachment.upload_abort");
}

/// `TASK-98.5`: hyper drops the request future when the client disconnects or an outer
/// deadline cancels it. A begun upload is then still aborted, so its staged bytes never hold
/// the caller's staging quota until they expire.
#[test]
fn an_upload_dropped_mid_transfer_still_aborts_the_staged_upload() {
    let (office, _) = staging_office(json!({}), None);
    runtime().block_on(async {
        let deadline = tokio::time::Instant::now() + STREAMED_BODY_TIMEOUT;
        // A body that never sends a byte: the upload waits for its first chunk.
        let (body, feeder) = body_stream::channel(10, CHUNK, deadline);
        let file = UploadFile {
            filename: "a.pdf".to_owned(),
            mime: "application/pdf",
        };
        let mut request = Box::pin(upload(Arc::clone(&office), file, body));
        let waited = tokio::time::timeout(Duration::from_millis(20), &mut request).await;
        assert!(waited.is_err(), "the upload waits for its body");
        assert_eq!(office.methods(), ["attachment.upload_begin"]);
        drop(request);
        for _ in 0..10 {
            if office.count("attachment.upload_abort") > 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(feeder);
    });
    assert_eq!(
        office.methods(),
        ["attachment.upload_begin", "attachment.upload_abort"]
    );
    assert_eq!(
        office.params("attachment.upload_abort")[0],
        json!({"upload_id": UPLOAD_ID})
    );
}

/// An office holding one file: `inspect` answers its view, `read_chunk` serves its bytes.
fn file_office(mime: &str, filename: &str, bytes: Vec<u8>) -> FakeOffice {
    let view = json!({
        "id": FILE_ID, "scope": "shared", "target_kind": "project",
        "target_id": "0193a5c0-0000-7000-8000-0000000000cc", "filename": filename,
        "mime_type": mime, "byte_count": bytes.len(), "title": filename,
        "artifact_status": "final", "uploaded_by_employee_id": "E1",
        "created_at": "2026-11-27T00:00:00.000Z", "updated_at": "2026-11-27T00:00:00.000Z",
        "content_sha256": SHA,
    });
    FakeOffice::new(move |method, params| match method {
        "attachment.inspect" => Ok(json!({"attachment": view.clone()})),
        "attachment.read_chunk" => {
            let offset = params["offset"].as_u64().unwrap() as usize;
            let max = params["max_bytes"].as_u64().unwrap() as usize;
            let end = (offset + max).min(bytes.len());
            Ok(json!({
                "attachment_id": FILE_ID, "offset": offset,
                "data_base64": BASE64_STANDARD.encode(&bytes[offset..end]),
                "eof": end == bytes.len(), "total_bytes": bytes.len(), "content_sha256": SHA,
            }))
        }
        other => panic!("unexpected {other}"),
    })
}

struct Downloaded {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

fn run_download(
    office: FakeOffice,
    if_none_match: Option<&str>,
    head: bool,
) -> (Arc<FakeOffice>, Result<Downloaded, ApiReply>) {
    let office = Arc::new(office);
    let request = DownloadRequest {
        attachment_id: FILE_ID.to_owned(),
        if_none_match: if_none_match.map(str::to_owned),
        head,
    };
    let answer = runtime().block_on(async {
        match download(Arc::clone(&office), request).await {
            WebReply::Json(reply) => Err(reply),
            WebReply::Stream(response) => {
                let (parts, body) = response.into_parts();
                Ok(Downloaded {
                    status: parts.status,
                    headers: parts.headers,
                    body: body.collect().await.unwrap().to_bytes(),
                })
            }
        }
    });
    (office, answer)
}

fn assert_security_headers(headers: &HeaderMap) {
    let etag = format!("\"{SHA}\"");
    for (name, value) in [
        ("x-content-type-options", "nosniff"),
        ("content-security-policy", "default-src 'none'; sandbox"),
        ("cross-origin-resource-policy", "same-origin"),
        ("cache-control", "private, no-cache"),
        ("etag", etag.as_str()),
    ] {
        assert_eq!(headers.get(name).unwrap(), value, "{name}");
    }
}

#[test]
fn a_document_streams_every_chunk_as_an_attachment_under_the_security_headers() {
    let pdf: Vec<u8> = (0..2 * CHUNK + 100).map(|i| (i % 249) as u8).collect();
    let (office, answer) = run_download(
        file_office("application/pdf", "Q3 report.pdf", pdf.clone()),
        None,
        false,
    );
    let answer = answer.ok().unwrap();
    assert_eq!(answer.status, StatusCode::OK);
    assert_security_headers(&answer.headers);
    assert_eq!(answer.headers["content-type"], "application/pdf");
    assert_eq!(
        answer.headers["content-length"],
        pdf.len().to_string().as_str()
    );
    assert_eq!(
        answer.headers["content-disposition"],
        "attachment; filename=\"Q3 report.pdf\"; filename*=UTF-8''Q3%20report.pdf"
    );
    assert_eq!(answer.body, pdf);
    let offsets: Vec<u64> = office
        .params("attachment.read_chunk")
        .iter()
        .map(|p| p["offset"].as_u64().unwrap())
        .collect();
    assert_eq!(offsets, [0, CHUNK as u64, 2 * CHUNK as u64]);
    assert_eq!(office.methods()[0], "attachment.inspect");
}

#[test]
fn raster_images_alone_are_inline_and_text_is_served_with_utf8() {
    let (_, png) = run_download(
        file_office("image/png", "shot.png", vec![1, 2, 3]),
        None,
        false,
    );
    let png = png.ok().unwrap();
    assert_eq!(png.headers["content-type"], "image/png");
    assert!(png.headers["content-disposition"]
        .to_str()
        .unwrap()
        .starts_with("inline; "));
    assert_security_headers(&png.headers);
    for (mime, served) in [
        ("text/plain", "text/plain; charset=utf-8"),
        ("application/json", "application/json; charset=utf-8"),
        ("image/svg+xml", "application/octet-stream"),
        ("text/html", "application/octet-stream"),
    ] {
        let (_, file) = run_download(file_office(mime, "notes.txt", b"hi".to_vec()), None, false);
        let file = file.ok().unwrap();
        assert_eq!(file.headers["content-type"], served);
        assert!(file.headers["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment; "));
    }
}

#[test]
fn a_matching_if_none_match_is_304_without_reading_any_chunk() {
    for tags in [
        format!("\"{SHA}\""),
        format!("W/\"{SHA}\""),
        format!("\"other\", \"{SHA}\""),
        "*".to_owned(),
    ] {
        let (office, answer) = run_download(
            file_office("image/png", "a.png", vec![9; 10]),
            Some(&tags),
            false,
        );
        let answer = answer.ok().unwrap();
        assert_eq!(answer.status, StatusCode::NOT_MODIFIED, "{tags}");
        assert!(answer.body.is_empty());
        assert_security_headers(&answer.headers);
        assert_eq!(office.count("attachment.read_chunk"), 0, "{tags}");
        assert_eq!(office.methods(), ["attachment.inspect"]);
    }
    let (office, answer) = run_download(
        file_office("image/png", "a.png", vec![9; 10]),
        Some("\"stale\""),
        false,
    );
    assert_eq!(answer.ok().unwrap().status, StatusCode::OK);
    assert_eq!(office.count("attachment.read_chunk"), 1);
    // HEAD answers the headers alone, without a chunk either.
    let (office, head) = run_download(
        file_office("application/pdf", "a.pdf", vec![9; 10]),
        None,
        true,
    );
    let head = head.ok().unwrap();
    assert_eq!((head.status, head.body.len()), (StatusCode::OK, 0));
    assert_eq!(head.headers["content-length"], "10");
    assert_eq!(office.count("attachment.read_chunk"), 0);
}

#[test]
fn refusals_keep_their_class_and_a_temporary_one_says_when_to_retry() {
    for (code, status) in [
        (ErrorCode::NotFound, StatusCode::NOT_FOUND),
        (ErrorCode::NotAuthorized, StatusCode::FORBIDDEN),
        (ErrorCode::StorageError, StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let office = FakeOffice::new(move |_, _| Err(ApiError::new(code, "no")));
        let (office, answer) = run_download(office, None, false);
        assert_eq!(answer.err().unwrap().status(), status);
        assert_eq!(office.methods(), ["attachment.inspect"], "no chunk is read");
    }
    // The bytes are elsewhere for now: 503 with Retry-After, before any header is sent.
    let inner = file_office("application/pdf", "a.pdf", vec![1; 10]);
    let office = FakeOffice::new(move |method, params| {
        if method == "attachment.read_chunk" {
            let mut error = ApiError::new(ErrorCode::Unavailable, "not on this office yet");
            error.retry_after_ms = Some(1500);
            return Err(error);
        }
        (inner.handler)(method, params)
    });
    let (_, answer) = run_download(office, None, false);
    let reply = answer.err().unwrap();
    assert_eq!(reply.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reply.header("retry-after").unwrap(), "2");
    assert_eq!(body_json(&reply)["error"]["retry_after_ms"], 1500);
    assert_eq!(body_json(&reply)["error"]["code"], "unavailable");
}

#[test]
fn a_chunk_that_disagrees_with_the_inspected_file_ends_the_body_short() {
    let pdf = vec![5u8; CHUNK + 50];
    let inner = file_office("application/pdf", "a.pdf", pdf.clone());
    let office = FakeOffice::new(move |method, params| {
        let mut reply = (inner.handler)(method, params)?;
        if method == "attachment.read_chunk" && params["offset"] != 0 {
            reply["content_sha256"] = json!("0".repeat(64));
        }
        Ok(reply)
    });
    let (_, answer) = run_download(office, None, false);
    let answer = answer.ok().unwrap();
    assert_eq!(
        answer.headers["content-length"],
        pdf.len().to_string().as_str()
    );
    assert_eq!(
        answer.body.len(),
        CHUNK,
        "the client sees fewer bytes than declared"
    );
    // An inspected digest that is not a SHA-256 is never served (it would be the ETag).
    let office = FakeOffice::new(|_, _| {
        Ok(json!({"attachment": {
            "id": FILE_ID, "scope": "shared", "target_kind": "organization",
            "filename": "a.pdf", "mime_type": "application/pdf", "byte_count": 1, "title": "a",
            "artifact_status": "final", "uploaded_by_employee_id": "E1",
            "created_at": "x", "updated_at": "x", "content_sha256": "\"\r\nx-evil: 1",
        }}))
    });
    let (office, answer) = run_download(office, None, false);
    assert_eq!(
        answer.err().unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(office.count("attachment.read_chunk"), 0);
}

#[test]
fn office_refusals_map_to_http_classes() {
    let reply = |code, retry: Option<u64>| {
        let mut error = ApiError::new(code, "message");
        error.retry_after_ms = retry;
        office_reply(OfficeFailure::Refused(error))
    };
    let limited = reply(ErrorCode::RateLimited, Some(200));
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.header("retry-after").unwrap(), "1");
    let busy = reply(ErrorCode::Unavailable, None);
    assert_eq!(busy.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(busy.header("retry-after").is_none());
    for (code, status) in [
        (ErrorCode::Unsupported, StatusCode::UNSUPPORTED_MEDIA_TYPE),
        (ErrorCode::TooLarge, StatusCode::PAYLOAD_TOO_LARGE),
        (ErrorCode::InvalidRequest, StatusCode::BAD_REQUEST),
        (ErrorCode::OperationReused, StatusCode::CONFLICT),
        (ErrorCode::NotBound, StatusCode::SERVICE_UNAVAILABLE),
    ] {
        assert_eq!(reply(code, None).status(), status, "{code:?}");
    }
    assert_eq!(
        office_reply(OfficeFailure::Transport).status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[test]
fn the_list_query_is_strict_and_forwards_only_the_offices_filters() {
    let query = files::list_query(Some(
        "scope=shared&target_kind=project&target_id=0193a5c0-0000-7000-8000-0000000000cc\
         &mime_category=image&artifact_status=draft&limit=20&cursor=0193a5c0-0000-7000-8000-0000000000dd",
    ))
    .ok()
    .unwrap();
    assert_eq!(query.params.scope, Some(AttachmentScope::Shared));
    assert_eq!(
        query.params.target_kind,
        Some(AttachmentTargetKind::Project)
    );
    assert_eq!(query.params.limit, Some(20));
    assert_eq!(query.mime_category, Some(MimeCategory::Image));
    assert_eq!(query.artifact_status, Some(ArtifactStatus::Draft));
    assert!(files::list_query(None).is_ok());
    for bad in [
        "limit=0",
        "limit=201",
        "limit=x",
        "scope=all",
        "target_kind=folder",
        "mime_category=video",
        "artifact_status=gone",
        "scope=shared&scope=shared",
        "unknown=1",
        "target_id=a%2Fb",
        "cursor=a+b",
        "scope",
        "scope=",
    ] {
        assert_eq!(
            files::list_query(Some(bad)).err().unwrap().status(),
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
}

#[test]
fn a_list_page_is_narrowed_to_the_asked_type_and_status_keeping_its_cursor() {
    let file = |id: &str, mime: &str, status: &str| {
        json!({"id": id, "scope": "shared", "target_kind": "organization", "filename": "f",
               "mime_type": mime, "byte_count": 1, "title": "f", "artifact_status": status,
               "uploaded_by_employee_id": "E1", "created_at": "x", "updated_at": "x",
               "content_sha256": SHA})
    };
    let page = json!({"attachments": [
        file("1", "image/png", "final"), file("2", "application/pdf", "draft"),
        file("3", "text/csv", "final"),
    ], "next_cursor": "3"});
    let ids = |query: &str| {
        let office = FakeOffice::new({
            let page = page.clone();
            move |_, _| Ok(page.clone())
        });
        let query = files::list_query(Some(query)).ok().unwrap();
        let reply = runtime().block_on(files::list(&office, query));
        assert_eq!(reply.status(), StatusCode::OK);
        let forwarded = office.params("attachment.list")[0].clone();
        assert!(
            forwarded.get("mime_category").is_none() && forwarded.get("artifact_status").is_none()
        );
        let body = body_json(&reply);
        assert_eq!(body["result"]["next_cursor"], "3");
        body["result"]["attachments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids("target_kind=organization"), ["1", "2", "3"]);
    assert_eq!(ids("target_kind=organization&mime_category=image"), ["1"]);
    assert_eq!(
        ids("target_kind=organization&mime_category=document&artifact_status=final"),
        ["3"]
    );
}

const COMMAND: &str = "0193a5c0-0000-7000-8000-0000000000ee";

#[test]
fn a_file_commit_binds_one_source_to_a_standalone_target_and_answers_201() {
    let body = json!({"command_id": COMMAND, "upload_id": UPLOAD_ID, "target_kind": "project",
                      "target_id": "0193a5c0-0000-7000-8000-0000000000cc", "title": "Spec",
                      "artifact_status": "review"});
    let (id, params) = files::commit_params(body.to_string().as_bytes())
        .ok()
        .unwrap();
    assert_eq!(id.as_str(), COMMAND);
    let office = FakeOffice::new(|_, params| {
        Ok(json!({"attachment": {
            "id": FILE_ID, "scope": "shared", "target_kind": "project",
            "target_id": params["target_id"].clone(), "filename": "spec.pdf",
            "mime_type": "application/pdf", "byte_count": 1, "title": params["title"].clone(),
            "artifact_status": params["artifact_status"].clone(), "uploaded_by_employee_id": "E1",
            "created_at": "x", "updated_at": "x", "content_sha256": SHA,
        }}))
    });
    let reply = runtime().block_on(files::commit(&office, params));
    assert_eq!(reply.status(), StatusCode::CREATED);
    assert_eq!(body_json(&reply)["result"]["attachment"]["title"], "Spec");
    let sent = &office.params("attachment.attach")[0];
    assert_eq!(sent["upload_id"], UPLOAD_ID);
    assert_eq!(sent["command_id"], COMMAND);
    assert!(sent.get("source_attachment_id").is_none());

    for bad in [
        json!({"command_id": COMMAND, "target_kind": "organization"}),
        json!({"command_id": COMMAND, "upload_id": UPLOAD_ID, "source_attachment_id": FILE_ID,
               "target_kind": "organization"}),
        json!({"command_id": COMMAND, "upload_id": UPLOAD_ID, "target_kind": "task",
               "target_id": FILE_ID}),
        json!({"command_id": COMMAND, "upload_id": UPLOAD_ID, "target_kind": "organization",
               "extra": 1}),
        json!({"command_id": "not-a-uuid", "upload_id": UPLOAD_ID, "target_kind": "organization"}),
        // Every id the office would look up is a canonical UUIDv7 before any IPC.
        json!({"command_id": COMMAND, "upload_id": "../u", "target_kind": "organization"}),
        json!({"command_id": COMMAND, "upload_id": UPLOAD_ID.to_uppercase(),
               "target_kind": "organization"}),
        json!({"command_id": COMMAND, "source_attachment_id": "0193a5c0-0000-4000-8000-0000000000bb",
               "target_kind": "organization"}),
        json!({"command_id": COMMAND, "upload_id": UPLOAD_ID, "target_kind": "project",
               "target_id": "project-1"}),
    ] {
        let refused = files::commit_params(bad.to_string().as_bytes())
            .err()
            .unwrap();
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[test]
fn a_metadata_update_names_the_path_file_and_at_least_one_change() {
    let (_, params) = files::update_params(
        FILE_ID.to_owned(),
        json!({"command_id": COMMAND, "description": ""})
            .to_string()
            .as_bytes(),
    )
    .ok()
    .unwrap();
    assert_eq!(params.attachment_id, FILE_ID);
    let office = FakeOffice::new(|_, _| {
        Ok(json!({"attachment": {
            "id": FILE_ID, "scope": "user_private", "target_kind": "user_private",
            "filename": "n.md", "mime_type": "text/markdown", "byte_count": 1, "title": "n",
            "artifact_status": "final", "uploaded_by_employee_id": "E1",
            "created_at": "x", "updated_at": "x", "content_sha256": SHA,
        }}))
    });
    let reply = runtime().block_on(files::update(&office, params));
    assert_eq!(reply.status(), StatusCode::OK);
    assert_eq!(
        office.params("attachment.update_meta")[0]["description"],
        ""
    );
    for bad in [
        json!({"command_id": COMMAND}),
        json!({"command_id": COMMAND, "title": "t", "attachment_id": FILE_ID}),
        json!({"title": "t"}),
    ] {
        let refused = files::update_params(FILE_ID.to_owned(), bad.to_string().as_bytes());
        assert_eq!(
            refused.err().unwrap().status(),
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
}

fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.append(
            hyper::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
    }
    map
}

#[test]
fn only_allowlisted_upload_types_pass_with_their_caps() {
    for (mime, cap) in [
        ("image/png", 10 << 20),
        ("IMAGE/JPEG", 10 << 20),
        ("image/webp", 10 << 20),
        ("image/gif", 10 << 20),
        ("application/pdf", 25 << 20),
        (
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            25 << 20,
        ),
        (
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            25 << 20,
        ),
        ("text/markdown; charset=utf-8", 25 << 20),
        ("text/plain;charset=\"UTF-8\"", 25 << 20),
        ("text/csv", 25 << 20),
        ("application/json", 25 << 20),
    ] {
        let media = media::upload_media(&headers(&[("content-type", mime)]))
            .ok()
            .unwrap();
        assert_eq!(media.max_bytes, cap, "{mime}");
    }
    for refused in [
        vec![("content-type", "multipart/form-data; boundary=x")],
        vec![("content-type", "text/html")],
        vec![("content-type", "image/svg+xml")],
        vec![("content-type", "application/javascript")],
        vec![("content-type", "text/javascript")],
        vec![("content-type", "application/octet-stream")],
        vec![("content-type", "application/x-msdownload")],
        vec![("content-type", "image/bmp")],
        vec![("content-type", "application/pdf; charset=utf-8")],
        vec![("content-type", "image/png; name=x")],
        vec![("content-type", "text/plain; charset=latin1")],
        // One `charset` parameter, its value bare or in exactly one pair of quotes.
        vec![("content-type", "text/plain; charset=utf-8; charset=utf-8")],
        vec![("content-type", "text/plain; charset=\"\"utf-8\"\"")],
        vec![("content-type", "text/plain; charset=\"utf-8")],
        vec![("content-type", "text/plain;")],
        vec![("content-type", "image/png"), ("content-type", "image/png")],
        vec![],
    ] {
        let reply = media::upload_media(&headers(&refused)).err().unwrap();
        assert_eq!(
            reply.status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "{refused:?}"
        );
    }
}

#[test]
fn the_filename_header_is_one_percent_encoded_plain_name() {
    let name = |value: &str| media::upload_filename(&headers(&[("x-attachment-filename", value)]));
    assert_eq!(name("Q3%20report.pdf").ok().unwrap(), "Q3 report.pdf");
    assert_eq!(name("caf%C3%A9.md").ok().unwrap(), "café.md");
    assert_eq!(name("a%22b%22.txt").ok().unwrap(), "a\"b\".txt");
    let longest = "a".repeat(252) + ".md";
    assert_eq!(name(&longest).ok().unwrap().len(), 255);
    let long = "a".repeat(253) + ".md";
    for bad in [
        "",
        "%2E%2E%2Fetc",
        "dir%2Fa.pdf",
        "dir%5Ca.pdf",
        ".hidden.md",
        "a%00.md",
        "a%0D%0Ab.md",
        "%FF.md",
        "%2",
        "%20a.md",
        long.as_str(),
        // Bidirectional overrides and isolates (U+202A..U+202E, U+2066..U+2069) could show a
        // name other than the stored one: `invoice<RLO>fdp.exe` reads as `invoiceexe.pdf`.
        "invoice%E2%80%AEfdp.exe",
        "a%E2%80%AA.md",
        "a%E2%81%A6.md",
        "a%E2%81%A9.md",
    ] {
        assert_eq!(
            name(bad).err().unwrap().status(),
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
    assert!(media::upload_filename(&headers(&[])).is_err());
    let twice = headers(&[
        ("x-attachment-filename", "a.md"),
        ("x-attachment-filename", "a.md"),
    ]);
    assert!(media::upload_filename(&twice).is_err());
}

#[test]
fn the_declared_length_is_required_exact_and_within_the_types_cap() {
    let image = media::upload_media(&headers(&[("content-type", "image/png")]))
        .ok()
        .unwrap();
    let pdf = media::upload_media(&headers(&[("content-type", "application/pdf")]))
        .ok()
        .unwrap();
    let length = |pairs: &[(&str, &str)], media| media::upload_length(&headers(pairs), media);
    assert_eq!(
        length(&[("content-length", "10485760")], image).ok(),
        Some(10 << 20)
    );
    assert_eq!(
        length(&[("content-length", "26214400")], pdf).ok(),
        Some(25 << 20)
    );
    for (pairs, media, status) in [
        (
            vec![("content-length", "10485761")],
            image,
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            vec![("content-length", "26214401")],
            pdf,
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            vec![("content-length", "99999999999999999999999")],
            pdf,
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (vec![("content-length", "0")], pdf, StatusCode::BAD_REQUEST),
        (vec![], pdf, StatusCode::LENGTH_REQUIRED),
        (
            vec![("content-length", "-1")],
            pdf,
            StatusCode::LENGTH_REQUIRED,
        ),
        (
            vec![("content-length", "1"), ("content-length", "1")],
            pdf,
            StatusCode::LENGTH_REQUIRED,
        ),
        (
            vec![("content-length", "5"), ("transfer-encoding", "chunked")],
            pdf,
            StatusCode::LENGTH_REQUIRED,
        ),
    ] {
        assert_eq!(
            length(&pairs, media).err().unwrap().status(),
            status,
            "{pairs:?}"
        );
    }
}

#[test]
fn content_disposition_never_carries_raw_quotes_or_line_breaks() {
    let value = media::content_disposition("evil\"\r\nX-Injected: 1\\.pdf", false);
    let text = value.to_str().unwrap();
    assert!(!text.contains(['\r', '\n', '\\']), "{text}");
    assert_eq!(
        text.matches('"').count(),
        2,
        "only the fallback's own quotes: {text}"
    );
    assert!(
        text.starts_with("attachment; filename=\"evil___X-Injected_ 1_.pdf\"; "),
        "{text}"
    );
    assert!(
        text.ends_with("filename*=UTF-8''evil%22%0D%0AX-Injected%3A%201%5C.pdf"),
        "{text}"
    );
    let unicode = media::content_disposition("résumé.png", true);
    assert_eq!(
        unicode.to_str().unwrap(),
        "inline; filename=\"r_sum_.png\"; filename*=UTF-8''r%C3%A9sum%C3%A9.png"
    );
    let empty = media::content_disposition("ü", false);
    assert!(empty
        .to_str()
        .unwrap()
        .starts_with("attachment; filename=\"_\""));
}
