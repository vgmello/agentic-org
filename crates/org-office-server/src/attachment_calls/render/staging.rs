//! Bounded chunked-input staging, chunked PDF output buffering, caller-scoped isolation,
//! and CSPRNG render UUIDv7 generation for `attachment.render` (`TASK-98.7`).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use org_doc_render::{BoundedDocumentRenderer, DocumentRenderer, RenderedPdf, MAX_SOURCE_BYTES};
use org_office::{Office, OfficeError};
use org_proto::attachments::{
    AttachmentRenderParams, AttachmentRenderResult, ATTACHMENT_CHUNK_MAX_BASE64_CHARS,
    ATTACHMENT_CHUNK_MAX_BYTES,
};
use org_proto::{ApiError, ErrorCode};
use ring::rand::{SecureRandom, SystemRandom};
use sha2::{Digest, Sha256};

use super::{verify_active_caller, RenderDelegation};

const BUFFER_TTL: Duration = Duration::from_secs(300);
const MAX_STAGED_INPUTS: usize = 8;
const MAX_STAGED_INPUTS_PER_CALLER: usize = 2;
const MAX_BUFFERED_OUTPUTS: usize = 8;
const MAX_BUFFERED_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

static FALLBACK_ID_SEQ: AtomicU64 = AtomicU64::new(1);

/// Isolation key for staged render inputs and buffered local PDF outputs.
///
/// Separates non-delegated calls (`delegated_for: None`), Chief-of-Staff wakes
/// (`delegated_for: Some(None)`), and Chief-of-Staff calls acting for distinct callers
/// (`delegated_for: Some(Some(caller_id))`) so Caller A's staged inputs or buffered PDFs
/// can never be accessed under Caller B's turn or a wake.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct RenderCallerKey {
    member_id: String,
    delegated_for: Option<Option<String>>,
}

impl RenderCallerKey {
    pub(super) fn from_context(member_id: &str, delegation: Option<&RenderDelegation<'_>>) -> Self {
        Self {
            member_id: member_id.to_owned(),
            delegated_for: delegation.map(|d| d.caller_id.map(str::to_owned)),
        }
    }
}

/// Shared render engine and bounded chunk buffers for one office worker.
pub(crate) struct RenderRuntime {
    pub(super) renderer: Arc<dyn DocumentRenderer>,
    inputs: Mutex<BTreeMap<String, StagedInput>>,
    outputs: Mutex<BTreeMap<String, BufferedPdf>>,
}

struct StagedInput {
    caller: RenderCallerKey,
    source: Vec<u8>,
    css: Vec<u8>,
    expires_at: Instant,
}

struct BufferedPdf {
    caller: RenderCallerKey,
    bytes: Vec<u8>,
    page_count: u32,
    sha256: String,
    expires_at: Instant,
}

impl RenderRuntime {
    pub(crate) fn production() -> Self {
        Self::with_renderer(Arc::new(BoundedDocumentRenderer::production()))
    }

    pub(crate) fn with_renderer(renderer: Arc<dyn DocumentRenderer>) -> Self {
        Self {
            renderer,
            inputs: Mutex::new(BTreeMap::new()),
            outputs: Mutex::new(BTreeMap::new()),
        }
    }
}

pub(crate) fn default_runtime() -> &'static RenderRuntime {
    static RUNTIME: OnceLock<RenderRuntime> = OnceLock::new();
    RUNTIME.get_or_init(RenderRuntime::production)
}

pub(super) fn mint_render_uuid() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let mut b = [0u8; 16];
    let rng = SystemRandom::new();
    if rng.fill(&mut b[6..16]).is_err() {
        let seq = FALLBACK_ID_SEQ.fetch_add(1, Ordering::Relaxed);
        let seed = format!("render-fallback:{now}:{seq}");
        let digest = Sha256::digest(seed.as_bytes());
        b[6..16].copy_from_slice(&digest[0..10]);
    }
    b[0..6].copy_from_slice(&now.to_be_bytes()[2..8]);
    b[6] = 0x70 | (b[6] & 0x0f);
    b[8] = 0x80 | (b[8] & 0x3f);
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(super) fn is_output_chunk_read(params: &AttachmentRenderParams) -> bool {
    params.return_bytes
        && params.render_id.is_some()
        && params.source.is_empty()
        && params.source_chunk_base64.is_none()
        && params.css_chunk_base64.is_none()
        && params.input_eof.is_none()
}

