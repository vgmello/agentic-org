//! Chunked attachment upload and download helpers shared across `file`, `msg`, and `task`
//! commands (`TASK-98.4`, ADR-0037).
//!
//! Part of the leaf `runtime`: never imports any feature slice. Zero hot-path overhead when
//! `--attach` and `--attachment-id` are both omitted (ADR-0014).

use std::io::Read as _;
use std::path::Path;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_command::Refusal;
use org_proto::attachments::{
    AttachmentReadChunk, AttachmentReadChunkParams, AttachmentReadChunkResult,
    AttachmentUploadAbort, AttachmentUploadBegin, AttachmentUploadBeginParams,
    AttachmentUploadBeginResult, AttachmentUploadChunk, AttachmentUploadChunkParams,
    AttachmentUploadCommit, AttachmentUploadCommitResult, AttachmentUploadRef, RenderSourceKind,
    StagedUploadView, ATTACHMENT_CHUNK_MAX_BYTES, ATTACHMENT_DOCUMENT_MAX_BYTES,
    ATTACHMENT_FILENAME_MAX_BYTES, ATTACHMENT_RENDER_SOURCE_MAX_BYTES,
};
use org_proto::{Call, ErrorCode};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::runtime::file_guard::{
    open_confined_regular_file, prepare_output_path, read_upload_file, write_prepared_output,
    FileRoots,
};
use crate::runtime::input::decode_err;
use crate::runtime::transport::OfficeTransport;

/// Summary of a file written by `file fetch`.
#[derive(Clone, Debug)]
pub(crate) struct FetchedFile {
    pub id: String,
    pub output: String,
    pub byte_count: u64,
    pub content_sha256: String,
}

/// Validates `path` locally under `roots` (exfiltration guard + magic-byte pre-check), streams
/// `<= 512 KiB` base64 chunks to `attachment.upload_*`, and commits the staged upload.
/// Aborts the staged upload best-effort if any chunk or commit fails.
pub(crate) fn stage_local_file<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    path: &str,
    roots: &FileRoots,
) -> Result<StagedUploadView, Refusal> {
    let local = read_upload_file(path, roots)?;
    let begin_params = AttachmentUploadBeginParams {
        filename: local.filename,
        mime_type: local.mime_type.to_owned(),
        byte_count: local.bytes.len() as u64,
    };
    let raw_begin = transport.call_method(AttachmentUploadBegin::METHOD, json!(begin_params))?;
    let begun: AttachmentUploadBeginResult =
        serde_json::from_value(raw_begin).map_err(|_| decode_err(AttachmentUploadBegin::METHOD))?;
    let upload_id = begun.upload.upload_id;
    let chunk_size = begun.chunk_size_bytes.clamp(1, ATTACHMENT_CHUNK_MAX_BYTES) as usize;

    let mut offset = 0u64;
    for chunk in local.bytes.chunks(chunk_size) {
        let params = AttachmentUploadChunkParams {
            upload_id: upload_id.clone(),
            offset,
            data_base64: STANDARD.encode(chunk),
        };
        if let Err(err) = transport.call_method(AttachmentUploadChunk::METHOD, json!(params)) {
            abort_upload_best_effort(transport, &upload_id);
            return Err(err);
        }
        offset = offset.saturating_add(chunk.len() as u64);
    }

    let commit_raw = match transport.call_method(
        AttachmentUploadCommit::METHOD,
        json!(AttachmentUploadRef {
            upload_id: upload_id.clone(),
        }),
    ) {
        Ok(raw) => raw,
        Err(err) => {
            abort_upload_best_effort(transport, &upload_id);
            return Err(err);
        }
    };
    let committed: AttachmentUploadCommitResult = serde_json::from_value(commit_raw)
        .map_err(|_| decode_err(AttachmentUploadCommit::METHOD))?;
    Ok(committed.upload)
}

