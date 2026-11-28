import type { AppError } from '$lib/proto';
import type { GatewayReply } from './gateway-errors';

export interface AttachmentBinaryUploadRequest {
	path: string;
	body: Blob;
	contentType: string;
	filename: string;
	signal?: AbortSignal;
}

export interface AttachmentBinaryDownloadRequest {
	path: string;
	ifNoneMatch?: string;
	signal?: AbortSignal;
}

export type AttachmentBinaryDownloadReply =
	| { status: 304; etag?: string }
	| {
			status: 200;
			blob: Blob;
			etag?: string;
			contentType: string;
			contentDisposition?: string;
	  }
	| { status: number; errorBody: unknown };

export interface AttachmentsRuntime {
	read(path: string, signal?: AbortSignal): Promise<GatewayReply>;
	submit(path: string, body: unknown, signal?: AbortSignal): Promise<GatewayReply>;
	patch(path: string, body: unknown, signal?: AbortSignal): Promise<GatewayReply>;
	uploadAttachment(request: AttachmentBinaryUploadRequest): Promise<GatewayReply>;
	downloadAttachment(
		request: AttachmentBinaryDownloadRequest
	): Promise<AttachmentBinaryDownloadReply>;
	newOperationId(): string;
	hasSessionPair(): boolean;
	onSessionEnded(listener: (caller: string | undefined) => void): () => void;
}

const unconfiguredError = (): AppError => ({
	source: 'api',
	code: 'not_authorized',
	message: 'Sign in to access attachments.'
});

const FAIL_CLOSED_RUNTIME: AttachmentsRuntime = {
	read: () => Promise.reject(unconfiguredError()),
	submit: () => Promise.reject(unconfiguredError()),
	patch: () => Promise.reject(unconfiguredError()),
	uploadAttachment: () => Promise.reject(unconfiguredError()),
	downloadAttachment: () => Promise.reject(unconfiguredError()),
	newOperationId: () => {
		throw unconfiguredError();
	},
	hasSessionPair: () => false,
	onSessionEnded: () => () => {}
};

let currentRuntime: AttachmentsRuntime = FAIL_CLOSED_RUNTIME;
const sessionEndedCallbacks = new Set<(caller: string | undefined) => void>();
let unsubscribeRuntime: (() => void) | undefined;

/** Binds the active session transport into the HTTP attachments layer without a platform cycle. */
export function bindAttachmentsRuntime(runtime: AttachmentsRuntime): void {
	unsubscribeRuntime?.();
	currentRuntime = runtime;
	unsubscribeRuntime = runtime.onSessionEnded((caller) => {
		for (const listener of [...sessionEndedCallbacks]) {
			try {
				listener(caller);
			} catch {
				/* Keep teardown listeners isolated. */
			}
		}
	});
}

export function getAttachmentsRuntime(): AttachmentsRuntime {
	return currentRuntime;
}

export function subscribeAttachmentsSessionEnded(
	listener: (caller: string | undefined) => void
): () => void {
	sessionEndedCallbacks.add(listener);
	return () => {
		sessionEndedCallbacks.delete(listener);
	};
}
