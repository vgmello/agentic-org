import { asNonEmptyStr, asNum, asRec, asStr, type Rec } from './decode';

export const ATTACHMENT_IMAGE_MAX_BYTES = 10 * 1024 * 1024;
export const ATTACHMENT_DOCUMENT_MAX_BYTES = 25 * 1024 * 1024;
export const ATTACHMENT_FILENAME_MAX_BYTES = 255;
export const MESSAGE_ATTACHMENTS_MAX = 10;

export const RASTER_IMAGE_MIMES = ['image/png', 'image/jpeg', 'image/webp', 'image/gif'] as const;

export const BINARY_DOCUMENT_MIMES = [
	'application/pdf',
	'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
	'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet'
] as const;

export const TEXT_DOCUMENT_MIMES = [
	'text/markdown',
	'text/plain',
	'text/csv',
	'application/json'
] as const;

export const ALLOWED_ATTACHMENT_MIMES = [
	...RASTER_IMAGE_MIMES,
	...BINARY_DOCUMENT_MIMES,
	...TEXT_DOCUMENT_MIMES
] as const;

export const ATTACHMENT_ACCEPT = [
	...ALLOWED_ATTACHMENT_MIMES,
	'.png',
	'.jpg',
	'.jpeg',
	'.webp',
	'.gif',
	'.pdf',
	'.docx',
	'.xlsx',
	'.md',
	'.txt',
	'.csv',
	'.json'
].join(',');

const RASTER_SET: ReadonlySet<string> = new Set(RASTER_IMAGE_MIMES);
const BINARY_DOC_SET: ReadonlySet<string> = new Set(BINARY_DOCUMENT_MIMES);
const TEXT_DOC_SET: ReadonlySet<string> = new Set(TEXT_DOCUMENT_MIMES);

const EXTENSION_MIME: Readonly<Record<string, string>> = {
	'.png': 'image/png',
	'.jpg': 'image/jpeg',
	'.jpeg': 'image/jpeg',
	'.webp': 'image/webp',
	'.gif': 'image/gif',
	'.pdf': 'application/pdf',
	'.docx': 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
	'.xlsx': 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
	'.md': 'text/markdown',
	'.markdown': 'text/markdown',
	'.txt': 'text/plain',
	'.csv': 'text/csv',
	'.json': 'application/json'
};

export type ArtifactStatus = 'draft' | 'review' | 'final' | 'superseded' | 'archived';
export type AttachmentScope = 'shared' | 'user_private';
export type AttachmentTargetKind =
	| 'task'
	| 'task_comment'
	| 'review_finding'
	| 'review_finding_reply'
	| 'project'
	| 'organization'
	| 'message'
	| 'ask_answer'
	| 'user_private';

const ARTIFACT_STATUSES: ReadonlySet<string> = new Set<ArtifactStatus>([
	'draft',
	'review',
	'final',
	'superseded',
	'archived'
]);
const SCOPES: ReadonlySet<string> = new Set<AttachmentScope>(['shared', 'user_private']);
const TARGET_KINDS: ReadonlySet<string> = new Set<AttachmentTargetKind>([
	'task',
	'task_comment',
	'review_finding',
	'review_finding_reply',
	'project',
	'organization',
	'message',
	'ask_answer',
	'user_private'
]);

export interface AttachmentMeta {
	id: string;
	filename: string;
	title: string;
	description?: string;
	artifact_status: ArtifactStatus;
	mime_type: string;
	byte_count: number;
	width?: number;
	height?: number;
	page_count?: number;
	uploaded_by_employee_id: string;
	created_at: string;
}

export interface AttachmentView extends AttachmentMeta {
	scope: AttachmentScope;
	target_kind: AttachmentTargetKind;
	target_id?: string;
	updated_at: string;
	content_sha256: string;
}

export interface UploadedFile {
	upload_id: string;
	mime_type: string;
	byte_count: number;
	width?: number;
	height?: number;
	page_count?: number;
	filename: string;
	content_sha256?: string;
}

export type FilenameCheck = { ok: true; filename: string } | { ok: false; message: string };

