import { describe, expect, it } from 'vitest';
import type { OfficeInfo } from '$lib/proto';
import { sampleOrg } from '$lib/modules/organization/sample';
import { TessPanelState } from './tess-panel.svelte';

const offices: OfficeInfo[] = [
	{
		id: 'hq',
		organization_id: 'org-1',
		name: 'HQ',
		role: 'hq',
		host: 'This Mac',
		status: 'active'
	}
];

describe('TessPanelState', () => {
	it('toggles cleanly, records badged offline_summary turns, and preserves decision context', () => {
		const panel = new TessPanelState();
		const snap = sampleOrg('hq');

		expect(panel.open).toBe(false);
		expect(panel.runtimeStatus).toBe('offline');
		panel.toggle();
		expect(panel.open).toBe(true);

		const turn = panel.ask(
			'Explain the risk, impact, and recommendation for: Approve release v1',
			{
				snapshot: snap,
				items: [],
				offices,
				ownerId: 'you',
				routeLabel: 'Home'
			},
			{
				decisionId: 'd-1',
				decision: {
					id: 'd-1',
					title: 'Approve release v1',
					recommendation: 'Ada recommends approving release v1.',
					risk: { label: 'Low risk', sub: 'Reversible gate' },
					impact: { label: 'High impact', sub: 'Unblocks launch' },
					backgroundContext: 'All 412 checks passed.',
					detailHref: '/inbox?item=d-1',
					projectName: 'Atlas'
				}
			}
		);

		expect(turn?.mode).toBe('offline_summary');
		expect(turn?.contextLabel).toBe('Approve release v1');
		expect(turn?.answer.headline).toContain('Approve release v1');
		expect(panel.turns).toHaveLength(1);
		expect(panel.decisionContext?.id).toBe('d-1');

		panel.dismissContext();
		expect(panel.contextDismissed).toBe(true);
		expect(panel.decisionContext).toBeUndefined();

		const turn2 = panel.ask('Budget, offices & active agents', {
			snapshot: snap,
			items: [],
			offices,
			ownerId: 'you',
			routeLabel: 'Home'
		});
		expect(turn2?.contextLabel).toBeUndefined();
		expect(panel.turns).toHaveLength(2);

		panel.clear();
		expect(panel.turns).toHaveLength(0);
	});

	it('resets dismissed/decision context on route navigation without closing the panel', () => {
		const panel = new TessPanelState();
		panel.openPanel();
		panel.dismissContext();
		panel.noteRouteChange();
		expect(panel.open).toBe(true);
		expect(panel.contextDismissed).toBe(false);
	});
});