pub(super) fn handle_output_chunk_read(
    office: &Arc<Office>,
    runtime: &RenderRuntime,
    caller: &str,
    caller_key: &RenderCallerKey,
    params: &AttachmentRenderParams,
) -> Result<Result<AttachmentRenderResult, ApiError>, OfficeError> {
    if let Err(refusal) = verify_active_caller(office, caller)? {
        return Ok(Err(refusal));
    }
    let Some(render_id) = params.render_id.as_deref() else {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "missing render_id",
        )));
    };
    let Ok(offset) = usize::try_from(params.chunk_offset.unwrap_or(0)) else {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "chunk_offset is past the end of the rendered PDF",
        )));
    };
    let mut guard = runtime
        .outputs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = Instant::now();
    guard.retain(|_, v| v.expires_at > now);
    let Some(entry) = guard.get(render_id) else {
        return Ok(Err(not_found_output()));
    };
    if entry.caller != *caller_key {
        return Ok(Err(not_found_output()));
    }
    if offset > entry.bytes.len() {
        return Ok(Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "chunk_offset is past the end of the rendered PDF",
        )));
    }
    let end = offset
        .saturating_add(ATTACHMENT_CHUNK_MAX_BYTES as usize)
        .min(entry.bytes.len());
    let eof = end >= entry.bytes.len();
    let result = AttachmentRenderResult {
        pdf_base64: Some(STANDARD.encode(&entry.bytes[offset..end])),
        render_id: (!eof).then(|| render_id.to_owned()),
        offset: Some(offset as u64),
        eof: Some(eof),
        page_count: entry.page_count,
        byte_count: entry.bytes.len() as u64,
        content_sha256: Some(entry.sha256.clone()),
        ..Default::default()
    };
    if eof {
        guard.remove(render_id);
    }
    Ok(Ok(result))
}

fn not_found_output() -> ApiError {
    ApiError::new(
        ErrorCode::NotFound,
        "rendered PDF buffer not found or expired",
    )
}

fn not_found_staged_input() -> ApiError {
    ApiError::new(
        ErrorCode::NotFound,
        "staged render input not found or expired",
    )
}

pub(super) enum InputStep {
    NeedMore(String),
    Ready((String, Option<String>)),
}

pub(super) fn assemble_input(
    runtime: &RenderRuntime,
    caller_key: &RenderCallerKey,
    params: &AttachmentRenderParams,
) -> Result<InputStep, ApiError> {
    let chunked = params.source_chunk_base64.is_some()
        || params.css_chunk_base64.is_some()
        || params.input_eof.is_some();
    if !chunked {
        return Ok(InputStep::Ready((
            params.source.clone(),
            params.css.clone(),
        )));
    }
    let src_chunk = decode_chunk_field(params.source_chunk_base64.as_deref())?;
    let css_chunk = decode_chunk_field(params.css_chunk_base64.as_deref())?;

    let mut guard = runtime
        .inputs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = Instant::now();
    guard.retain(|_, v| v.expires_at > now);

    let id = match params.render_id.as_deref() {
        Some(existing) => existing.to_owned(),
        None => insert_fresh_staged_input(&mut guard, caller_key, now),
    };
    let Some(entry) = guard.get_mut(&id) else {
        return Err(not_found_staged_input());
    };
    if entry.caller != *caller_key {
        return Err(not_found_staged_input());
    }
    let next_total = entry
        .source
        .len()
        .saturating_add(entry.css.len())
        .saturating_add(src_chunk.len())
        .saturating_add(css_chunk.len());
    if next_total > MAX_SOURCE_BYTES {
        guard.remove(&id);
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            format!("render input exceeds {MAX_SOURCE_BYTES} bytes"),
        ));
    }
    entry.source.extend_from_slice(&src_chunk);
    entry.css.extend_from_slice(&css_chunk);
    entry.expires_at = now + BUFFER_TTL;

    if params.input_eof != Some(true) {
        return Ok(InputStep::NeedMore(id));
    }
    let finished = guard.remove(&id).expect("staged entry present");
    decode_staged_utf8(finished)
}