/// Best-effort cleanup of a staged upload on failure.
pub(crate) fn abort_upload_best_effort<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    upload_id: &str,
) {
    let _ = transport.call_method(
        AttachmentUploadAbort::METHOD,
        json!(AttachmentUploadRef {
            upload_id: upload_id.to_owned(),
        }),
    );
}

/// Result of resolving repeatable `--attach` and `--attachment-id` inputs before a write RPC.
///
/// `ids` is the full ordered list passed to the downstream RPC. `staged_upload_ids` contains only
/// uploads freshly staged during this call so the caller can abort them best-effort if the
/// downstream RPC fails (without touching caller-supplied `--attachment-id`s).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ResolvedAttachments {
    pub ids: Vec<String>,
    pub staged_upload_ids: Vec<String>,
}

impl ResolvedAttachments {
    /// Best-effort aborts every freshly staged upload on downstream RPC error.
    pub(crate) fn abort_staged<T: OfficeTransport + ?Sized>(&self, transport: &mut T) {
        for id in &self.staged_upload_ids {
            abort_upload_best_effort(transport, id);
        }
    }
}

/// Resolves repeatable `--attach <path>` and `--attachment-id <id>` flags into a single list of
/// IDs for `chat.send`, `msg.reply`, `chat.ask`, `chat.answer`, `tasks.create`, `tasks.update`, or
/// `tasks.comment`.
///
/// Fast path (ADR-0014): when both slices are empty, does zero filesystem work and zero RPCs.
pub(crate) fn resolve_attach_ids<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    attach_paths: &[String],
    attachment_ids: &[String],
    max_count: usize,
) -> Result<ResolvedAttachments, Refusal> {
    if attach_paths.is_empty() && attachment_ids.is_empty() {
        return Ok(ResolvedAttachments::default());
    }
    resolve_attach_ids_with_roots(
        transport,
        attach_paths,
        attachment_ids,
        max_count,
        &FileRoots::from_env(),
    )
}

/// Like [`resolve_attach_ids`], with explicit [`FileRoots`] for deterministic testing.
pub(crate) fn resolve_attach_ids_with_roots<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    attach_paths: &[String],
    attachment_ids: &[String],
    max_count: usize,
    roots: &FileRoots,
) -> Result<ResolvedAttachments, Refusal> {
    if attach_paths.is_empty() && attachment_ids.is_empty() {
        return Ok(ResolvedAttachments::default());
    }
    let total = attach_paths.len().saturating_add(attachment_ids.len());
    if total > max_count {
        return Err(Refusal::new(
            ErrorCode::TooLarge,
            format!("At most {max_count} attachments are allowed here (got {total})."),
            "Attach fewer files and retry.",
        ));
    }
    for (idx, id) in attachment_ids.iter().enumerate() {
        if !org_proto::is_uuid_v7(id) {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                format!("Invalid attachment ID `{id}`: expected a canonical lowercase UUIDv7."),
                "Pass attachment IDs returned by `file upload` or `file list`.",
            ));
        }
        if attachment_ids[..idx].contains(id) {
            return Err(Refusal::new(
                ErrorCode::InvalidRequest,
                format!("Duplicate attachment ID `{id}`."),
                "Pass each attachment ID at most once.",
            ));
        }
    }

    let mut staged_upload_ids = Vec::with_capacity(attach_paths.len());
    for path in attach_paths {
        match stage_local_file(transport, path, roots) {
            Ok(staged) => staged_upload_ids.push(staged.upload_id),
            Err(err) => {
                for id in &staged_upload_ids {
                    abort_upload_best_effort(transport, id);
                }
                return Err(err);
            }
        }
    }
    let mut ids = staged_upload_ids.clone();
    ids.extend(attachment_ids.iter().cloned());
    Ok(ResolvedAttachments {
        ids,
        staged_upload_ids,
    })
}

