import { afterEach, describe, expect, it, vi } from 'vitest';
import { buildHomeProjectHealthOverlays } from '$lib/modules/home/model';
import type { InboxItem } from '$lib/modules/inbox/types';
import type { ProjectRow } from '$lib/modules/projects/types';
import { deriveProjectHealth } from '$lib/platform/workspace/project-health';
import type { OfficeInfo, WorkItemSummary } from '$lib/proto';

const orgView = vi.fn();
const workSnapshot = vi.fn();
const inboxList = vi.fn();

vi.mock('$lib/modules/organization/api', () => ({
	orgApi: {
		view: (...args: unknown[]) => orgView(...args)
	}
}));
vi.mock('$lib/modules/work/client', () => ({
	workApi: {
		snapshot: (...args: unknown[]) => workSnapshot(...args)
	}
}));
vi.mock('$lib/modules/inbox/api', () => ({
	inboxApi: {
		list: (...args: unknown[]) => inboxList(...args)
	}
}));

import { ShellData } from './shell-data.svelte';

const baseOffices: OfficeInfo[] = [
	{
		id: 'office-1',
		organization_id: 'org-1',
		name: 'HQ',
		role: 'hq',
		host: 'This Mac',
		status: 'active'
	}
];

afterEach(() => {
	vi.clearAllMocks();
});

