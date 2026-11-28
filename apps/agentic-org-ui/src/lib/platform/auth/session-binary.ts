import type { AppError } from '$lib/proto';
import { SESSION_PROOF_HEADER } from '../http/wire';
import { SessionRequestError } from './session-gateway';
import { readSessionErrorReason, sameAccount } from './session-http-helpers';
import type { SessionRecord, SessionStore } from './session-record';

export interface AttachmentUploadRequest {
	path: string;
	body: Blob;
	contentType: string;
	filename: string;
	signal?: AbortSignal;
}

export interface AttachmentUploadReply {
	status: number;
	body: unknown;
}

export interface AttachmentDownloadRequest {
	path: string;
	ifNoneMatch?: string;
	signal?: AbortSignal;
}

export type AttachmentDownloadReply =
	| { status: 304; etag?: string }
	| {
			status: 200;
			blob: Blob;
			etag?: string;
			contentType: string;
			contentDisposition?: string;
	  }
	| { status: number; errorBody: unknown };

export interface SessionBinaryDeps {
	store: Pick<SessionStore, 'read'>;
	fetch?: typeof fetch;
	ensureOpen: () => void;
	authEnabled: () => boolean;
	currentRecord: () => SessionRecord | undefined;
	setRecord: (next: SessionRecord) => void;
	generation: () => number;
	active: (token: number) => boolean;
	ended: (reason: 'ended' | 'signed-out' | 'changed-account', login?: string) => void;
	reauth: (reason?: 'session_ended' | 'unknown') => void;
	withCallerAuth: <T>(call: (proof: string | undefined) => Promise<T>) => Promise<T>;
}

const FILENAME_HEADER = 'X-Attachment-Filename';
const FETCH_BASE: Pick<RequestInit, 'mode' | 'credentials' | 'redirect' | 'cache'> = {
	mode: 'same-origin',
	credentials: 'same-origin',
	redirect: 'error',
	cache: 'no-store'
};

function hasControlOrSpace(text: string): boolean {
	for (let i = 0; i < text.length; i++) {
		const code = text.charCodeAt(i);
		if (code <= 0x20 || code === 0x7f) return true;
	}
	return false;
}

function validStreamPath(path: string): boolean {
	return (
		typeof path === 'string' &&
		path.startsWith('/api/v1/') &&
		!path.includes('..') &&
		!path.includes('\\') &&
		!hasControlOrSpace(path)
	);
}

/** Attachment upload/download paths must be exact same-origin /api/v1/ routes with no query string. */
function validAttachmentPath(path: string): boolean {
	if (
		typeof path !== 'string' ||
		!path.startsWith('/api/v1/') ||
		path.includes('?') ||
		path.includes('#') ||
		hasControlOrSpace(path)
	) {
		return false;
	}
	return path
		.slice(1)
		.split('/')
		.every((seg) => {
			if (!seg || seg.includes('\\') || /%(?:2f|5c)/i.test(seg)) return false;
			const dots = seg.replace(/%2e/gi, '.');
			return dots !== '.' && dots !== '..';
		});
}

function pairRequiredError(authEnabled: boolean): AppError {
	return {
		source: 'api',
		code: 'not_authorized',
		message: authEnabled
			? 'Sign in to access attachments.'
			: 'Attachments require a signed-in browser session and are unavailable in local authless mode.'
	};
}

async function readJsonSafely(res: Response): Promise<unknown> {
	try {
		return await res.json();
	} catch {
		return undefined;
	}
}

function errorPayloadOf(reply: AttachmentUploadReply | AttachmentDownloadReply): unknown {
	return 'body' in reply ? reply.body : 'errorBody' in reply ? reply.errorBody : undefined;
}

/**
 * Binary & streaming transport bound to the session controller's proof without exposing it.
 * Attachments are strictly pair-only (cookie + X-Org-Session-Proof) and fail closed without
 * mutating `auth_enabled` or ending a local-authless session.
 */