export type FileValidationResult =
	| { ok: true; filename: string; mimeType: string; maxBytes: number }
	| { ok: false; code: 'invalid_request' | 'unsupported' | 'too_large'; message: string };

function hasUnsafeFilenameChar(name: string): boolean {
	for (const ch of name) {
		const cp = ch.codePointAt(0) ?? 0;
		if (
			cp <= 0x1f ||
			(cp >= 0x7f && cp <= 0x9f) ||
			ch === '/' ||
			ch === '\\' ||
			(cp >= 0x202a && cp <= 0x202e) ||
			(cp >= 0x2066 && cp <= 0x2069)
		) {
			return true;
		}
	}
	return false;
}

/** Matches `attachment_routes/media.rs::upload_filename` before sending bytes. */
export function validateAttachmentFilename(raw: string): FilenameCheck {
	const filename = raw.trim();
	if (!filename || filename !== raw) {
		return { ok: false, message: 'File name must not be empty or have leading/trailing spaces.' };
	}
	if (filename.startsWith('.')) {
		return { ok: false, message: 'File name must not start with a dot.' };
	}
	if (new TextEncoder().encode(filename).byteLength > ATTACHMENT_FILENAME_MAX_BYTES) {
		return { ok: false, message: 'File name exceeds 255 UTF-8 bytes.' };
	}
	if (hasUnsafeFilenameChar(filename)) {
		return { ok: false, message: 'File name contains unsupported path or control characters.' };
	}
	return { ok: true, filename };
}

function extensionOf(filename: string): string {
	const dot = filename.lastIndexOf('.');
	return dot > 0 ? filename.slice(dot).toLowerCase() : '';
}

function isUtf8CharsetParam(param: string): boolean {
	const eq = param.indexOf('=');
	if (eq < 0) return false;
	const key = param.slice(0, eq).trim().toLowerCase();
	let value = param.slice(eq + 1).trim();
	if (value.startsWith('"') && value.endsWith('"') && value.length >= 2) value = value.slice(1, -1);
	return key === 'charset' && value.toLowerCase() === 'utf-8';
}

/**
 * Normalizes a browser File MIME type against the closed allowlist. Never promotes explicitly
 * disallowed types (SVG, HTML, JS, octet-stream); fills missing or `text/x-markdown` types from
 * allowlisted extensions only.
 */
export function normalizeAttachmentMime(rawType: string, filename: string): string | undefined {
	const trimmed = rawType.trim();
	const ext = extensionOf(filename);
	if (!trimmed) return EXTENSION_MIME[ext];
	const parts = trimmed.split(';');
	const essence = parts[0].trim().toLowerCase();
	if (parts.length > 2) return undefined;
	if (essence === 'text/x-markdown' && (ext === '.md' || ext === '.markdown')) {
		return parts.length === 1 || isUtf8CharsetParam(parts[1]) ? 'text/markdown' : undefined;
	}
	if (RASTER_SET.has(essence) || BINARY_DOC_SET.has(essence)) {
		return parts.length === 1 ? essence : undefined;
	}
	if (TEXT_DOC_SET.has(essence)) {
		return parts.length === 1 || isUtf8CharsetParam(parts[1]) ? essence : undefined;
	}
	return undefined;
}

export function validateAttachmentFile(
	file: Pick<File, 'name' | 'type' | 'size'>,
	filenameOverride?: string
): FileValidationResult {
	const nameCheck = validateAttachmentFilename(filenameOverride ?? file.name);
	if (!nameCheck.ok) return { ok: false, code: 'invalid_request', message: nameCheck.message };
	const mimeType = normalizeAttachmentMime(file.type, nameCheck.filename);
	if (!mimeType) {
		return {
			ok: false,
			code: 'unsupported',
			message:
				'This file type is not accepted. Allowed: PNG, JPEG, WebP, GIF, PDF, DOCX, XLSX, Markdown, TXT, CSV, JSON.'
		};
	}
	if (!Number.isFinite(file.size) || file.size <= 0) {
		return { ok: false, code: 'invalid_request', message: 'An attachment holds at least 1 byte.' };
	}
	const maxBytes = RASTER_SET.has(mimeType)
		? ATTACHMENT_IMAGE_MAX_BYTES
		: ATTACHMENT_DOCUMENT_MAX_BYTES;
	if (file.size > maxBytes) {
		const limit = RASTER_SET.has(mimeType) ? '10 MiB' : '25 MiB';
		return {
			ok: false,
			code: 'too_large',
			message: `This file exceeds the ${limit} limit for its type.`
		};
	}
	return { ok: true, filename: nameCheck.filename, mimeType, maxBytes };
}