describe('ShellData inboxLoadedOrganizationId', () => {
	it('stays undefined when inboxApi.list rejects, sets on fulfillment, and clears on reset and dispose', async () => {
		orgView.mockResolvedValue({ snapshot: { members: [], teams: [], projects: [] } });
		workSnapshot.mockResolvedValue({ items: [] });
		inboxList.mockRejectedValueOnce(new Error('Inbox unavailable'));

		let currentOrg = 'org-1';
		const shell = new ShellData({
			getOrganizationId: () => currentOrg,
			getBaseOffices: () => baseOffices,
			loadOffices: async (_id, offices) => offices
		});

		const rejectedResult = await shell.load('org-1', true).promise;
		expect(rejectedResult).toEqual({ inboxUpdated: false, workUpdated: true });
		expect(shell.loaded).toBe(true);
		expect(shell.loadedOrganizationId).toBe('org-1');
		expect(shell.inboxLoadedOrganizationId).toBeUndefined();

		const inboxRows = [{ id: 'inbox-1', state: 'open' }] as unknown as InboxItem[];
		inboxList.mockResolvedValueOnce(inboxRows);
		const fulfilledResult = await shell.refresh();
		expect(fulfilledResult).toEqual({ inboxUpdated: true, workUpdated: true });
		expect(shell.inboxLoadedOrganizationId).toBe('org-1');
		expect(shell.inboxItems).toEqual(inboxRows);

		currentOrg = 'org-2';
		inboxList.mockRejectedValueOnce(new Error('Org 2 inbox unavailable'));
		const switchedResult = await shell.load('org-2', true).promise;
		expect(switchedResult).toEqual({ inboxUpdated: false, workUpdated: true });
		expect(shell.loadedOrganizationId).toBe('org-2');
		expect(shell.inboxLoadedOrganizationId).toBeUndefined();

		inboxList.mockResolvedValueOnce([]);
		await shell.refresh();
		expect(shell.inboxLoadedOrganizationId).toBe('org-2');

		shell.dispose();
		expect(shell.inboxLoadedOrganizationId).toBeUndefined();
	});

	it('forwards AbortSignal to all 4 shell loaders and aborts superseded or cancelled loads', async () => {
		const orgSignals: (AbortSignal | undefined)[] = [];
		const workSignals: (AbortSignal | undefined)[] = [];
		const inboxSignals: (AbortSignal | undefined)[] = [];
		const officeSignals: (AbortSignal | undefined)[] = [];

		orgView.mockImplementation((_id: string, signal?: AbortSignal) => {
			orgSignals.push(signal);
			return Promise.resolve({ snapshot: { members: [], teams: [], projects: [] } });
		});
		workSnapshot.mockImplementation(
			(_id: string, _p?: string, _c?: string, signal?: AbortSignal) => {
				workSignals.push(signal);
				return Promise.resolve({ items: [] });
			}
		);
		inboxList.mockImplementation((_id: string, signal?: AbortSignal) => {
			inboxSignals.push(signal);
			return Promise.resolve([]);
		});

		const shell = new ShellData({
			getOrganizationId: () => 'org-1',
			getBaseOffices: () => baseOffices,
			loadOffices: async (_id, offices, signal) => {
				officeSignals.push(signal);
				return offices;
			}
		});

		const first = shell.load('org-1', true);
		await Promise.resolve();
		expect(orgSignals[0]?.aborted).toBe(false);
		expect(workSignals[0]?.aborted).toBe(false);
		expect(inboxSignals[0]?.aborted).toBe(false);
		expect(officeSignals[0]?.aborted).toBe(false);

		const second = shell.load('org-1', false);
		await Promise.resolve();
		expect(orgSignals[0]?.aborted).toBe(true);
		expect(workSignals[0]?.aborted).toBe(true);
		expect(inboxSignals[0]?.aborted).toBe(true);
		expect(officeSignals[0]?.aborted).toBe(true);
		expect(orgSignals[1]?.aborted).toBe(false);

		await Promise.all([first.promise, second.promise]);

		const third = shell.load('org-1', false);
		third.cancel();
		expect(orgSignals[2]?.aborted).toBe(true);
		await third.promise;
	});

	it('stores frozen projectHealthById overlays, filters stale team IDs, ignores wrong org/post-dispose, and clears on reset', async () => {
		orgView.mockResolvedValue({
			snapshot: {
				members: [],
				teams: [{ id: 'team-1', name: 'Platform', version: 1 }],
				projects: [{ id: 'p-1', name: 'Federation' }],
				project_teams: [
					{ id: 'pt-valid', project_id: 'p-1', team_id: 'team-1', version: 1 },
					{ id: 'pt-stale', project_id: 'p-1', team_id: 'team-deleted', version: 1 }
				]
			}
		});
		workSnapshot.mockResolvedValue({ items: [] });
		inboxList.mockResolvedValue([]);

		let currentOrg = 'org-1';
		const shell = new ShellData({
			getOrganizationId: () => currentOrg,
			getBaseOffices: () => baseOffices,
			loadOffices: async (_id, offices) => offices
		});

		await shell.load('org-1', true).promise;

		// validProjectTeams strips dangling project_teams entries not present in snapshot.teams.
		expect(shell.validProjectTeams).toEqual([
			{ id: 'pt-valid', project_id: 'p-1', team_id: 'team-1', version: 1 }
		]);

		// Wrong organization updates are ignored.
		shell.setProjectHealthOverlays('org-other', {
			'p-1': { dueDate: '2026-11-20', blockedCount: 2, teamIds: ['team-1'] }
		});
		expect(shell.projectHealthById).toEqual({});

		// Active org overlays are frozen and stale teamIds are filtered against snapshot.teams.
		shell.setProjectHealthOverlays('org-1', {
			'p-1': {
				dueDate: '2026-11-20',
				blockedCount: 1,
				criticalBugCount: 2,
				teamIds: ['team-1', 'team-deleted']
			}
		});
		expect(Object.isFrozen(shell.projectHealthById)).toBe(true);
		expect(Object.isFrozen(shell.projectHealthById['p-1'])).toBe(true);
		expect(shell.projectHealthById['p-1']).toEqual({
			dueDate: '2026-11-20',
			blockedCount: 1,
			criticalBugCount: 2,
			teamIds: ['team-1']
		});

		// Switching org with reset=true clears stale overlays.
		currentOrg = 'org-2';
		await shell.load('org-2', true).promise;
		expect(shell.projectHealthById).toEqual({});

		// Updates after dispose() are ignored.
		shell.dispose();
		shell.setProjectHealthOverlays('org-2', { 'p-1': { dueDate: '2026-12-01' } });
		expect(shell.projectHealthById).toEqual({});
	});

	it('clears volatile Home counts on live refresh and on leaving Home while preserving dueDate and teamIds (N2)', async () => {
		const nowMs = Date.parse('2026-11-27T12:00:00Z');
		orgView.mockResolvedValue({
			snapshot: {
				members: [],
				teams: [{ id: 'team-1', name: 'Platform', version: 1 }],
				projects: [{ id: 'p-1', name: 'Federation' }],
				project_teams: []
			}
		});
		inboxList.mockResolvedValue([]);
		workSnapshot.mockResolvedValueOnce({ items: [] });

		const shell = new ShellData({
			getOrganizationId: () => 'org-1',
			getBaseOffices: () => baseOffices,
			loadOffices: async (_id, offices) => offices
		});
		await shell.load('org-1', true).promise;

		// Home publishes volatile counts + dueDate + teamIds -> sidebar reflects Off track.
		shell.setProjectHealthOverlays('org-1', {
			'p-1': {
				dueDate: '2026-11-20',
				doneCount: 1,
				totalCount: 2,
				blockedCount: 1,
				criticalBugCount: 1,
				teamIds: ['team-1']
			}
		});
		expect(
			deriveProjectHealth({
				projectId: 'p-1',
				workItems: shell.workItems,
				inboxItems: shell.inboxItems,
				projectTeams: shell.validProjectTeams,
				overlay: shell.projectHealthById['p-1'],
				nowMs
			}).tone
		).toBe('bad');

		// Live refresh lands with all work completed & 0 blockers/bugs -> volatile counts stripped,
		// dueDate + teamIds preserved, sidebar immediately transitions to On track!
		const doneTask: WorkItemSummary = {
			id: 'w-1',
			kind: 'task',
			project_id: 'p-1',
			short_id: 'ORG-1',
			title: 'Finished task',
			status: 'done',
			created_by: 'owner',
			created_at: '2026-11-20T10:00:00Z',
			updated_at: '2026-11-27T10:00:00Z',
			severity_confirmed: false,
			reopen_count: 0,
			fixes: [],
			fixed_by: [],
			checklist: { total: 0, done: 0, required_left: 0 },
			done_anyway: false,
			owner_office_id: 'office-1',
			generation: 1,
			version: 1,
			deleted: false
		};
		workSnapshot.mockResolvedValueOnce({ items: [doneTask] });
		await shell.refresh();

		expect(shell.projectHealthById['p-1']).toEqual({
			dueDate: '2026-11-20',
			teamIds: ['team-1']
		});
		expect(
			deriveProjectHealth({
				projectId: 'p-1',
				workItems: shell.workItems,
				inboxItems: shell.inboxItems,
				projectTeams: shell.validProjectTeams,
				overlay: shell.projectHealthById['p-1'],
				nowMs
			})
		).toEqual({ tone: 'ok', label: 'On track', statusPillClass: 't-green' });

		// Leaving Home (clearVolatileProjectHealthCounts) also clears volatile counts immediately while keeping dueDate & teamIds.
		shell.setProjectHealthOverlays('org-1', {
			'p-1': {
				dueDate: '2026-11-20',
				doneCount: 0,
				totalCount: 2,
				blockedCount: 2,
				teamIds: ['team-1']
			}
		});
		shell.clearVolatileProjectHealthCounts('org-1');
		expect(shell.projectHealthById['p-1']).toEqual({
			dueDate: '2026-11-20',
			teamIds: ['team-1']
		});

		// Preserving dueDate on failed homeApi.projects retains dueDate without losing it.
		shell.setProjectHealthOverlays(
			'org-1',
			{ 'p-1': { teamIds: ['team-1'] } },
			{ preserveDueDate: true }
		);
		expect(shell.projectHealthById['p-1']).toEqual({
			dueDate: '2026-11-20',
			teamIds: ['team-1']
		});
		shell.dispose();
	});

	it('preserves Home overlays published while shell.load() is in flight, strips on subsequent refresh, and guards wrong-org/post-dispose clears (M1, L2)', async () => {
		const nowMs = Date.parse('2026-11-27T12:00:00Z');
		orgView.mockResolvedValue({
			snapshot: {
				members: [],
				teams: [{ id: 'team-1', name: 'Platform', version: 1 }],
				projects: [
					{ id: 'p-bug', name: 'Office federation' },
					{ id: 'p-stale-team', name: 'Unassigned project' }
				],
				project_teams: []
			}
		});
		inboxList.mockResolvedValue([]);

		let resolveInitialWork!: (v: { items: WorkItemSummary[] }) => void;
		workSnapshot.mockImplementationOnce(
			() =>
				new Promise((resolve) => {
					resolveInitialWork = resolve;
				})
		);

		const shell = new ShellData({
			getOrganizationId: () => 'org-1',
			getBaseOffices: () => baseOffices,
			loadOffices: async (_id, offices) => offices
		});

		// Start initial shell.load (seq=1); Home finishes publishing BEFORE shell.load settles.
		const initialLoad = shell.load('org-1', true);
		await Promise.resolve();
		const rows = makeParityRows();
		const homeProjects = [
			{
				id: 'p-bug',
				name: 'Office federation',
				done: 1,
				total: 2,
				open: 1,
				blocked: 0,
				members: 2,
				due: '2026-11-20'
			}
		];
		shell.setProjectHealthOverlays('org-1', buildHomeProjectHealthOverlays(rows, homeProjects));

		// Initial shell.load (seq=1) now settles -> MUST NOT wipe Home's in-flight volatile counts.
		resolveInitialWork({ items: [] });
		await initialLoad.promise;

		expect(shell.projectHealthById['p-bug']).toEqual({
			dueDate: '2026-11-20',
			doneCount: 1,
			totalCount: 2,
			criticalBugCount: 1,
			teamIds: ['team-1']
		});
		const sidebarHealth = (projectId: string) =>
			deriveProjectHealth({
				projectId,
				workItems: shell.workItems,
				inboxItems: shell.inboxItems,
				projectTeams: shell.validProjectTeams,
				overlay: shell.projectHealthById[projectId],
				nowMs
			});
		expect(sidebarHealth('p-bug')).toEqual({
			tone: 'bad',
			label: 'Off track',
			statusPillClass: 't-red'
		});
		expect(sidebarHealth('p-stale-team')).toEqual({
			tone: 'warn',
			label: 'At risk',
			statusPillClass: 't-amber'
		});

		// Wrong-org clearVolatileProjectHealthCounts is safely ignored.
		shell.clearVolatileProjectHealthCounts('org-other');
		expect(shell.projectHealthById['p-bug']?.criticalBugCount).toBe(1);

		// A subsequent shell.refresh() (seq=2 > 1) strips volatile counts and reflects live workItems.
		workSnapshot.mockResolvedValueOnce({ items: [] });
		await shell.refresh();
		expect(shell.projectHealthById['p-bug']).toEqual({
			dueDate: '2026-11-20',
			teamIds: ['team-1']
		});
		expect(sidebarHealth('p-bug')).toEqual({
			tone: 'ok',
			label: 'On track',
			statusPillClass: 't-green'
		});

		// Post-dispose clearVolatileProjectHealthCounts is safely ignored.
		shell.setProjectHealthOverlays('org-1', {
			'p-bug': { dueDate: '2026-11-20', blockedCount: 2 }
		});
		shell.dispose();
		shell.clearVolatileProjectHealthCounts('org-1');
		expect(shell.projectHealthById['p-bug']?.blockedCount).toBe(2);
	});
});

function makeParityRows(): ProjectRow[] {
	return [
		{
			id: 'p-bug',
			number: 1,
			slug: 'office-federation',
			name: 'Office federation',
			ownerTeam: { id: 'team-1', name: 'Platform', ref: 'TEAM-1' },
			contributors: [],
			archived: false,
			tasks: { todo: 1, inProgress: 0, blocked: 0, done: 1, cancelled: 0 },
			bugs: { critical: 1, major: 0, minor: 0, trivial: 0, unset: 0 },
			developmentProcess: null
		},
		{
			id: 'p-stale-team',
			number: 2,
			slug: 'unassigned',
			name: 'Unassigned project',
			ownerTeam: { id: 'team-deleted', name: 'Ghost', ref: 'TEAM-9' },
			contributors: [],
			archived: false,
			tasks: { todo: 0, inProgress: 0, blocked: 0, done: 0, cancelled: 0 },
			bugs: { critical: 0, major: 0, minor: 0, trivial: 0, unset: 0 },
			developmentProcess: null
		}
	];
}
