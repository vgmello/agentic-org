import type { AppError } from '$lib/proto';
import { asNonEmptyStr, type Rec } from './decode';
import { gatewayStatusCode, readGatewayAppError, unwrapGatewayResult } from './gateway-errors';
import {
	getAttachmentsRuntime,
	type AttachmentBinaryDownloadReply,
	type AttachmentsRuntime
} from './attachments-runtime';
import {
	decodeAttachmentView,
	decodeUploadedFile,
	validateAttachmentFile,
	type ArtifactStatus,
	type AttachmentScope,
	type AttachmentTargetKind,
	type AttachmentView,
	type UploadedFile
} from './attachments-validate';

export * from './attachments-validate';

export interface UploadAttachmentInput {
	filename?: string;
	signal?: AbortSignal;
}

export interface DownloadAttachmentOptions {
	ifNoneMatch?: string;
	signal?: AbortSignal;
}

export type DownloadAttachmentResult =
	| { status: 304; etag?: string }
	| {
			status: 200;
			blob: Blob;
			etag?: string;
			contentType: string;
			contentDisposition?: string;
	  };

export interface RegisterFileInput {
	command_id?: string;
	upload_id?: string;
	source_attachment_id?: string;
	target_kind: 'project' | 'organization' | 'user_private';
	target_id?: string;
	title?: string;
	description?: string;
	artifact_status?: ArtifactStatus;
}

export interface UpdateFileInput {
	command_id?: string;
	title?: string;
	description?: string;
	artifact_status?: ArtifactStatus;
}

export interface ListFilesParams {
	scope?: AttachmentScope;
	target_kind?: AttachmentTargetKind;
	target_id?: string;
	mime_category?: 'image' | 'document';
	artifact_status?: ArtifactStatus;
	limit?: number;
	cursor?: string;
}

export interface ListFilesResult {
	attachments: AttachmentView[];
	next_cursor?: string;
}

const FILES_LIMIT_MIN = 1;
const FILES_LIMIT_MAX = 200;

