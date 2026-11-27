import { describe, expect, it, vi } from 'vitest';
import type { InboxItem } from '$lib/modules/inbox/types';
import { sampleOrg } from '$lib/modules/organization/sample';
import { deriveDecisionSpotlight } from '$lib/modules/home/model';
import { SpotlightController } from './spotlight-controller.svelte';

function makeQuestionDecision(id = 'q-1') {
	const snap = sampleOrg('hq');
	const [decision] = deriveDecisionSpotlight([], snap, 'you', Date.parse('2026-11-27T12:00:00Z'), [
		{
			id,
			kind: 'question',
			source: 'Chat ask',
			title: 'Confirm rollout window',
			who: { id: 'ada', name: 'Ada', kind: 'agent' },
			why: 'Ready for cutover.',
			created_at: '2026-11-27T11:00:00Z',
			actions: ['reply'],
			state: 'open'
		}
	]);
	return { snap, decision };
}

function deferred<T>() {
	let resolve!: (value: T) => void;
	let reject!: (reason?: unknown) => void;
	const promise = new Promise<T>((res, rej) => {
		resolve = res;
		reject = rej;
	});
	return { promise, resolve, reject };
}

function handledReplyItem(id: string, outcome?: string): InboxItem {
	return {
		id,
		kind: 'question',
		source: 'Chat ask',
		title: 'Confirm rollout window',
		who: { id: 'ada', name: 'Ada', kind: 'agent' },
		why: 'Ready for cutover.',
		created_at: '2026-11-27T11:00:00Z',
		actions: ['reply'],
		state: 'handled',
		handled: outcome ? { outcome, at: '2026-11-27T12:01:00Z' } : undefined
	};
}

