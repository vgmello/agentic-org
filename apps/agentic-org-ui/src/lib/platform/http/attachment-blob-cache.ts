import {
	downloadAttachment,
	type DownloadAttachmentOptions,
	type DownloadAttachmentResult
} from './attachments';
import { subscribeAttachmentsSessionEnded } from './attachments-runtime';

export interface CachedBlobHandle {
	readonly key: string;
	readonly objectUrl: string;
	readonly etag?: string;
	readonly contentType: string;
	readonly bytes: number;
}

export interface AcquireBlobOptions {
	signal?: AbortSignal;
	/** Revalidate cached entry with `If-None-Match`; `304` reuses the existing `blob:` URL. */
	revalidate?: boolean;
}

export interface AttachmentBlobCacheDeps {
	download: (
		orgId: string,
		attachmentId: string,
		opts?: DownloadAttachmentOptions
	) => Promise<DownloadAttachmentResult>;
	createObjectURL?: (blob: Blob) => string;
	revokeObjectURL?: (url: string) => void;
	onSessionEnded?: (listener: () => void) => () => void;
	maxEntries?: number;
	maxBytes?: number;
	now?: () => number;
}

interface CacheEntry {
	key: string;
	objectUrl: string;
	etag?: string;
	bytes: number;
	contentType: string;
	refs: number;
	lastUsed: number;
}

interface InFlightRequest {
	controller: AbortController;
	waiters: number;
	aborted: boolean;
	settled: boolean;
	entry?: CacheEntry;
	promise: Promise<CacheEntry>;
}

const DEFAULT_MAX_ENTRIES = 100;
const DEFAULT_MAX_BYTES = 50 * 1024 * 1024;
const abortError = (signal: AbortSignal): unknown =>
	signal.reason ?? new DOMException('Aborted', 'AbortError');
const toHandle = (entry: CacheEntry): CachedBlobHandle => ({
	key: entry.key,
	objectUrl: entry.objectUrl,
	etag: entry.etag,
	contentType: entry.contentType,
	bytes: entry.bytes
});

/**
 * Bounded ref-counted LRU cache of authenticated attachment `blob:` URLs.
 * Never revokes an in-use URL (`refs > 0`), deduplicates concurrent in-flight fetches,
 * supports `If-None-Match` (`304`) reuse, and revokes all URLs immediately on session end.
 */