function hasDisallowedQueryChar(value: string): boolean {
	for (let i = 0; i < value.length; i++) {
		const code = value.charCodeAt(i);
		if (code <= 0x20 || (code >= 0x7f && code <= 0x9f)) return true;
	}
	return /[%+&=?\s#]/.test(value);
}

function buildFilesQuery(params?: ListFilesParams): string {
	if (!params) return '';
	const pairs: string[] = [];
	const push = (key: string, value: string | undefined) => {
		if (value === undefined || value === '') return;
		if (hasDisallowedQueryChar(value)) {
			throw {
				source: 'api',
				code: 'invalid_request',
				message: `Invalid file list filter value for ${key}.`
			} satisfies AppError;
		}
		pairs.push(`${key}=${value}`);
	};
	push('scope', params.scope);
	push('target_kind', params.target_kind);
	push('target_id', params.target_id);
	push('mime_category', params.mime_category);
	push('artifact_status', params.artifact_status);
	if (params.limit !== undefined) {
		if (
			!Number.isInteger(params.limit) ||
			params.limit < FILES_LIMIT_MIN ||
			params.limit > FILES_LIMIT_MAX
		) {
			throw {
				source: 'api',
				code: 'invalid_request',
				message: `File list limit must be an integer between ${FILES_LIMIT_MIN} and ${FILES_LIMIT_MAX}.`
			} satisfies AppError;
		}
		pairs.push(`limit=${params.limit}`);
	}
	push('cursor', params.cursor);
	return pairs.length ? `?${pairs.join('&')}` : '';
}

function unwrapAttachmentView(result: Rec, fallbackMessage: string): AttachmentView {
	const view = decodeAttachmentView(result.attachment);
	if (!view) {
		throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies AppError;
	}
	return view;
}

export function createAttachmentsClient(getRuntime: () => AttachmentsRuntime) {
	return Object.freeze({
		hasSessionPair: () => getRuntime().hasSessionPair(),

		async uploadAttachment(
			orgId: string,
			file: File,
			opts?: UploadAttachmentInput
		): Promise<UploadedFile> {
			const checked = validateAttachmentFile(file, opts?.filename);
			if (!checked.ok) {
				throw { source: 'api', code: checked.code, message: checked.message } satisfies AppError;
			}
			const path = `/api/v1/organizations/${encodeURIComponent(orgId)}/attachments`;
			const reply = await getRuntime().uploadAttachment({
				path,
				body: file,
				contentType: checked.mimeType,
				filename: checked.filename,
				signal: opts?.signal
			});
			const result = unwrapGatewayResult(reply, 'The attachment could not be uploaded.');
			const decoded = decodeUploadedFile(result);
			if (!decoded) {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Unexpected upload response from gateway.'
				} satisfies AppError;
			}
			return decoded;
		},

		async downloadAttachment(
			orgId: string,
			attachmentId: string,
			opts?: DownloadAttachmentOptions
		): Promise<DownloadAttachmentResult> {
			const path = `/api/v1/organizations/${encodeURIComponent(orgId)}/attachments/${encodeURIComponent(attachmentId)}`;
			const reply: AttachmentBinaryDownloadReply = await getRuntime().downloadAttachment({
				path,
				ifNoneMatch: opts?.ifNoneMatch,
				signal: opts?.signal
			});
			if ('errorBody' in reply) {
				throw readGatewayAppError(
					reply.errorBody,
					gatewayStatusCode(reply.status),
					'The attachment could not be downloaded.'
				);
			}
			if (reply.status === 304) return { status: 304, etag: reply.etag };
			return {
				status: 200,
				blob: reply.blob,
				etag: reply.etag,
				contentType: reply.contentType,
				contentDisposition: reply.contentDisposition
			};
		},

		async listFiles(
			orgId: string,
			params?: ListFilesParams,
			signal?: AbortSignal
		): Promise<ListFilesResult> {
			const query = buildFilesQuery(params);
			const path = `/api/v1/organizations/${encodeURIComponent(orgId)}/files${query}`;
			const reply = await getRuntime().read(path, signal);
			const result = unwrapGatewayResult(reply, 'Files could not be loaded.');
			const raw = Array.isArray(result.attachments) ? result.attachments : [];
			const attachments = raw
				.map(decodeAttachmentView)
				.filter((item): item is AttachmentView => item !== undefined);
			const next_cursor = asNonEmptyStr(result.next_cursor);
			return { attachments, ...(next_cursor ? { next_cursor } : {}) };
		},

		async registerFile(
			orgId: string,
			input: RegisterFileInput,
			signal?: AbortSignal
		): Promise<AttachmentView> {
			const hasUpload = Boolean(input.upload_id?.trim());
			const hasSource = Boolean(input.source_attachment_id?.trim());
			if (hasUpload === hasSource) {
				throw {
					source: 'api',
					code: 'invalid_request',
					message: 'Provide either upload_id or source_attachment_id, not both.'
				} satisfies AppError;
			}
			const runtime = getRuntime();
			const path = `/api/v1/organizations/${encodeURIComponent(orgId)}/files`;
			const body = {
				command_id: input.command_id ?? runtime.newOperationId(),
				...(input.upload_id !== undefined ? { upload_id: input.upload_id } : {}),
				...(input.source_attachment_id !== undefined
					? { source_attachment_id: input.source_attachment_id }
					: {}),
				target_kind: input.target_kind,
				...(input.target_id !== undefined ? { target_id: input.target_id } : {}),
				...(input.title !== undefined ? { title: input.title } : {}),
				...(input.description !== undefined ? { description: input.description } : {}),
				...(input.artifact_status !== undefined ? { artifact_status: input.artifact_status } : {})
			};
			const reply = await runtime.submit(path, body, signal);
			return unwrapAttachmentView(
				unwrapGatewayResult(reply, 'The file could not be saved.'),
				'The file could not be saved.'
			);
		},

		async updateFile(
			orgId: string,
			fileId: string,
			input: UpdateFileInput,
			signal?: AbortSignal
		): Promise<AttachmentView> {
			if (
				input.title === undefined &&
				input.description === undefined &&
				input.artifact_status === undefined
			) {
				throw {
					source: 'api',
					code: 'invalid_request',
					message: 'Provide a title, description, or status to update.'
				} satisfies AppError;
			}
			const runtime = getRuntime();
			const path = `/api/v1/organizations/${encodeURIComponent(orgId)}/files/${encodeURIComponent(fileId)}`;
			const body = {
				command_id: input.command_id ?? runtime.newOperationId(),
				...(input.title !== undefined ? { title: input.title } : {}),
				...(input.description !== undefined ? { description: input.description } : {}),
				...(input.artifact_status !== undefined ? { artifact_status: input.artifact_status } : {})
			};
			const reply = await runtime.patch(path, body, signal);
			return unwrapAttachmentView(
				unwrapGatewayResult(reply, 'The file could not be updated.'),
				'The file could not be updated.'
			);
		}
	});
}

const defaultClient = createAttachmentsClient(getAttachmentsRuntime);

export const hasAttachmentSessionPair = defaultClient.hasSessionPair;
export const uploadAttachment = defaultClient.uploadAttachment;
export const downloadAttachment = defaultClient.downloadAttachment;
export const listFiles = defaultClient.listFiles;
export const registerFile = defaultClient.registerFile;
export const updateFile = defaultClient.updateFile;