export function createSessionBinaryOps(deps: SessionBinaryDeps) {
	const sendFetch = deps.fetch ?? globalThis.fetch;

	async function runWithPair<T extends AttachmentUploadReply | AttachmentDownloadReply>(
		execute: (proof: string) => Promise<{ res: Response; parsed: () => Promise<T> }>
	): Promise<T> {
		deps.ensureOpen();
		const expected = deps.currentRecord();
		if (!expected) throw pairRequiredError(deps.authEnabled());
		const token = deps.generation();
		const assertCurrent = async (res?: Response) => {
			if (!deps.active(token)) {
				if (res) await res.body?.cancel().catch(() => {});
				throw new Error('Session changed');
			}
		};

		let attempt = await execute(expected.proof);
		await assertCurrent(attempt.res);
		const first = await attempt.parsed();
		await assertCurrent();
		if (first.status !== 401) return first;

		const stored = await deps.store.read();
		await assertCurrent();
		if (!stored || !sameAccount(expected, stored)) {
			deps.ended(stored ? 'changed-account' : 'ended', stored?.login);
			throw new Error('Session changed');
		}
		if (stored.proof !== expected.proof) {
			deps.setRecord(stored);
			attempt = await execute(stored.proof);
			await assertCurrent(attempt.res);
			const retried = await attempt.parsed();
			await assertCurrent();
			if (retried.status !== 401) return retried;
			deps.reauth(readSessionErrorReason(errorPayloadOf(retried)));
			return retried;
		}
		deps.reauth(readSessionErrorReason(errorPayloadOf(first)));
		return first;
	}

	return Object.freeze({
		openSseStream(path: string, signal?: AbortSignal): Promise<Response> {
			if (!validStreamPath(path)) return Promise.reject(new Error('Invalid API stream path'));
			const token = deps.generation();
			return deps.withCallerAuth(async (proof) => {
				const headers = new Headers({ Accept: 'text/event-stream' });
				if (proof) headers.set(SESSION_PROOF_HEADER, proof);
				const res = await sendFetch(path, { ...FETCH_BASE, method: 'GET', headers, signal });
				if (!deps.active(token)) {
					await res.body?.cancel().catch(() => {});
					throw new Error('Session changed');
				}
				if (res.status === 401) {
					const sessionReason = readSessionErrorReason(await readJsonSafely(res));
					throw new SessionRequestError(401, {
						code: 'unauthenticated',
						message: '',
						session_reason: sessionReason
					});
				}
				return res;
			});
		},

		uploadAttachment(request: AttachmentUploadRequest): Promise<AttachmentUploadReply> {
			if (!validAttachmentPath(request.path)) {
				return Promise.reject(new Error('Invalid attachment upload path'));
			}
			const encodedFilename = encodeURIComponent(request.filename);
			return runWithPair<AttachmentUploadReply>(async (proof) => {
				const headers = new Headers({
					Accept: 'application/json',
					'Content-Type': request.contentType,
					[FILENAME_HEADER]: encodedFilename,
					[SESSION_PROOF_HEADER]: proof
				});
				const res = await sendFetch(request.path, {
					...FETCH_BASE,
					method: 'POST',
					headers,
					body: request.body,
					signal: request.signal
				});
				return {
					res,
					parsed: async () => ({ status: res.status, body: await readJsonSafely(res) })
				};
			});
		},

		downloadAttachment(request: AttachmentDownloadRequest): Promise<AttachmentDownloadReply> {
			if (!validAttachmentPath(request.path)) {
				return Promise.reject(new Error('Invalid attachment download path'));
			}
			return runWithPair<AttachmentDownloadReply>(async (proof) => {
				const headers = new Headers({ [SESSION_PROOF_HEADER]: proof });
				if (request.ifNoneMatch) headers.set('If-None-Match', request.ifNoneMatch);
				const res = await sendFetch(request.path, {
					...FETCH_BASE,
					method: 'GET',
					headers,
					signal: request.signal
				});
				return {
					res,
					parsed: async (): Promise<AttachmentDownloadReply> => {
						const etag = res.headers.get('etag') ?? undefined;
						if (res.status === 304) {
							await res.body?.cancel().catch(() => {});
							return { status: 304, etag };
						}
						if (res.status === 200) {
							const blob = await res.blob();
							return {
								status: 200,
								blob,
								etag,
								contentType:
									res.headers.get('content-type') || blob.type || 'application/octet-stream',
								contentDisposition: res.headers.get('content-disposition') ?? undefined
							};
						}
						return { status: res.status, errorBody: await readJsonSafely(res) };
					}
				};
			});
		}
	});
}
