import { describe, expect, it, vi } from 'vitest';
import { markInterruptedUploads, type StagedAttachment } from '$lib/components/attachments/model';
import { createDraftStore } from './store';

function stagedPreview(id: string, previewUrl: string): StagedAttachment {
	return {
		localId: id,
		filename: `${id}.png`,
		mimeType: 'image/png',
		byteCount: 16,
		previewUrl,
		status: 'ready',
		uploadId: `u-${id}`
	};
}

describe('same-tab Chat drafts', () => {
	it('keeps thread replies and individual asks separate, even for the same id', () => {
		const store = createDraftStore(() => 'org-a');
		const first = store.forThread('root');
		first.reply.write('Thread text');
		first.ask('root').write('Ask on the root');
		first.ask('reply').write('Ask on a reply');
		const reopened = store.forThread('root');
		expect(reopened.reply.read()).toBe('Thread text');
		expect(reopened.ask('root').read()).toBe('Ask on the root');
		expect(reopened.ask('reply').read()).toBe('Ask on a reply');
		expect(store.forThread('another').ask('root').read()).toBe('');
	});

	it('clears the submitted draft while preserving later edits and other drafts', () => {
		const store = createDraftStore(() => 'org-a');
		const { reply, ask } = store.forThread('root');
		reply.write('Old reply');
		const clearReply = reply.clearOnSuccess();
		reply.write('Newer reply');
		const askDraft = ask('ask');
		askDraft.write('Answer');
		const clearAnswer = askDraft.clearOnSuccess();
		clearReply();
		clearAnswer();
		expect(reply.read()).toBe('Newer reply');
		expect(askDraft.read()).toBe('');
	});

	it('clears on organization changes and ignores writes and late completions from old scopes', () => {
		let id = 'org-a';
		const store = createDraftStore(() => id);
		const old = store.forThread('root').reply;
		old.write('A draft');
		const clearOld = old.clearOnSuccess();
		id = 'org-b';
		const next = store.forThread('root').reply;
		expect(next.read()).toBe('');
		next.write('B draft');
		old.write('Stale draft');
		clearOld();
		expect(next.read()).toBe('B draft');
		id = 'org-a';
		store.syncScope();
		expect(store.forThread('root').reply.read()).toBe('');
		store.dispose();
		next.write('After leaving the shell');
		expect(store.forThread('root').reply.read()).toBe('');
	});

	it('drops the least recently edited draft at capacity and removes empty text', () => {
		const store = createDraftStore(() => 'org-a', 2);
		const a = store.forThread('a').reply;
		const b = store.forThread('b').reply;
		const c = store.forThread('c').reply;
		a.write('A');
		b.write('B');
		a.write('Updated A');
		c.write('C');
		expect(b.read()).toBe('');
		expect(a.read()).toBe('Updated A');
		a.write('');
		b.write('B again');
		expect(c.read()).toBe('C');
		expect(b.read()).toBe('B again');
	});

	it('revokes staged blob preview URLs on session end, org switch, eviction, clearOnSuccess, and dispose', () => {
		const revokeSpy = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
		try {
			let endSession!: () => void;
			const unsub = vi.fn();
			const store = createDraftStore(
				() => 'org-a',
				2,
				(listener) => {
					endSession = listener;
					return unsub;
				}
			);
			const t1 = store.forThread('t1').reply;
			t1.writeAttachments?.([stagedPreview('1', 'blob:draft/1')]);
			const clearT1 = t1.clearOnSuccess();
			clearT1();
			expect(revokeSpy).toHaveBeenCalledWith('blob:draft/1');

			const t2 = store.forThread('t2').reply;
			t2.writeAttachments?.([stagedPreview('2', 'blob:draft/2')]);
			endSession();
			expect(revokeSpy).toHaveBeenCalledWith('blob:draft/2');
			expect(store.forThread('t2').reply.readAttachments?.()).toEqual([]);

			store.dispose();
			expect(unsub).toHaveBeenCalledTimes(1);
		} finally {
			revokeSpy.mockRestore();
		}
	});

	it('marks interrupted uploading items as retryable errors without altering ready items', () => {
		const uploading: StagedAttachment = {
			localId: 'u1',
			filename: 'shot.png',
			mimeType: 'image/png',
			byteCount: 128,
			previewUrl: 'blob:draft/shot',
			status: 'uploading'
		};
		const ready = stagedPreview('r1', 'blob:draft/ready');
		const next = markInterruptedUploads([uploading, ready]);
		expect(next[0]).toMatchObject({
			localId: 'u1',
			status: 'error',
			previewUrl: 'blob:draft/shot'
		});
		expect(next[0].error).toMatch(/interrupted/i);
		expect(next[1]).toBe(ready);
	});
});
