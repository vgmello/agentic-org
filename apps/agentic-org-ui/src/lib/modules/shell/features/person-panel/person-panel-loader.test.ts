import { describe, expect, it, vi } from 'vitest';
import { PersonPanelLoader } from './person-panel-loader.svelte';
import type { PersonPanelDataSource, PersonPanelLoadedData, PersonPanelProfile } from './types';

function fakeProfile(id: string, name: string): PersonPanelProfile {
	return {
		id,
		name,
		kind: 'agent',
		kindLabel: 'Agent',
		owner: false,
		role: 'Platform Engineer',
		status: 'active',
		isSelf: false,
		presenceLabel: 'Active',
		presenceTone: 'ok',
		homeTeam: { id: 'platform', name: 'Platform' },
		profileHref: `/members/${id}`,
		workTabHref: `/members/${id}/work`
	};
}

describe('PersonPanelLoader', () => {
	it('loads profile and open work while forwarding AbortSignal', async () => {
		const loadMember = vi.fn<PersonPanelDataSource['loadMember']>(
			async (_org, memberId, signal) => {
				expect(signal.aborted).toBe(false);
				return {
					profile: fakeProfile(memberId, 'Rowan'),
					openWork: [
						{
							id: 't1',
							ref: 'ORG-1',
							title: 'Custody lock fix',
							statusLabel: 'In progress',
							href: '/work/ORG-1'
						}
					],
					totalOpenWork: 4
				};
			}
		);

		const loader = new PersonPanelLoader({ loadMember });
		const { done } = loader.load('org-1', 'rowan');
		await done;

		expect(loader.loading).toBe(false);
		expect(loader.error).toBeUndefined();
		expect(loader.profile?.name).toBe('Rowan');
		expect(loader.openWork).toHaveLength(1);
		expect(loader.totalOpenWork).toBe(4);
		loader.dispose();
	});

	it('aborts the previous request and ignores stale responses when switching members', async () => {
		let resolveFirst: ((value: PersonPanelLoadedData) => void) | undefined;
		let firstSignal: AbortSignal | undefined;

		const loadMember = vi.fn<PersonPanelDataSource['loadMember']>((_org, memberId, signal) => {
			if (memberId === 'rowan') {
				firstSignal = signal;
				return new Promise<PersonPanelLoadedData>((resolve) => {
					resolveFirst = resolve;
				});
			}
			return Promise.resolve({
				profile: fakeProfile('maya', 'Maya Chen'),
				openWork: [],
				totalOpenWork: 0
			});
		});

		const loader = new PersonPanelLoader({ loadMember });
		const first = loader.load('org-1', 'rowan');
		const second = loader.load('org-1', 'maya');
		await second.done;

		expect(firstSignal?.aborted).toBe(true);
		expect(loader.profile?.name).toBe('Maya Chen');

		resolveFirst?.({
			profile: fakeProfile('rowan', 'Rowan'),
			openWork: [],
			totalOpenWork: 0
		});
		await first.done;

		expect(loader.profile?.name).toBe('Maya Chen');
		loader.dispose();
	});

	it('drops stale rejection when superseded and aborts cleanly on dispose', async () => {
		let rejectFirst: ((err: Error) => void) | undefined;
		let activeSignal: AbortSignal | undefined;

		const loadMember = vi.fn<PersonPanelDataSource['loadMember']>((_org, memberId, signal) => {
			activeSignal = signal;
			if (memberId === 'first') {
				return new Promise<PersonPanelLoadedData>((_resolve, reject) => {
					rejectFirst = reject;
				});
			}
			return new Promise<PersonPanelLoadedData>(() => undefined);
		});

		const loader = new PersonPanelLoader({ loadMember });
		const first = loader.load('org-1', 'first');
		const second = loader.load('org-1', 'second');

		rejectFirst?.(new Error('stale boom'));
		await first.done;
		expect(loader.error).toBeUndefined();

		loader.dispose();
		expect(activeSignal?.aborted).toBe(true);
		expect(loader.disposed).toBe(true);
		second.cancel();
	});
});
