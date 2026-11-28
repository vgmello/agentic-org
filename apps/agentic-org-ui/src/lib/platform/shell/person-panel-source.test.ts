import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { MemberProfile } from '$lib/modules/members/types';
import type { TaskRow } from '$lib/modules/workitems/types';
import type { WorkItemSummary } from '$lib/proto';

const { profileMock, queueMock } = vi.hoisted(() => ({
	profileMock: vi.fn(),
	queueMock: vi.fn()
}));

vi.mock('$lib/modules/members/api', () => ({
	membersApi: {
		profile: profileMock
	}
}));

vi.mock('$lib/modules/workitems/api', () => ({
	workItemsApi: {
		queue: queueMock
	}
}));

import { createPersonPanelDataSource, personPanelDataSource } from './person-panel-source';

function makeProfile(
	id: string,
	name: string,
	overrides: Partial<MemberProfile> = {}
): MemberProfile {
	return {
		id,
		handle: id,
		name,
		kind: 'agent',
		owner: false,
		canSignIn: false,
		role: 'Platform Engineer',
		status: 'active',
		homeTeam: { id: 'platform', name: 'Platform', ref: 'TEAM-1' },
		office: { id: 'hq', name: 'HQ' },
		presence: { id: 'working' },
		directReports: 0,
		leads: [],
		allowHiring: { on: false, canChange: false },
		viewer: { isSelf: false, isOwnerOrAdmin: false, costVisible: false },
		...overrides
	};
}

function makeTask(
	id: string,
	ref: string,
	assigneeId: string | null,
	status: TaskRow['status'] = 'in_progress'
): TaskRow {
	return {
		id,
		ref,
		revision: 1,
		type: 'task',
		title: `Task ${ref}`,
		status,
		priority: 'normal',
		severity: null,
		effort: null,
		project: { id: 'p1', name: 'Office federation', number: 1 },
		team: { id: 'platform', name: 'Platform' },
		assignedTeam: null,
		assignee: assigneeId ? { id: assigneeId, name: assigneeId, kind: 'agent' } : null,
		reporter: { id: 'owner', name: 'Owner', kind: 'person' },
		createdAt: '2026-11-27T09:00:00Z',
		updatedAt: '2026-11-27T10:00:00Z',
		statusAt: '2026-11-27T10:00:00Z',
		checklist: { done: 0, total: 0, requiredLeft: 0 },
		blockedReason: null,
		reopenCount: 0,
		needsMe: null,
		budget: null
	};
}

function makeSummary(
	id: string,
	ref: string,
	assigneeId: string,
	status: WorkItemSummary['status'] = 'in_progress'
): WorkItemSummary {
	return {
		id,
		kind: 'task',
		short_id: ref,
		title: `Summary ${ref}`,
		status,
		assignee_id: assigneeId,
		created_by: 'owner',
		created_at: '2026-11-27T09:00:00Z',
		updated_at: '2026-11-27T10:00:00Z',
		severity_confirmed: false,
		reopen_count: 0,
		fixes: [],
		fixed_by: [],
		checklist: { total: 0, done: 0, required_left: 0 },
		done_anyway: false,
		owner_office_id: 'hq',
		generation: 1,
		version: 1,
		deleted: false
	};
}

