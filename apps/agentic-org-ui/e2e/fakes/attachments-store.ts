import type { AttachmentMeta, UploadedFile } from '../../src/lib/platform/http/attachments';
import type {
	AttachmentBinaryDownloadReply,
	AttachmentBinaryDownloadRequest,
	AttachmentBinaryUploadRequest
} from '../../src/lib/platform/http/attachments-runtime';

interface StoredFakeBlob {
	readonly id: string;
	readonly orgId: string;
	readonly filename: string;
	readonly mimeType: string;
	readonly byteCount: number;
	readonly width?: number;
	readonly height?: number;
	readonly pageCount?: number;
	readonly etag: string;
	readonly blob: Blob;
	readonly createdAt: string;
}

const byOrg = new Map<string, Map<string, StoredFakeBlob>>();
let seq = 0;

function fakeUuidV7(): string {
	seq += 1;
	const tail = seq.toString(16).padStart(12, '0');
	return `0190a5d2-7c3e-7a01-8b2c-${tail}`;
}

function fakeEtag(id: string, byteCount: number): string {
	const seed = `${id.replace(/-/g, '')}${byteCount.toString(16).padStart(32, '0')}`.slice(0, 64);
	return `"${seed}"`;
}

async function sniffImageDimensions(
	blob: Blob,
	mimeType: string
): Promise<{ width?: number; height?: number }> {
	if (mimeType === 'image/png' && blob.size >= 24) {
		const buf = new Uint8Array(await blob.slice(0, 24).arrayBuffer());
		if (buf[0] === 0x89 && buf[1] === 0x50 && buf[2] === 0x4e && buf[3] === 0x47) {
			const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
			return { width: view.getUint32(16, false), height: view.getUint32(20, false) };
		}
	}
	return {};
}

function parseOrgFromUploadPath(path: string): string {
	const m = /^\/api\/v1\/organizations\/([^/]+)\/attachments$/.exec(path);
	return m ? decodeURIComponent(m[1]) : 'default';
}

function parseOrgAndIdFromDownloadPath(path: string): { orgId: string; id: string } | undefined {
	const m = /^\/api\/v1\/organizations\/([^/]+)\/attachments\/([^/]+)$/.exec(path);
	return m ? { orgId: decodeURIComponent(m[1]), id: decodeURIComponent(m[2]) } : undefined;
}

export async function fakeUploadAttachment(
	request: AttachmentBinaryUploadRequest
): Promise<{ status: number; body: unknown }> {
	const orgId = parseOrgFromUploadPath(request.path);
	const id = fakeUuidV7();
	const dims = await sniffImageDimensions(request.body, request.contentType);
	const pageCount = request.contentType === 'application/pdf' ? 2 : undefined;
	const etag = fakeEtag(id, request.body.size);
	const entry: StoredFakeBlob = {
		id,
		orgId,
		filename: request.filename,
		mimeType: request.contentType,
		byteCount: request.body.size,
		width: dims.width,
		height: dims.height,
		pageCount,
		etag,
		blob: request.body,
		createdAt: new Date().toISOString()
	};
	let orgMap = byOrg.get(orgId);
	if (!orgMap) {
		orgMap = new Map();
		byOrg.set(orgId, orgMap);
	}
	orgMap.set(id, entry);
	const uploaded: UploadedFile = {
		upload_id: id,
		mime_type: entry.mimeType,
		byte_count: entry.byteCount,
		filename: entry.filename,
		...(entry.width !== undefined ? { width: entry.width } : {}),
		...(entry.height !== undefined ? { height: entry.height } : {}),
		...(entry.pageCount !== undefined ? { page_count: entry.pageCount } : {}),
		content_sha256: etag.slice(1, -1)
	};
	return { status: 201, body: { result: uploaded } };
}

export async function fakeDownloadAttachment(
	request: AttachmentBinaryDownloadRequest
): Promise<AttachmentBinaryDownloadReply> {
	const parsed = parseOrgAndIdFromDownloadPath(request.path);
	const entry = parsed ? byOrg.get(parsed.orgId)?.get(parsed.id) : undefined;
	if (!entry) {
		return {
			status: 404,
			errorBody: { error: { code: 'not_found', message: 'Attachment not found.' } }
		};
	}
	if (request.ifNoneMatch && request.ifNoneMatch.includes(entry.etag)) {
		return { status: 304, etag: entry.etag };
	}
	return {
		status: 200,
		blob: entry.blob,
		etag: entry.etag,
		contentType: entry.mimeType,
		contentDisposition: `${entry.mimeType.startsWith('image/') ? 'inline' : 'attachment'}; filename="${entry.filename}"`
	};
}

export function resolveFakeAttachmentMeta(
	orgId: string,
	ids: readonly string[] | undefined,
	uploadedByEmployeeId: string
): AttachmentMeta[] {
	if (!ids?.length) return [];
	const orgMap = byOrg.get(orgId);
	if (!orgMap) return [];
	const out: AttachmentMeta[] = [];
	for (const id of ids) {
		const entry = orgMap.get(id);
		if (!entry) continue;
		out.push({
			id: entry.id,
			filename: entry.filename,
			title: entry.filename,
			artifact_status: 'final',
			mime_type: entry.mimeType,
			byte_count: entry.byteCount,
			...(entry.width !== undefined ? { width: entry.width } : {}),
			...(entry.height !== undefined ? { height: entry.height } : {}),
			...(entry.pageCount !== undefined ? { page_count: entry.pageCount } : {}),
			uploaded_by_employee_id: uploadedByEmployeeId,
			created_at: entry.createdAt
		});
	}
	return out;
}