/// Downloads `attachment_id` in `<= 512 KiB` chunks, verifies its SHA-256 digest, and safely
/// writes it to `output_path` under `roots` (`O_NOFOLLOW`, regular file only).
pub(crate) fn fetch_attachment_to_path<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    attachment_id: &str,
    output_path: &str,
    roots: &FileRoots,
) -> Result<FetchedFile, Refusal> {
    if !org_proto::is_uuid_v7(attachment_id) {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "Attachment `--id` must be a canonical lowercase UUIDv7.",
            "Pass an attachment ID returned by `file upload` or `file list`.",
        ));
    }
    let prepared = prepare_output_path(output_path, roots)?;

    let mut bytes = Vec::new();
    let mut expected_sha = String::new();
    let mut expected_total = None;

    loop {
        let offset = bytes.len() as u64;
        let params = AttachmentReadChunkParams {
            attachment_id: attachment_id.to_owned(),
            offset,
            max_bytes: Some(ATTACHMENT_CHUNK_MAX_BYTES),
        };
        let raw = transport.call_method(AttachmentReadChunk::METHOD, json!(params))?;
        let chunk: AttachmentReadChunkResult =
            serde_json::from_value(raw).map_err(|_| decode_err(AttachmentReadChunk::METHOD))?;
        if chunk.total_bytes > ATTACHMENT_DOCUMENT_MAX_BYTES {
            return Err(Refusal::new(
                ErrorCode::TooLarge,
                "Attachment size exceeds the maximum downloadable limit (25 MiB).",
                "Verify the attachment record on the office.",
            ));
        }
        if let Some(prev) = expected_total {
            if prev != chunk.total_bytes || expected_sha != chunk.content_sha256 {
                return Err(Refusal::new(
                    ErrorCode::Unavailable,
                    "Attachment digest or size changed during download.",
                    "Retry `file fetch`.",
                ));
            }
        } else {
            expected_total = Some(chunk.total_bytes);
            expected_sha = chunk.content_sha256;
        }
        let decoded = STANDARD.decode(&chunk.data_base64).map_err(|_| {
            Refusal::new(
                ErrorCode::Unavailable,
                "Office returned invalid base64 chunk data.",
                "Retry `file fetch`.",
            )
        })?;
        if decoded.is_empty() && !chunk.eof {
            return Err(Refusal::new(
                ErrorCode::Unavailable,
                "Office returned an empty non-EOF chunk.",
                "Retry `file fetch`.",
            ));
        }
        if (bytes.len() as u64).saturating_add(decoded.len() as u64) > ATTACHMENT_DOCUMENT_MAX_BYTES
        {
            return Err(Refusal::new(
                ErrorCode::TooLarge,
                "Downloaded attachment bytes exceeded the 25 MiB bound.",
                "Verify the attachment record on the office.",
            ));
        }
        bytes.extend_from_slice(&decoded);
        if chunk.eof {
            break;
        }
    }

    if Some(bytes.len() as u64) != expected_total {
        return Err(Refusal::new(
            ErrorCode::Unavailable,
            "Downloaded byte count does not match attachment total_bytes.",
            "Retry `file fetch`.",
        ));
    }
    let actual_sha: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual_sha != expected_sha {
        return Err(Refusal::new(
            ErrorCode::Unavailable,
            "Downloaded bytes did not match content_sha256.",
            "Retry `file fetch`.",
        ));
    }
    write_prepared_output(&prepared, &bytes)?;

    Ok(FetchedFile {
        id: attachment_id.to_owned(),
        output: prepared.display_path.display().to_string(),
        byte_count: bytes.len() as u64,
        content_sha256: actual_sha,
    })
}

/// Validated local Markdown/HTML source (and optional CSS) ready for `attachment.render`.
#[derive(Clone, Debug)]
pub(crate) struct LocalRenderInputs {
    pub filename: String,
    pub source_kind: RenderSourceKind,
    pub source: String,
    pub css: Option<String>,
}