describe('SpotlightController', () => {
	it('ignores stale completion when switching org mid-action (no receipt, toast, navigation, or refresh)', async () => {
		let currentOrg = 'org-a';
		const pending = deferred<{ message: string; navigateTo?: string }>();
		const navigate = vi.fn(async () => {});
		const notifySuccess = vi.fn();
		const notifyError = vi.fn();
		const refresh = vi.fn(async () => {});
		const onHandled = vi.fn();

		const { snap, decision } = makeQuestionDecision('d-1');
		const ctrl = new SpotlightController({
			getOrgId: () => currentOrg,
			executeAction: () => pending.promise,
			navigate,
			notifySuccess,
			notifyError,
			refresh
		});

		const task = ctrl.runAction(decision, 'primary', snap, '', onHandled);
		expect(ctrl.isBusyFor('d-1')).toBe(true);

		// Switch to org-b mid-flight
		currentOrg = 'org-b';
		expect(ctrl.isOrgBusy()).toBe(false);
		expect(ctrl.isBusyFor('d-1')).toBe(false);

		pending.resolve({ message: 'Approved', navigateTo: '/work' });
		const settled = await task;

		expect(settled).toBe(false);
		expect(onHandled).not.toHaveBeenCalled();
		expect(notifySuccess).not.toHaveBeenCalled();
		expect(notifyError).not.toHaveBeenCalled();
		expect(navigate).not.toHaveBeenCalled();
		expect(refresh).not.toHaveBeenCalled();
		expect(ctrl.sessionHandledCount).toBe(0);
		expect(ctrl.lastReceipt).toBeUndefined();
		currentOrg = 'org-a';
		expect(ctrl.sessionHandledCount).toBe(0);
		expect(ctrl.handledReceipts).toEqual({});
	});

	it('writes no state or side effects when disposed mid-flight', async () => {
		const pending = deferred<InboxItem>();
		const notifySuccess = vi.fn();
		const notifyError = vi.fn();
		const refresh = vi.fn(async () => {});
		const onHandled = vi.fn();

		const { decision } = makeQuestionDecision('q-dispose');
		const ctrl = new SpotlightController({
			getOrgId: () => 'org-a',
			actReply: () => pending.promise,
			navigate: async () => {},
			notifySuccess,
			notifyError,
			refresh
		});

		const task = ctrl.submitReply(decision, 'Ship at 14:00 UTC', onHandled);
		expect(ctrl.isBusyFor('q-dispose')).toBe(true);

		ctrl.dispose();
		expect(ctrl.destroyed).toBe(true);
		expect(ctrl.busyAction).toBeUndefined();

		pending.resolve(handledReplyItem('q-dispose', 'Replied in thread'));
		const settled = await task;

		expect(settled).toBe(false);
		expect(onHandled).not.toHaveBeenCalled();
		expect(notifySuccess).not.toHaveBeenCalled();
		expect(notifyError).not.toHaveBeenCalled();
		expect(refresh).not.toHaveBeenCalled();
		expect(ctrl.sessionHandledCount).toBe(0);
		expect(ctrl.handledReceipts).toEqual({});
	});

	it('does not block org B while org A has an in-flight action', async () => {
		let currentOrg = 'org-a';
		const orgAPending = deferred<InboxItem>();
		const orgBPending = deferred<InboxItem>();
		const refresh = vi.fn(async () => {});
		const notifySuccess = vi.fn();

		const { decision: decA } = makeQuestionDecision('q-a');
		const { decision: decB } = makeQuestionDecision('q-b');
		const ctrl = new SpotlightController({
			getOrgId: () => currentOrg,
			actReply: (orgId) => (orgId === 'org-a' ? orgAPending.promise : orgBPending.promise),
			navigate: async () => {},
			notifySuccess,
			notifyError: vi.fn(),
			refresh
		});

		const taskA = ctrl.submitReply(decA, 'Answer A');
		expect(ctrl.isOrgBusy('org-a')).toBe(true);

		// Switch to org B: org B is free to submit immediately
		currentOrg = 'org-b';
		expect(ctrl.isOrgBusy()).toBe(false);
		const taskB = ctrl.submitReply(decB, 'Answer B');
		expect(ctrl.isBusyFor('q-b')).toBe(true);

		// Org A settling late must not clear org B's busy state or write receipts
		orgAPending.resolve(handledReplyItem('q-a', 'Answered A'));
		await taskA;
		expect(ctrl.isBusyFor('q-b')).toBe(true);

		// Org B completes cleanly
		orgBPending.resolve(handledReplyItem('q-b', 'Answered B'));
		const handledB = await taskB;
		expect(handledB).toBe(true);
		expect(ctrl.isOrgBusy()).toBe(false);
		expect(ctrl.lastReceipt).toEqual({
			title: 'Confirm rollout window',
			outcome: 'Answered B',
			note: 'Answer B'
		});
		expect(refresh).toHaveBeenCalledTimes(1);
		expect(notifySuccess).toHaveBeenCalledTimes(1);
	});

	it('records reply receipt using returned handled.outcome and invokes refresh', async () => {
		const refresh = vi.fn(async () => {});
		const notifySuccess = vi.fn();
		const onHandled = vi.fn();
		const { decision } = makeQuestionDecision('q-ok');

		const ctrl = new SpotlightController({
			getOrgId: () => 'org-1',
			actReply: async () => handledReplyItem('q-ok', 'Replied by owner'),
			navigate: async () => {},
			notifySuccess,
			notifyError: vi.fn(),
			refresh
		});

		const handled = await ctrl.submitReply(decision, '  Start with EU region  ', onHandled);
		expect(handled).toBe(true);
		expect(onHandled).toHaveBeenCalledTimes(1);
		expect(refresh).toHaveBeenCalledTimes(1);
		expect(notifySuccess).toHaveBeenCalledWith('Sent answer to Ada.');
		expect(ctrl.sessionHandledCount).toBe(1);
		expect(ctrl.handledReceipts['q-ok']).toEqual({
			outcome: 'Replied by owner',
			note: 'Start with EU region'
		});
		expect(ctrl.lastReceipt).toEqual({
			title: 'Confirm rollout window',
			outcome: 'Replied by owner',
			note: 'Start with EU region'
		});

		ctrl.dismissLastReceipt();
		expect(ctrl.lastReceipt).toBeUndefined();
	});
});
