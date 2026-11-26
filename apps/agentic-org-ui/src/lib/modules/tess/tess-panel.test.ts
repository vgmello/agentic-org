import { describe, expect, it, vi } from 'vitest';
import { isBuiltInAgent, type Member, type OfficeInfo } from '$lib/proto';
import { sampleOrg } from '$lib/modules/organization/sample';
import { TessPanelState } from './tess-panel.svelte';
import type { TessApi } from './types';

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

function createStubApi(overrides: Partial<TessApi> = {}): TessApi {
	return {
		ask: vi.fn(async () => ({ kind: 'unavailable' as const, reason: 'Offline' })),
		streamTurn: vi.fn(async () => undefined),
		cancelTurn: vi.fn(async () => true),
		applyProposal: vi.fn(async () => ({
			ok: true as const,
			receipt: { summary: 'Applied.', entityHref: '/projects/p-2', entityLabel: 'Open project' }
		})),
		dismissProposal: vi.fn(async () => true),
		resetSession: vi.fn(async () => true),
		...overrides
	};
}

describe('TessPanelState', () => {
	it('toggles cleanly, records badged offline_summary turns, and preserves decision context', () => {
		const panel = new TessPanelState(createStubApi());
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

	it('returns an honest capability note for write/how-to prompts when offline', () => {
		const panel = new TessPanelState(createStubApi());
		const turn = panel.ask('Create an iOS project with Kanban', {
			snapshot: sampleOrg('hq'),
			items: [],
			offices,
			ownerId: 'you',
			routeLabel: 'Projects'
		});
		expect(turn?.answer.headline).toContain('Live Tess runtime is not connected');
	});

	it('streams live turns, handles Action Proposal apply/dismiss, and supports Stop/reset', async () => {
		const api = createStubApi({
			ask: vi.fn(async () => ({
				kind: 'streaming' as const,
				turnId: 'srv-1',
				streamUrl: '/api/v1/organizations/org-1/tess/turns/srv-1/stream'
			})),
			streamTurn: vi.fn(async (_org, _turnId, _url, handlers) => {
				handlers.onEvent({ type: 'turn.started', turnId: 'srv-1' });
				handlers.onEvent({
					type: 'tool.progress',
					call: {
						id: 'c1',
						label: 'Checked 3 teams',
						state: 'done',
						startedAt: 10,
						durationMs: 420
					}
				});
				handlers.onEvent({ type: 'text.delta', delta: 'Prepared your new project.' });
				handlers.onEvent({
					type: 'ui.block',
					block: {
						kind: 'action_proposal',
						proposal: {
							id: 'sug-1',
							kind: 'project.create',
							title: 'Create project',
							risk: 'reversible',
							ctaLabel: 'Create project',
							parameters: [{ label: 'Name', value: 'iOS App' }],
							status: 'pending'
						}
					}
				});
				handlers.onEvent({
					type: 'turn.completed',
					turnId: 'srv-1',
					followUps: ['Add tasks to iOS App']
				});
				handlers.onDone?.();
			})
		});

		const panel = new TessPanelState(api);
		const turn = panel.ask('Create an iOS project', {
			organizationId: 'org-1',
			snapshot: sampleOrg('hq'),
			items: [],
			offices,
			ownerId: 'you',
			routeLabel: 'Projects',
			routePath: '/projects'
		});
		expect(turn).toBeDefined();
		await new Promise((r) => setTimeout(r, 0));

		expect(panel.runtimeStatus).toBe('ready');
		const liveTurn = panel.turns[0];
		expect(liveTurn.mode).toBe('live');
		expect(liveTurn.streamText).toBe('Prepared your new project.');
		expect(liveTurn.toolCalls).toHaveLength(1);
		expect(liveTurn.followUps).toEqual(['Add tasks to iOS App']);

		await panel.applyProposal(liveTurn.id, 'sug-1');
		const updatedBlock = panel.turns[0].blocks?.[0];
		expect(updatedBlock?.kind === 'action_proposal' && updatedBlock.proposal.status).toBe(
			'applied'
		);
		expect(updatedBlock?.kind === 'action_proposal' && updatedBlock.proposal.receipt?.summary).toBe(
			'Applied.'
		);

		await panel.resetSession('org-1');
		expect(panel.turns).toHaveLength(0);
		expect(api.resetSession).toHaveBeenCalledWith('org-1');
	});

	it('resets dismissed/decision context on route navigation without closing the panel', () => {
		const panel = new TessPanelState(createStubApi());
		panel.openPanel();
		panel.dismissContext();
		panel.noteRouteChange();
		expect(panel.open).toBe(true);
		expect(panel.contextDismissed).toBe(false);
	});

	it('clears cross-org turns on setOrganization and rolls back failed dismissProposal', async () => {
		const api = createStubApi({
			ask: vi.fn(async () => ({
				kind: 'streaming' as const,
				turnId: 'srv-2',
				streamUrl: '/api/v1/organizations/org-1/tess/turns/srv-2/stream'
			})),
			streamTurn: vi.fn(async (_org, _turnId, _url, handlers) => {
				handlers.onEvent({
					type: 'ui.block',
					block: {
						kind: 'action_proposal',
						proposal: {
							id: 'sug-fail',
							kind: 'project.create',
							title: 'Create project',
							risk: 'reversible',
							ctaLabel: 'Create project',
							parameters: [],
							status: 'pending'
						}
					}
				});
				handlers.onDone?.();
			}),
			dismissProposal: vi.fn(async () => false)
		});

		const panel = new TessPanelState(api);
		panel.ask('Create project', {
			organizationId: 'org-1',
			snapshot: sampleOrg('hq'),
			items: [],
			offices,
			ownerId: 'you',
			routeLabel: 'Projects'
		});
		await new Promise((r) => setTimeout(r, 0));

		const turnId = panel.turns[0].id;
		await panel.dismissProposal(turnId, 'sug-fail');
		const block = panel.turns[0].blocks?.[0];
		expect(block?.kind === 'action_proposal' && block.proposal.status).toBe('pending');
		expect(block?.kind === 'action_proposal' && block.proposal.errorMessage).toContain(
			'Could not dismiss'
		);

		panel.setOrganization('org-2');
		expect(panel.turns).toHaveLength(0);
		expect(panel.runtimeStatus).toBe('offline');
	});
});

describe('isBuiltInAgent', () => {
	const baseAgent = (overrides: Partial<Member>): Member => ({
		id: 'm-1',
		kind: 'agent',
		name: 'Ada',
		handle: 'ada',
		version: 1,
		agent: {
			role: 'Staff Engineer',
			title: 'Staff Engineer',
			runtime: 'claude_code',
			messaging_name: 'ada',
			status: 'active'
		},
		...overrides
	});

	it('recognizes built-in HR and Tess (chief_of_staff) agents and excludes hired agents or people', () => {
		expect(isBuiltInAgent(baseAgent({ name: 'HR', handle: 'hr' }))).toBe(true);
		expect(
			isBuiltInAgent(
				baseAgent({
					name: 'Tess',
					handle: 'tess',
					agent: {
						role: 'Chief of Staff',
						title: 'Chief of Staff',
						runtime: 'claude_code',
						messaging_name: 'tess',
						status: 'active'
					}
				})
			)
		).toBe(true);
		expect(
			isBuiltInAgent({
				...baseAgent({ name: 'Custom', handle: 'custom' }),
				builtin_key: 'chief_of_staff'
			} as Member)
		).toBe(true);
		expect(isBuiltInAgent(baseAgent({}))).toBe(false);
		expect(isBuiltInAgent(baseAgent({ name: 'HR', handle: 'recruiter-bot' }))).toBe(false);
		expect(isBuiltInAgent(baseAgent({ name: 'Tess', handle: 'tess' }))).toBe(false);
		expect(
			isBuiltInAgent({
				id: 'p-1',
				kind: 'person',
				name: 'Tess',
				handle: 'tess',
				version: 1
			})
		).toBe(false);
		expect(isBuiltInAgent(undefined)).toBe(false);
	});
});
