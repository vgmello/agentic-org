import { describe, expect, it } from 'vitest';
import { createDraftStore } from './store';

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
});