export function createAttachmentBlobCache(deps: AttachmentBlobCacheDeps) {
	const createUrl = deps.createObjectURL ?? ((blob: Blob) => URL.createObjectURL(blob));
	const revokeUrl = deps.revokeObjectURL ?? ((url: string) => URL.revokeObjectURL(url));
	const maxEntries = deps.maxEntries ?? DEFAULT_MAX_ENTRIES;
	const maxBytes = deps.maxBytes ?? DEFAULT_MAX_BYTES;
	const now = deps.now ?? (() => Date.now());

	const entries = new Map<string, CacheEntry>();
	const retired = new Map<string, number>();
	const inFlight = new Map<string, InFlightRequest>();
	let totalBytes = 0;
	let epoch = 0;

	function touch(entry: CacheEntry) {
		entry.lastUsed = now();
		entries.delete(entry.key);
		entries.set(entry.key, entry);
	}

	function evictUnused(protectKey?: string) {
		if (entries.size <= maxEntries && totalBytes <= maxBytes) return;
		for (const [key, entry] of entries) {
			if (entries.size <= maxEntries && totalBytes <= maxBytes) break;
			if (entry.refs > 0 || key === protectKey || inFlight.has(key)) continue;
			entries.delete(key);
			totalBytes = Math.max(0, totalBytes - entry.bytes);
			revokeUrl(entry.objectUrl);
		}
	}

	function retirePrevious(key: string) {
		const prev = entries.get(key);
		if (!prev) return;
		entries.delete(key);
		totalBytes = Math.max(0, totalBytes - prev.bytes);
		if (prev.refs > 0) {
			retired.set(prev.objectUrl, (retired.get(prev.objectUrl) ?? 0) + prev.refs);
		} else {
			revokeUrl(prev.objectUrl);
		}
	}

	function storeResult(
		key: string,
		res: DownloadAttachmentResult,
		existing: CacheEntry | undefined,
		initialRefs: number
	): CacheEntry {
		if (res.status === 304) {
			if (existing && entries.get(key) === existing) {
				existing.refs += initialRefs;
				touch(existing);
				return existing;
			}
			throw new Error('Received 304 Not Modified without a cached attachment');
		}
		const objectUrl = createUrl(res.blob);
		retirePrevious(key);
		const next: CacheEntry = {
			key,
			objectUrl,
			etag: res.etag,
			bytes: res.blob.size,
			contentType: res.contentType,
			refs: initialRefs,
			lastUsed: now()
		};
		entries.set(key, next);
		totalBytes += next.bytes;
		evictUnused(key);
		return next;
	}

	function startFetch(
		key: string,
		orgId: string,
		attachmentId: string,
		existing?: CacheEntry
	): InFlightRequest {
		const requestEpoch = epoch;
		const controller = new AbortController();
		const flight: InFlightRequest = {
			controller,
			waiters: 0,
			aborted: false,
			settled: false,
			promise: Promise.resolve(undefined as unknown as CacheEntry)
		};
		inFlight.set(key, flight);
		const finish = (res: DownloadAttachmentResult): CacheEntry => {
			if (requestEpoch !== epoch || flight.aborted || flight.waiters <= 0) {
				throw new DOMException('Aborted', 'AbortError');
			}
			const entry = storeResult(key, res, existing, flight.waiters);
			flight.settled = true;
			flight.entry = entry;
			if (inFlight.get(key) === flight) inFlight.delete(key);
			return entry;
		};
		const run = async (): Promise<CacheEntry> => {
			const res = await deps.download(orgId, attachmentId, {
				ifNoneMatch: existing?.etag,
				signal: controller.signal
			});
			if (res.status === 304 && (!existing || entries.get(key) !== existing)) {
				const fresh = await deps.download(orgId, attachmentId, { signal: controller.signal });
				return finish(fresh);
			}
			return finish(res);
		};
		flight.promise = run().finally(() => {
			if (inFlight.get(key) === flight) inFlight.delete(key);
		});
		flight.promise.catch(() => {});
		return flight;
	}

	function detachWaiter(key: string, flight: InFlightRequest): void {
		if (flight.settled) {
			if (flight.entry) release(key, flight.entry.objectUrl);
			return;
		}
		flight.waiters = Math.max(0, flight.waiters - 1);
		if (flight.waiters === 0) {
			flight.aborted = true;
			if (inFlight.get(key) === flight) inFlight.delete(key);
			flight.controller.abort();
		}
	}

	async function acquire(
		orgId: string,
		attachmentId: string,
		opts?: AcquireBlobOptions
	): Promise<CachedBlobHandle> {
		const signal = opts?.signal;
		if (signal?.aborted) throw abortError(signal);
		const key = `${orgId}:${attachmentId}`;
		const cached = entries.get(key);
		if (cached && !opts?.revalidate) {
			cached.refs++;
			touch(cached);
			return toHandle(cached);
		}
		const active = inFlight.get(key);
		const flight =
			active && !active.aborted && !active.settled
				? active
				: startFetch(key, orgId, attachmentId, cached);
		flight.waiters++;
		const waitEpoch = epoch;
		let detached = false;
		let onAbort: (() => void) | undefined;
		try {
			const entry = await new Promise<CacheEntry>((resolve, reject) => {
				onAbort = () => {
					if (detached) return;
					detached = true;
					detachWaiter(key, flight);
					reject(abortError(signal!));
				};
				signal?.addEventListener('abort', onAbort, { once: true });
				flight.promise.then(resolve, reject);
			});
			if (signal?.aborted) {
				if (!detached) {
					detached = true;
					release(key, entry.objectUrl);
				}
				throw abortError(signal);
			}
			if (waitEpoch !== epoch) throw new DOMException('Session ended', 'AbortError');
			if (entries.get(key) === entry) touch(entry);
			return toHandle(entry);
		} finally {
			if (onAbort && signal) signal.removeEventListener('abort', onAbort);
		}
	}

	function release(key: string, objectUrl?: string): void {
		if (objectUrl && retired.has(objectUrl)) {
			const left = Math.max(0, (retired.get(objectUrl) ?? 1) - 1);
			if (left === 0) {
				retired.delete(objectUrl);
				revokeUrl(objectUrl);
			} else {
				retired.set(objectUrl, left);
			}
			return;
		}
		const entry = entries.get(key);
		if (!entry || (objectUrl && entry.objectUrl !== objectUrl)) return;
		entry.refs = Math.max(0, entry.refs - 1);
		evictUnused();
	}

	function clear(): void {
		epoch++;
		for (const flight of inFlight.values()) flight.controller.abort();
		inFlight.clear();
		for (const entry of entries.values()) revokeUrl(entry.objectUrl);
		entries.clear();
		for (const url of retired.keys()) revokeUrl(url);
		retired.clear();
		totalBytes = 0;
	}

	const unsubscribe = deps.onSessionEnded?.(() => clear());
	return Object.freeze({
		acquire,
		release,
		clear,
		size: () => entries.size,
		totalBytes: () => totalBytes,
		dispose: () => {
			unsubscribe?.();
			clear();
		}
	});
}

export const attachmentBlobCache = createAttachmentBlobCache({
	download: downloadAttachment,
	onSessionEnded: subscribeAttachmentsSessionEnded
});
