import {
	downloadAttachment,
	RASTER_IMAGE_MIMES,
	type ArtifactStatus,
	type AttachmentMeta,
	type UploadedFile
} from '$lib/platform/http/attachments';

const RASTER_SET: ReadonlySet<string> = new Set(RASTER_IMAGE_MIMES);

export interface StagedAttachment {
	readonly localId: string;
	readonly file?: File;
	readonly filename: string;
	readonly mimeType: string;
	readonly byteCount: number;
	readonly previewUrl?: string;
	readonly status: 'uploading' | 'ready' | 'error';
	readonly uploadId?: string;
	readonly uploaded?: UploadedFile;
	readonly error?: string;
	readonly width?: number;
	readonly height?: number;
}

export function isRasterImage(mimeType: string): boolean {
	return RASTER_SET.has(mimeType.split(';', 1)[0].trim().toLowerCase());
}

export function formatBytes(bytes: number): string {
	if (!Number.isFinite(bytes) || bytes < 0) return '0 B';
	if (bytes < 1024) return `${Math.round(bytes)} B`;
	if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(bytes < 10 * 1024 ? 1 : 0)} KB`;
	return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function mimeBadgeLabel(mimeType: string, filename?: string): string {
	const essence = mimeType.split(';', 1)[0].trim().toLowerCase();
	switch (essence) {
		case 'image/png':
			return 'PNG';
		case 'image/jpeg':
			return 'JPG';
		case 'image/webp':
			return 'WEBP';
		case 'image/gif':
			return 'GIF';
		case 'application/pdf':
			return 'PDF';
		case 'application/vnd.openxmlformats-officedocument.wordprocessingml.document':
			return 'DOCX';
		case 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet':
			return 'XLSX';
		case 'text/markdown':
			return 'MD';
		case 'text/plain':
			return 'TXT';
		case 'text/csv':
			return 'CSV';
		case 'application/json':
			return 'JSON';
		default: {
			const dot = filename ? filename.lastIndexOf('.') : -1;
			if (dot > 0 && filename) {
				const ext = filename.slice(dot + 1).toUpperCase();
				if (ext.length >= 1 && ext.length <= 5) return ext;
			}
			return 'FILE';
		}
	}
}

export function artifactStatusLabel(status: ArtifactStatus): string {
	switch (status) {
		case 'draft':
			return 'Draft';
		case 'review':
			return 'In review';
		case 'final':
			return 'Final';
		case 'superseded':
			return 'Superseded';
		case 'archived':
			return 'Archived';
	}
}

/** Downloads an authenticated attachment and saves it via a short-lived same-origin blob link. */
export async function triggerAttachmentDownload(
	orgId: string,
	attachmentId: string,
	filename: string,
	signal?: AbortSignal
): Promise<void> {
	const reply = await downloadAttachment(orgId, attachmentId, { signal });
	if (reply.status !== 200) {
		throw new Error('Attachment content was not returned.');
	}
	const url = URL.createObjectURL(reply.blob);
	try {
		const link = document.createElement('a');
		link.href = url;
		link.download = filename || 'attachment';
		link.rel = 'noopener';
		link.style.display = 'none';
		document.body.appendChild(link);
		link.click();
		link.remove();
	} finally {
		// Delay revocation briefly so WebKit/Safari finishes reading the blob URL for the save sheet.
		setTimeout(() => URL.revokeObjectURL(url), 1000);
	}
}

function extensionForMime(mime: string): string {
	switch (mime.toLowerCase()) {
		case 'image/png':
			return '.png';
		case 'image/jpeg':
			return '.jpg';
		case 'image/webp':
			return '.webp';
		case 'image/gif':
			return '.gif';
		case 'application/pdf':
			return '.pdf';
		default:
			return '';
	}
}

function collectClipboardFiles(dt: DataTransfer): File[] {
	const out: File[] = [];
	const seen = new Set<File>();
	const push = (file: File | null) => {
		if (!file || seen.has(file)) return;
		seen.add(file);
		out.push(file);
	};
	for (const item of Array.from(dt.items ?? [])) {
		if (item.kind === 'file') push(item.getAsFile());
	}
	if (!out.length) {
		for (const file of Array.from(dt.files ?? [])) push(file);
	}
	return out;
}

function normalizePastedFile(file: File, idx: number): File {
	const rawName = file.name.trim();
	if (rawName && rawName !== 'image.png' && rawName !== 'blob') return file;
	const ext = extensionForMime(file.type) || (rawName === 'image.png' ? '.png' : '');
	const suffix = idx > 0 ? `-${idx + 1}` : '';
	const stamp = new Date().toISOString().replace(/[:.]/g, '-');
	return new File([file], `pasted-${stamp}${suffix}${ext}`, {
		type: file.type,
		lastModified: file.lastModified
	});
}

/** Extracts pasted files from a browser ClipboardEvent, synthesizing a safe name if omitted. */
export function extractClipboardFiles(event: ClipboardEvent): File[] {
	const dt = event.clipboardData;
	if (!dt) return [];
	return collectClipboardFiles(dt).map(normalizePastedFile);
}

export function stagedToAttachmentMeta(
	items: readonly StagedAttachment[],
	uploadedByEmployeeId: string,
	createdAt: string
): AttachmentMeta[] {
	return items
		.filter((item) => item.status === 'ready' && item.uploadId)
		.map((item) => ({
			id: item.uploadId!,
			filename: item.uploaded?.filename ?? item.filename,
			title: item.uploaded?.filename ?? item.filename,
			artifact_status: 'final' as const,
			mime_type: item.uploaded?.mime_type ?? item.mimeType,
			byte_count: item.uploaded?.byte_count ?? item.byteCount,
			...((item.uploaded?.width ?? item.width)
				? { width: item.uploaded?.width ?? item.width }
				: {}),
			...((item.uploaded?.height ?? item.height)
				? { height: item.uploaded?.height ?? item.height }
				: {}),
			...(item.uploaded?.page_count !== undefined ? { page_count: item.uploaded.page_count } : {}),
			uploaded_by_employee_id: uploadedByEmployeeId,
			created_at: createdAt
		}));
}

export function revokeStagedPreviews(items: readonly StagedAttachment[]): void {
	for (const item of items) {
		if (item.previewUrl) URL.revokeObjectURL(item.previewUrl);
	}
}

/** Transitions interrupted in-flight uploads to retryable errors while keeping local previews. */
export function markInterruptedUploads(
	items: readonly StagedAttachment[],
	message = 'Upload interrupted. Retry to upload.'
): StagedAttachment[] {
	if (!items.some((item) => item.status === 'uploading')) return [...items];
	return items.map((item) =>
		item.status === 'uploading' ? { ...item, status: 'error' as const, error: message } : item
	);
}