/// Validates and reads `--from` (`.md`/`.markdown`/`.html`/`.htm`) and optional `--css` (`.css`)
/// strictly through [`open_confined_regular_file`] (`O_NOFOLLOW`, allowed roots + denylist,
/// combined `<= 2 MiB` UTF-8 cap) BEFORE any `attachment.render` RPC opens.
pub(crate) fn read_render_inputs(
    from_path: &str,
    css_path: Option<&str>,
    roots: &FileRoots,
) -> Result<LocalRenderInputs, Refusal> {
    let (canonical_src, src_file) = open_confined_regular_file(from_path, roots)?;
    let (filename, source_kind) = classify_render_source(&canonical_src)?;
    let source = read_bounded_utf8(
        src_file,
        ATTACHMENT_RENDER_SOURCE_MAX_BYTES,
        "Render source",
        false,
    )?;

    let css = match css_path {
        Some(raw_css) => {
            let (canonical_css, css_file) = open_confined_regular_file(raw_css, roots)?;
            classify_render_css(&canonical_css)?;
            let remaining = ATTACHMENT_RENDER_SOURCE_MAX_BYTES.saturating_sub(source.len());
            let css_text = read_bounded_utf8(css_file, remaining, "Combined source + CSS", true)?;
            Some(css_text)
        }
        None => None,
    };

    Ok(LocalRenderInputs {
        filename,
        source_kind,
        source,
        css,
    })
}

fn read_bounded_utf8(
    file: std::fs::File,
    max_bytes: usize,
    label: &str,
    allow_empty: bool,
) -> Result<String, Refusal> {
    let mut bytes = Vec::new();
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| {
            Refusal::new(
                ErrorCode::InvalidRequest,
                format!("Could not read {label}: {err}"),
                "Verify the file is a readable regular UTF-8 file inside an allowed root.",
            )
        })?;
    if !allow_empty && bytes.is_empty() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            format!("{label} must not be empty."),
            "Pass a non-empty Markdown or HTML file.",
        ));
    }
    if bytes.len() > max_bytes {
        return Err(Refusal::new(
            ErrorCode::TooLarge,
            format!("{label} exceeds the 2 MiB render input limit."),
            "Keep combined Markdown/HTML and CSS at or below 2 MiB.",
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("{label} must be valid UTF-8 text."),
            "Save the source or stylesheet as UTF-8 and retry.",
        )
    })
}

fn plain_filename(path: &Path) -> Result<String, Refusal> {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "File name must be valid UTF-8.",
            "Rename the file to a plain UTF-8 filename.",
        ));
    };
    if name.is_empty()
        || name.len() > ATTACHMENT_FILENAME_MAX_BYTES
        || name.starts_with('.')
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
    {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "Filename must be a single plain non-hidden filename.",
            "Rename the file to a plain filename (1..=255 bytes, no leading dot).",
        ));
    }
    Ok(name.to_owned())
}

fn classify_render_source(path: &Path) -> Result<(String, RenderSourceKind), Refusal> {
    let name = plain_filename(path)?;
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    let kind = match ext.as_str() {
        "md" | "markdown" => RenderSourceKind::Markdown,
        "html" | "htm" => RenderSourceKind::Html,
        _ => {
            return Err(Refusal::new(
                ErrorCode::Unsupported,
                format!("Render source extension `.{ext}` is not supported."),
                "Pass a `.md`, `.markdown`, `.html`, or `.htm` file to `--from`.",
            ));
        }
    };
    Ok((name, kind))
}

fn classify_render_css(path: &Path) -> Result<(), Refusal> {
    let name = plain_filename(path)?;
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    if ext != "css" {
        return Err(Refusal::new(
            ErrorCode::Unsupported,
            format!("Stylesheet extension `.{ext}` is not supported for `--css`."),
            "Pass a `.css` file to `--css`.",
        ));
    }
    Ok(())
}