fn insert_fresh_staged_input(
    guard: &mut BTreeMap<String, StagedInput>,
    caller_key: &RenderCallerKey,
    now: Instant,
) -> String {
    evict_per_caller_excess(guard, caller_key);
    while guard.len() >= MAX_STAGED_INPUTS {
        let Some(victim) = pick_global_eviction_victim(guard) else {
            break;
        };
        guard.remove(&victim);
    }
    let fresh = mint_render_uuid();
    guard.insert(
        fresh.clone(),
        StagedInput {
            caller: caller_key.clone(),
            source: Vec::new(),
            css: Vec::new(),
            expires_at: now + BUFFER_TTL,
        },
    );
    fresh
}

fn evict_per_caller_excess(
    guard: &mut BTreeMap<String, StagedInput>,
    caller_key: &RenderCallerKey,
) {
    loop {
        let count = guard.values().filter(|v| v.caller == *caller_key).count();
        if count < MAX_STAGED_INPUTS_PER_CALLER {
            return;
        }
        let Some(oldest) = guard
            .iter()
            .filter(|(_, v)| v.caller == *caller_key)
            .min_by_key(|(_, v)| v.expires_at)
            .map(|(k, _)| k.clone())
        else {
            return;
        };
        guard.remove(&oldest);
    }
}

fn pick_global_eviction_victim(guard: &BTreeMap<String, StagedInput>) -> Option<String> {
    guard
        .iter()
        .min_by_key(|(_, v)| v.expires_at)
        .map(|(k, _)| k.clone())
}

fn decode_staged_utf8(finished: StagedInput) -> Result<InputStep, ApiError> {
    let source = String::from_utf8(finished.source).map_err(|_| {
        ApiError::new(
            ErrorCode::InvalidRequest,
            "render source must be valid UTF-8",
        )
    })?;
    let css = if finished.css.is_empty() {
        None
    } else {
        Some(String::from_utf8(finished.css).map_err(|_| {
            ApiError::new(ErrorCode::InvalidRequest, "render CSS must be valid UTF-8")
        })?)
    };
    Ok(InputStep::Ready((source, css)))
}

fn decode_chunk_field(raw: Option<&str>) -> Result<Vec<u8>, ApiError> {
    let Some(text) = raw else {
        return Ok(Vec::new());
    };
    if text.len() > ATTACHMENT_CHUNK_MAX_BASE64_CHARS {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            "chunk exceeds 512 KiB limit",
        ));
    }
    STANDARD
        .decode(text.as_bytes())
        .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "chunk is not valid base64"))
}

pub(super) fn deliver_local_pdf(
    runtime: &RenderRuntime,
    caller_key: &RenderCallerKey,
    rendered: RenderedPdf,
) -> AttachmentRenderResult {
    let sha256 = sha256_hex(&rendered.bytes);
    let chunk_max = ATTACHMENT_CHUNK_MAX_BYTES as usize;
    if rendered.bytes.len() <= chunk_max {
        return AttachmentRenderResult {
            pdf_base64: Some(STANDARD.encode(&rendered.bytes)),
            offset: Some(0),
            eof: Some(true),
            page_count: rendered.page_count,
            byte_count: rendered.byte_count,
            content_sha256: Some(sha256),
            ..Default::default()
        };
    }
    let first_chunk = STANDARD.encode(&rendered.bytes[..chunk_max]);
    let render_id = mint_render_uuid();
    let mut guard = runtime
        .outputs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = Instant::now();
    guard.retain(|_, v| v.expires_at > now);
    evict_output_buffers(&mut guard, rendered.bytes.len());
    guard.insert(
        render_id.clone(),
        BufferedPdf {
            caller: caller_key.clone(),
            bytes: rendered.bytes,
            page_count: rendered.page_count,
            sha256: sha256.clone(),
            expires_at: now + BUFFER_TTL,
        },
    );
    AttachmentRenderResult {
        pdf_base64: Some(first_chunk),
        render_id: Some(render_id),
        offset: Some(0),
        eof: Some(false),
        page_count: rendered.page_count,
        byte_count: rendered.byte_count,
        content_sha256: Some(sha256),
        ..Default::default()
    }
}

fn evict_output_buffers(guard: &mut BTreeMap<String, BufferedPdf>, incoming_len: usize) {
    while guard.len() >= MAX_BUFFERED_OUTPUTS
        || guard.values().map(|v| v.bytes.len()).sum::<usize>() + incoming_len
            > MAX_BUFFERED_OUTPUT_BYTES
    {
        let Some(oldest) = guard
            .iter()
            .min_by_key(|(_, v)| v.expires_at)
            .map(|(k, _)| k.clone())
        else {
            break;
        };
        guard.remove(&oldest);
    }
}
