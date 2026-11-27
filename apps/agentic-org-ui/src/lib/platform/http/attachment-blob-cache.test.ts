import { describe, expect, it, vi } from 'vitest';
import { createAttachmentBlobCache } from './attachment-blob-cache';
import type { DownloadAttachmentResult } from './attachments';

function makeBlob(bytes: number, type = 'image/png'): Blob {
	return new Blob([new Uint8Array(bytes)], { type });
}

describe('attachmentBlobCache', () => {
	it('evicts least-recently-used unreferenced entries by maxEntries and maxBytes and revokes their URLs', async () => {
		let seq = 0;
		const revoked: string[] = [];
		const download = vi.fn(async (_org: string, id: string): Promise<DownloadAttachmentResult> => ({
			status: 200,
			blob: makeBlob(100),
			etag: `"etag-${id}"`,
			contentType: 'image/png'
		}));
		const cache = createAttachmentBlobCache({
			download,
			createObjectURL: () => `blob:test/${++seq}`,
			revokeObjectURL: (url) => revoked.push(url),
			maxEntries: 2,
			maxBytes: 250
		});

		const h1 = await cache.acquire('org-1', 'a1');
		cache.release(h1.key, h1.objectUrl);
		const h2 = await cache.acquire('org-1', 'a2');
		cache.release(h2.key, h2.objectUrl);

		// Touch a1 so a2 becomes LRU.
		const h1Again = await cache.acquire('org-1', 'a1');
		expect(h1Again.objectUrl).toBe(h1.objectUrl);
		expect(download).toHaveBeenCalledTimes(2);
		cache.release(h1Again.key, h1Again.objectUrl);

		// Inserting a3 evicts a2 (LRU), keeping a1 and a3.
		const h3 = await cache.acquire('org-1', 'a3');
		expect(revoked).toEqual([h2.objectUrl]);
		expect(cache.size()).toBe(2);
		expect(cache.totalBytes()).toBe(200);
		cache.release(h3.key, h3.objectUrl);

		// Oversized entry exceeding maxBytes evicts remaining unused entries after itself.
		download.mockResolvedValueOnce({
			status: 200,
			blob: makeBlob(200),
			etag: '"etag-a4"',
			contentType: 'image/png'
		});
		const h4 = await cache.acquire('org-1', 'a4');
		expect(revoked).toContain(h1.objectUrl);
		expect(revoked).toContain(h3.objectUrl);
		expect(cache.size()).toBe(1);
		expect(cache.totalBytes()).toBe(200);
		cache.release(h4.key, h4.objectUrl);
	});

	it('never revokes an active ref-counted entry until all holders release it', async () => {
		let seq = 0;
		const revoked: string[] = [];
		const download = vi.fn(async (_org: string, id: string): Promise<DownloadAttachmentResult> => ({
			status: 200,
			blob: makeBlob(60),
			etag: `"etag-${id}"`,
			contentType: 'image/png'
		}));
		const cache = createAttachmentBlobCache({
			download,
			createObjectURL: () => `blob:test/${++seq}`,
			revokeObjectURL: (url) => revoked.push(url),
			maxEntries: 1,
			maxBytes: 100
		});

		const h1a = await cache.acquire('org-1', 'a1');
		const h1b = await cache.acquire('org-1', 'a1');
		const h2 = await cache.acquire('org-1', 'a2');
		// Both a1 (refs=2) and a2 (refs=1) stay mounted: zero revocations.
		expect(revoked).toEqual([]);
		expect(cache.size()).toBe(2);

		cache.release(h1a.key, h1a.objectUrl);
		expect(revoked).toEqual([]);
		cache.release(h1b.key, h1b.objectUrl);
		// Once a1 drops to refs=0 and cache is over budget, a1 is evicted immediately.
		expect(revoked).toEqual([h1a.objectUrl]);
		expect(cache.size()).toBe(1);
		cache.release(h2.key, h2.objectUrl);
	});

	it('reuses cached objectUrl on 304 Not Modified conditional revalidation', async () => {
		let seq = 0;
		const revoked: string[] = [];
		const download = vi
			.fn<
				(
					org: string,
					id: string,
					opts?: { ifNoneMatch?: string }
				) => Promise<DownloadAttachmentResult>
			>()
			.mockResolvedValueOnce({
				status: 200,
				blob: makeBlob(40),
				etag: '"abc123"',
				contentType: 'image/png'
			})
			.mockResolvedValueOnce({ status: 304, etag: '"abc123"' });

		const cache = createAttachmentBlobCache({
			download,
			createObjectURL: () => `blob:test/${++seq}`,
			revokeObjectURL: (url) => revoked.push(url)
		});

		const first = await cache.acquire('org-1', 'a1');
		const revalidated = await cache.acquire('org-1', 'a1', { revalidate: true });

		expect(download).toHaveBeenNthCalledWith(
			2,
			'org-1',
			'a1',
			expect.objectContaining({ ifNoneMatch: '"abc123"' })
		);
		expect(revalidated.objectUrl).toBe(first.objectUrl);
		expect(seq).toBe(1);
		expect(revoked).toEqual([]);
	});

	it('deduplicates concurrent in-flight fetches and only aborts upstream when all waiters abort', async () => {
		let resolveDownload!: (value: DownloadAttachmentResult) => void;
		let capturedSignal: AbortSignal | undefined;
		const download = vi.fn((_org: string, _id: string, opts?: { signal?: AbortSignal }) => {
			capturedSignal = opts?.signal;
			return new Promise<DownloadAttachmentResult>((resolve) => {
				resolveDownload = resolve;
			});
		});

		const cache = createAttachmentBlobCache({
			download,
			createObjectURL: () => 'blob:test/dedup',
			revokeObjectURL: () => {}
		});

		const c1 = new AbortController();
		const c2 = new AbortController();
		const p1 = cache.acquire('org-1', 'a1', { signal: c1.signal });
		const p2 = cache.acquire('org-1', 'a1', { signal: c2.signal });

		expect(download).toHaveBeenCalledTimes(1);
		c1.abort();
		await expect(p1).rejects.toThrow();
		// Second waiter is still active so upstream request must NOT abort.
		expect(capturedSignal?.aborted).toBe(false);

		resolveDownload({ status: 200, blob: makeBlob(24), etag: '"e1"', contentType: 'image/png' });
		const h2 = await p2;
		expect(h2.objectUrl).toBe('blob:test/dedup');

		// Aborting the sole waiter aborts upstream immediately.
		const c3 = new AbortController();
		const p3 = cache.acquire('org-1', 'a2', { signal: c3.signal });
		c3.abort();
		await expect(p3).rejects.toThrow();
		expect(capturedSignal?.aborted).toBe(true);
	});

	it('revokes every cached blob URL and aborts in-flight requests on session end', async () => {
		let seq = 0;
		const revoked: string[] = [];
		let endSession!: () => void;
		let inFlightSignal: AbortSignal | undefined;

		const download = vi.fn((_org: string, id: string, opts?: { signal?: AbortSignal }) => {
			if (id === 'slow') {
				inFlightSignal = opts?.signal;
				return new Promise<DownloadAttachmentResult>(() => {});
			}
			return Promise.resolve<DownloadAttachmentResult>({
				status: 200,
				blob: makeBlob(50),
				etag: `"${id}"`,
				contentType: 'image/png'
			});
		});

		const cache = createAttachmentBlobCache({
			download,
			createObjectURL: () => `blob:test/${++seq}`,
			revokeObjectURL: (url) => revoked.push(url),
			onSessionEnded: (listener) => {
				endSession = listener;
				return () => {};
			}
		});

		const h1 = await cache.acquire('org-1', 'a1');
		const h2 = await cache.acquire('org-1', 'a2');
		const slowPromise = cache.acquire('org-1', 'slow');
		slowPromise.catch(() => {});

		// Even with active refs > 0, sign-out / account switch must revoke ALL cached URLs.
		endSession();
		expect(revoked).toEqual([h1.objectUrl, h2.objectUrl]);
		expect(inFlightSignal?.aborted).toBe(true);
		expect(cache.size()).toBe(0);
		expect(cache.totalBytes()).toBe(0);
	});
});
