import type {
	AttachmentBinaryDownloadReply,
	AttachmentBinaryDownloadRequest,
	AttachmentBinaryUploadRequest,
	AttachmentsRuntime
} from '../../src/lib/platform/http/attachments-runtime';
import { fakeDownloadAttachment, fakeUploadAttachment } from './attachments-store';

export type {
	AttachmentBinaryDownloadReply,
	AttachmentBinaryDownloadRequest,
	AttachmentBinaryUploadRequest,
	AttachmentsRuntime
};

let realRuntime: AttachmentsRuntime | undefined;
const sessionEndedCallbacks = new Set<(caller: string | undefined) => void>();
let unsubscribeReal: (() => void) | undefined;

const FAKE_RUNTIME: AttachmentsRuntime = {
	read: (path, signal) =>
		realRuntime
			? realRuntime.read(path, signal)
			: Promise.resolve({ status: 200, body: { result: { attachments: [] } } }),
	submit: (path, body, signal) =>
		realRuntime
			? realRuntime.submit(path, body, signal)
			: Promise.resolve({ status: 201, body: { result: {} } }),
	patch: (path, body, signal) =>
		realRuntime
			? realRuntime.patch(path, body, signal)
			: Promise.resolve({ status: 200, body: { result: {} } }),
	uploadAttachment: (request) =>
		realRuntime?.hasSessionPair()
			? realRuntime.uploadAttachment(request)
			: fakeUploadAttachment(request),
	downloadAttachment: (request) =>
		realRuntime?.hasSessionPair()
			? realRuntime.downloadAttachment(request)
			: fakeDownloadAttachment(request),
	newOperationId: () =>
		realRuntime ? realRuntime.newOperationId() : '0190a5d2-7c3e-7a01-8b2c-000000000001',
	hasSessionPair: () => true,
	onSessionEnded: (listener) => subscribeAttachmentsSessionEnded(listener)
};

export function bindAttachmentsRuntime(runtime: AttachmentsRuntime): void {
	unsubscribeReal?.();
	realRuntime = runtime;
	unsubscribeReal = runtime.onSessionEnded((caller) => {
		for (const listener of [...sessionEndedCallbacks]) {
			try {
				listener(caller);
			} catch {
				/* Keep teardown isolated. */
			}
		}
	});
}

export function getAttachmentsRuntime(): AttachmentsRuntime {
	return FAKE_RUNTIME;
}

export function subscribeAttachmentsSessionEnded(
	listener: (caller: string | undefined) => void
): () => void {
	sessionEndedCallbacks.add(listener);
	return () => {
		sessionEndedCallbacks.delete(listener);
	};
}