function asArtifactStatus(raw: unknown): ArtifactStatus {
	const str = asStr(raw);
	return str && ARTIFACT_STATUSES.has(str) ? (str as ArtifactStatus) : 'final';
}

export function decodeAttachmentMeta(raw: unknown): AttachmentMeta | undefined {
	const r = asRec(raw);
	const id = asNonEmptyStr(r?.id);
	const filename = asNonEmptyStr(r?.filename);
	const mime_type = asNonEmptyStr(r?.mime_type);
	const byte_count = asNum(r?.byte_count);
	if (!r || !id || !filename || !mime_type || byte_count === undefined) return undefined;
	const description = asStr(r.description);
	const width = asNum(r.width);
	const height = asNum(r.height);
	const page_count = asNum(r.page_count);
	return {
		id,
		filename,
		title: asNonEmptyStr(r.title) ?? filename,
		...(description ? { description } : {}),
		artifact_status: asArtifactStatus(r.artifact_status),
		mime_type,
		byte_count,
		...(width !== undefined ? { width } : {}),
		...(height !== undefined ? { height } : {}),
		...(page_count !== undefined ? { page_count } : {}),
		uploaded_by_employee_id: asStr(r.uploaded_by_employee_id) ?? '',
		created_at: asStr(r.created_at) ?? ''
	};
}

export function decodeAttachmentMetaList(raw: unknown): AttachmentMeta[] {
	if (!Array.isArray(raw)) return [];
	const out: AttachmentMeta[] = [];
	const seen = new Set<string>();
	for (const entry of raw) {
		const meta = decodeAttachmentMeta(entry);
		if (meta && !seen.has(meta.id)) {
			seen.add(meta.id);
			out.push(meta);
		}
	}
	return out;
}

export function decodeAttachmentView(raw: unknown): AttachmentView | undefined {
	const r = asRec(raw);
	const meta = decodeAttachmentMeta(r);
	const scope = asStr(r?.scope);
	const targetKind = asStr(r?.target_kind);
	if (!r || !meta || !scope || !SCOPES.has(scope) || !targetKind || !TARGET_KINDS.has(targetKind)) {
		return undefined;
	}
	const targetId = asNonEmptyStr(r.target_id);
	return {
		...meta,
		scope: scope as AttachmentScope,
		target_kind: targetKind as AttachmentTargetKind,
		...(targetId ? { target_id: targetId } : {}),
		updated_at: asStr(r.updated_at) ?? meta.created_at,
		content_sha256: asStr(r.content_sha256) ?? ''
	};
}

export function decodeUploadedFile(raw: Rec): UploadedFile | undefined {
	const upload_id = asNonEmptyStr(raw.upload_id);
	const mime_type = asNonEmptyStr(raw.mime_type);
	const filename = asNonEmptyStr(raw.filename);
	const byte_count = asNum(raw.byte_count);
	if (!upload_id || !mime_type || !filename || byte_count === undefined) return undefined;
	const width = asNum(raw.width);
	const height = asNum(raw.height);
	const page_count = asNum(raw.page_count);
	const sha = asNonEmptyStr(raw.content_sha256);
	return {
		upload_id,
		mime_type,
		byte_count,
		filename,
		...(width !== undefined ? { width } : {}),
		...(height !== undefined ? { height } : {}),
		...(page_count !== undefined ? { page_count } : {}),
		...(sha ? { content_sha256: sha } : {})
	};
}
