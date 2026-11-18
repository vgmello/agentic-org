import { describe, expect, it } from 'vitest';
import { createUnsavedQuery } from './unsaved-query';

/** Two tabs joined by an in-memory channel pair; a message is never delivered to its sender. */
function bus() {
	const tabs: EventTarget[] = [];
	const channel = () => {
		const target = new EventTarget();
		tabs.push(target);
		return {
			postMessage(data: unknown) {
				for (const other of tabs) {
					if (other !== target) queueMicrotask(() => other.dispatchEvent(new MessageEvent('message', { data })));
				}
			},
			addEventListener: target.addEventListener.bind(target),
			removeEventListener: target.removeEventListener.bind(target)
		};
	};
	return { channel };
}

describe('B6 cross-tab unsaved text (SIGN-19)', () => {
	it('is true when another tab of this HQ holds unsaved text', async () => {
		const { channel } = bus();
		let otherText = '';
		const mine = createUnsavedQuery({ channel: channel(), local: () => false, timeoutMs: 50 });
		createUnsavedQuery({ channel: channel(), local: () => otherText.length > 0, timeoutMs: 50 });
		expect(await mine.anyUnsaved()).toBe(false);
		otherText = 'draft';
		expect(await mine.anyUnsaved()).toBe(true);
	});
	it('answers from this tab without asking, and ignores replies to other queries', async () => {
		const { channel } = bus();
		const posted: unknown[] = [];
		const own = channel();
		const mine = createUnsavedQuery({ channel: { ...own, postMessage: (d) => posted.push(d) }, local: () => true });
		expect(await mine.anyUnsaved()).toBe(true);
		expect(posted).toEqual([]);
		const peer = channel();
		const quiet = createUnsavedQuery({ channel: own, local: () => false, timeoutMs: 30, newId: () => 'mine' });
		const pending = quiet.anyUnsaved();
		peer.postMessage({ kind: 'unsaved', id: 'someone-else', unsaved: true });
		peer.postMessage({ kind: 'unsaved', id: 'mine', unsaved: 'yes' });
		expect(await pending).toBe(false);
	});
	it('stops answering after dispose', async () => {
		const { channel } = bus();
		const mine = createUnsavedQuery({ channel: channel(), local: () => false, timeoutMs: 30 });
		const other = createUnsavedQuery({ channel: channel(), local: () => true, timeoutMs: 30 });
		other.dispose();
		expect(await mine.anyUnsaved()).toBe(false);
	});
});
