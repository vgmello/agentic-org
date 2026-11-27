import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { MemberProfile } from '$lib/modules/members/types';
import type { TaskRow } from '$lib/modules/workitems/types';

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

import { personPanelDataSource } from './person-panel-source';

function makeProfile(id: string, name: string): MemberProfile {
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
		viewer: { isSelf: false, isOwnerOrAdmin: false, costVisible: false }
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

describe('personPanelDataSource', () => {
	beforeEach(() => {
		profileMock.mockReset();
		queueMock.mockReset();
	});

	it('forwards AbortSignal, filters open assignee rows, and caps open work items at 3', async () => {
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

		expect(profileMock).toHaveBeenCalledWith('org-1', 'rowan');
		expect(queueMock).toHaveBeenCalledWith('org-1', { kind: 'organization' }, controller.signal);
		expect(result.error).toBeUndefined();
		expect(result.profile?.name).toBe('Rowan');
		expect(result.profile?.presenceLabel).toBe('Working');
		expect(result.profile?.profileHref).toBe('/members/rowan');
		expect(result.profile?.workTabHref).toBe('/members/rowan/work');
		expect(result.totalOpenWork).toBe(4);
		expect(result.openWork.map((item) => item.ref)).toEqual(['ORG-1', 'ORG-2', 'ORG-3']);
	});

	it('surfaces profile errors cleanly when profile fetch fails', async () => {
		profileMock.mockRejectedValue(new Error('Member offline'));
		queueMock.mockResolvedValue({
			rows: [],
			viewer: { employee: null, teamIds: [], owner: true },
			needsMeSources: []
		});

		const result = await personPanelDataSource.loadMember(
			'org-1',
			'rowan',
			new AbortController().signal
		);
		expect(result.profile).toBeUndefined();
		expect(result.error).toBe('Member offline');
	});
});
