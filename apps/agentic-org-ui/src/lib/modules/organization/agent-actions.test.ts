import { describe, expect, it, vi } from 'vitest';
import type { Member, OrgSnapshot } from '$lib/proto';
import type { OrgSession } from './session';
import {
	assertMutableAgent,
	moveMemberToTeam,
	offboardAgent,
	setAgentStatus,
	updateAgent
} from './agent-actions';

function makeMember(overrides: Partial<Member> = {}): Member {
	return {
		id: 'mem-1',
		kind: 'agent',
		name: 'Noor',
		handle: 'noor',
		home_team_id: 'team-1',
		version: 3,
		owner: false,
		agent: {
			role: 'engineer',
			title: 'Engineer',
			runtime: 'claude_code',
			messaging_name: 'noor',
			status: 'active'
		},
		...overrides
	} as Member;
}

function makeSession(members: Member[]): {
	session: OrgSession;
	command: ReturnType<typeof vi.fn>;
} {
	const command = vi.fn(async () => ({}));
	const snapshot: OrgSnapshot = {
		position: { epoch: 1, seq: 1 },
		teams: [{ id: 'team-1', name: 'Core', version: 1 }],
		members
	};
	return {
		command,
		session: {
			snapshot: vi.fn(async () => snapshot),
			command
		} as unknown as OrgSession
	};
}

describe('organization agent-actions built-in agent immutability', () => {
	const hrByKey = makeMember({
		id: 'mem-hr',
		name: 'HR Lead',
		handle: 'custom-hr',
		builtin_key: 'hr'
	});
	const tessByHandle = makeMember({
		id: 'mem-tess',
		name: 'Tess',
		handle: 'tess-cos-1',
		agent: {
			role: 'chief_of_staff',
			title: 'Chief of Staff',
			runtime: 'claude_code',
			messaging_name: 'tess-cos-1',
			status: 'active'
		}
	});
	const regularAgent = makeMember({ id: 'mem-noor', name: 'Noor', handle: 'noor' });

	it('assertMutableAgent rejects HR (by builtin_key) and Tess (by handle) and permits regular agents', () => {
		expect(() => assertMutableAgent(hrByKey, 'blocked')).toThrow();
		expect(() => assertMutableAgent(tessByHandle, 'blocked')).toThrow();
		expect(() => assertMutableAgent(regularAgent, 'blocked')).not.toThrow();
	});

	it('blocks pause, offboard, home-team move, and name/role/handle edits for built-in agents', async () => {
		const { session, command } = makeSession([hrByKey, tessByHandle, regularAgent]);

		await expect(setAgentStatus(session, 'mem-hr', 'paused')).rejects.toMatchObject({
			code: 'not_authorized'
		});
		await expect(offboardAgent(session, 'mem-tess')).rejects.toMatchObject({
			code: 'not_authorized'
		});
		await expect(moveMemberToTeam(session, 'mem-hr', 'team-1', 'team-2')).rejects.toMatchObject({
			code: 'not_authorized'
		});
		await expect(updateAgent(session, 'mem-hr', { role: 'custom' })).rejects.toMatchObject({
			code: 'not_authorized'
		});
		await expect(updateAgent(session, 'mem-tess', { name: 'Other' })).rejects.toMatchObject({
			code: 'not_authorized'
		});
		await expect(
			updateAgent(session, 'mem-tess', { messagingName: 'renamed' })
		).rejects.toMatchObject({
			code: 'not_authorized'
		});
		expect(command).not.toHaveBeenCalled();

		// Non-identity profile updates (e.g. model/focus) remain allowed for built-in agents.
		await updateAgent(session, 'mem-tess', { model: 'claude-sonnet-4-6' });
		expect(command).toHaveBeenCalledTimes(1);
	});
});