describe('personPanelDataSource', () => {
	beforeEach(() => {
		profileMock.mockReset();
		queueMock.mockReset();
	});

	it('forwards AbortSignal, falls back to at most 1 queue call when unhydrated, and caps open work at 3', async () => {
		profileMock.mockResolvedValue(makeProfile('rowan', 'Rowan'));
		queueMock.mockResolvedValue({
			rows: [
				makeTask('t1', 'ORG-1', 'rowan', 'in_progress'),
				makeTask('t2', 'ORG-2', 'rowan', 'todo'),
				makeTask('t3', 'ORG-3', 'rowan', 'blocked'),
				makeTask('t4', 'ORG-4', 'rowan', 'todo'),
				makeTask('t5', 'ORG-5', 'rowan', 'done'),
				makeTask('t6', 'ORG-6', 'maya', 'in_progress')
			],
			viewer: { employee: null, teamIds: [], owner: true },
			needsMeSources: []
		});

		const controller = new AbortController();
		const result = await personPanelDataSource.loadMember('org-1', 'rowan', controller.signal);

		expect(profileMock).toHaveBeenCalledWith('org-1', 'rowan', controller.signal);
		expect(queueMock).toHaveBeenCalledTimes(1);
		expect(queueMock).toHaveBeenCalledWith('org-1', { kind: 'organization' }, controller.signal);
		expect(result.error).toBeUndefined();
		expect(result.profile?.name).toBe('Rowan');
		expect(result.profile?.presenceLabel).toBe('Working');
		expect(result.profile?.profileHref).toBe('/members/rowan');
		expect(result.profile?.workTabHref).toBe('/members/rowan/work');
		expect(result.totalOpenWork).toBe(4);
		expect(result.openWorkUnavailable).toBeUndefined();
		expect(result.openWork.map((item) => item.ref)).toEqual(['ORG-1', 'ORG-2', 'ORG-3']);
	});

	it('marks openWorkUnavailable when the fallback workItemsApi.queue call fails', async () => {
		profileMock.mockResolvedValue(makeProfile('rowan', 'Rowan'));
		queueMock.mockRejectedValue(new Error('Work queue offline'));

		const result = await personPanelDataSource.loadMember(
			'org-1',
			'rowan',
			new AbortController().signal
		);

		expect(result.error).toBeUndefined();
		expect(result.profile?.name).toBe('Rowan');
		expect(result.openWork).toEqual([]);
		expect(result.totalOpenWork).toBe(0);
		expect(result.openWorkUnavailable).toBe(true);
	});

	it('makes 0 workItemsApi.queue calls when profile.openWork is returned by membersApi.profile', async () => {
		profileMock.mockResolvedValue(
			makeProfile('rowan', 'Rowan', {
				openWork: {
					total: 2,
					items: [
						{ id: 't1', ref: 'ORG-1', title: 'Sync federation', status: 'in_progress' },
						{ id: 't2', ref: 'ORG-2', title: 'Fix relay', status: 'blocked' }
					]
				}
			})
		);

		const result = await personPanelDataSource.loadMember(
			'org-1',
			'rowan',
			new AbortController().signal
		);

		expect(queueMock).not.toHaveBeenCalled();
		expect(result.totalOpenWork).toBe(2);
		expect(result.openWork.map((item) => item.ref)).toEqual(['ORG-1', 'ORG-2']);
		expect(result.openWork[1]?.statusLabel).toBe('Blocked');
	});

	it('reuses hydrated shellData.workItems with 0 workItemsApi.queue calls', async () => {
		profileMock.mockResolvedValue(makeProfile('rowan', 'Rowan'));
		const source = createPersonPanelDataSource(() => ({
			organizationId: 'org-1',
			workLoaded: true,
			workItems: [
				makeSummary('s1', 'ORG-10', 'rowan', 'in_progress'),
				makeSummary('s2', 'ORG-11', 'rowan', 'done'),
				makeSummary('s3', 'ORG-12', 'maya', 'todo')
			]
		}));

		const result = await source.loadMember('org-1', 'rowan', new AbortController().signal);
		expect(queueMock).not.toHaveBeenCalled();
		expect(result.totalOpenWork).toBe(1);
		expect(result.openWork.map((item) => item.ref)).toEqual(['ORG-10']);
	});

	it('trusts builtin_key exclusively when present and falls back to handle/role otherwise', async () => {
		profileMock.mockResolvedValue(
			makeProfile('tess-custom', 'Tess', {
				handle: 'tess',
				role: 'Chief of Staff',
				builtinKey: 'custom'
			})
		);
		queueMock.mockResolvedValue({
			rows: [],
			viewer: { employee: null, teamIds: [], owner: true },
			needsMeSources: []
		});

		const nonBuiltIn = await personPanelDataSource.loadMember(
			'org-1',
			'tess-custom',
			new AbortController().signal
		);
		expect(nonBuiltIn.profile?.isBuiltIn).toBe(false);

		profileMock.mockResolvedValue(
			makeProfile('tess', 'Tess', {
				handle: 'tess',
				role: 'Chief of Staff',
				builtinKey: 'chief_of_staff'
			})
		);
		const builtIn = await personPanelDataSource.loadMember(
			'org-1',
			'tess',
			new AbortController().signal
		);
		expect(builtIn.profile?.isBuiltIn).toBe(true);
	});

	it('preserves plain AppError messages instead of collapsing to a generic fallback', async () => {
		profileMock.mockRejectedValue({
			source: 'api',
			code: 'not_found',
			message: 'That member could not be found.'
		});

		const result = await personPanelDataSource.loadMember(
			'org-1',
			'ghost',
			new AbortController().signal
		);
		expect(result.profile).toBeUndefined();
		expect(result.error).toBe('That member could not be found.');
	});

	it('rethrows AbortError when the AbortSignal is cancelled', async () => {
		const controller = new AbortController();
		profileMock.mockImplementation(async () => {
			controller.abort();
			return makeProfile('rowan', 'Rowan');
		});

		await expect(
			personPanelDataSource.loadMember('org-1', 'rowan', controller.signal)
		).rejects.toThrow(/Aborted/);
		expect(queueMock).not.toHaveBeenCalled();
	});
});
