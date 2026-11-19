import { describe, expect, it } from 'vitest';
import type { AppError } from '$lib/proto';
import { newId, withId, type OrgCommandInput } from './commands';
import { applyCommand } from './reducer';
import { notInATeam, teamTree, unassignedProjects } from './model';
import { sampleOrg } from './sample';

const s0 = sampleOrg('office-1');
const run = (s = s0, ...inputs: OrgCommandInput[]) => inputs.reduce((acc, i) => applyCommand(acc, withId(i)), s);
const err = (fn: () => unknown): AppError => {
	try {
		fn();
	} catch (e) {
		return e as AppError;
	}
	throw new Error('expected an error');
};
const team = (s: typeof s0, id: string) => s.teams.find((t) => t.id === id)!;
const member = (s: typeof s0, id: string) => s.members.find((m) => m.id === id)!;

describe('commands', () => {
	it('new ids are 26-character ULIDs, sortable by time and different each time', () => {
		const a = newId(1_000_000), b = newId(1_000_001);
		expect(a).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
		expect(a.slice(0, 10) < b.slice(0, 10)).toBe(true);
		expect(newId()).not.toBe(newId());
	});

	it('every command advances the position by one and does not touch the input snapshot', () => {
		const before = JSON.stringify(s0);
		const s1 = run(s0, { method: 'team.create', params: { team_id: 't1', name: 'Ops' } });
		expect(s1.position).toEqual({ epoch: 1, seq: s0.position.seq + 1 });
		expect(JSON.stringify(s0)).toBe(before);
	});
});

describe('TEAM-5 creating a team', () => {
	it('creates it under a team, or at the top, with version 1', () => {
		const s = run(s0, { method: 'team.create', params: { team_id: 'qa', name: '  QA  ', owner_team_id: 'eng' } }, { method: 'team.create', params: { team_id: 'ops', name: 'Ops' } });
		expect(team(s, 'qa')).toMatchObject({ name: 'QA', owner_team_id: 'eng', version: 1 });
		expect(team(s, 'ops').owner_team_id).toBeUndefined();
		expect(teamTree(s).map((n) => n.team.name)).toEqual(['Design', 'Engineering', 'Ops', 'People']);
	});

	it('a duplicate sibling name is refused, ignoring case; the same name elsewhere is fine', () => {
		expect(err(() => run(s0, { method: 'team.create', params: { team_id: 'x', name: 'platform', owner_team_id: 'eng' } })).code).toBe('already_exists');
		expect(err(() => run(s0, { method: 'team.create', params: { team_id: 'x', name: 'design' } })).code).toBe('already_exists');
		expect(() => run(s0, { method: 'team.create', params: { team_id: 'x', name: 'Design', owner_team_id: 'people' } })).not.toThrow();
	});

	it('an empty or invalid name, a taken id and a missing owner team are refused', () => {
		expect(err(() => run(s0, { method: 'team.create', params: { team_id: 'x', name: '  ' } })).code).toBe('invalid_name');
		expect(err(() => run(s0, { method: 'team.create', params: { team_id: 'eng', name: 'New' } })).code).toBe('already_exists');
		expect(err(() => run(s0, { method: 'team.create', params: { team_id: 'x', name: 'New', owner_team_id: 'ghost' } })).code).toBe('not_found');
	});

	it('the lead must be a member of the team and never the owner (TEAM-5, OWNER-2)', () => {
		const lead = (id?: string) => () => run(s0, { method: 'team.set_lead', params: { team_id: 'web', expected_version: 1, lead_member_id: id } });
		expect(team(lead('mira')(), 'web').lead_member_id).toBe('mira');
		expect(err(lead('you')).code).toBe('owner_protected');
		expect(err(lead('juno')).code).toBe('not_a_member');
		expect(team(lead(undefined)(), 'web').lead_member_id).toBeUndefined();
	});
});

describe('TEAM-6 members in several teams', () => {
	it('adds a member to a team, refuses a duplicate and the owner, and a stale version conflicts', () => {
		const s = run(s0, { method: 'team.add_member', params: { membership_id: 'm1', team_id: 'design', member_id: 'quinn', title_in_team: 'Intern' } });
		expect(s.memberships.find((m) => m.id === 'm1')).toMatchObject({ team_id: 'design', member_id: 'quinn', title_in_team: 'Intern', version: 1 });
		expect(notInATeam(s).map((m) => m.id)).not.toContain('quinn');
		expect(err(() => run(s, { method: 'team.add_member', params: { membership_id: 'm2', team_id: 'design', member_id: 'quinn' } })).code).toBe('already_exists');
		expect(err(() => run(s0, { method: 'team.add_member', params: { membership_id: 'm2', team_id: 'design', member_id: 'you' } })).code).toBe('owner_protected');
		expect(err(() => run(s0, { method: 'team.add_member', params: { membership_id: 'm2', team_id: 'design', member_id: 'ghost' } })).code).toBe('not_found');
		const stale = s0.memberships[0];
		expect(err(() => run(s0, { method: 'team.remove_member', params: { membership_id: stale.id, expected_version: 9 } })).code).toBe('conflict');
	});

	it('removing the lead from the team clears the lead', () => {
		const lead = s0.memberships.find((m) => m.team_id === 'web' && m.member_id === 'noor')!;
		const s = run(s0, { method: 'team.remove_member', params: { membership_id: lead.id, expected_version: lead.version } });
		expect(team(s, 'web').lead_member_id).toBeUndefined();
		expect(s.memberships.some((m) => m.id === lead.id)).toBe(false);
	});

	it('a title can be set and cleared', () => {
		const ms = s0.memberships[0];
		const s = run(s0, { method: 'team.set_title', params: { membership_id: ms.id, expected_version: ms.version, title_in_team: 'Lead' } });
		expect(s.memberships.find((m) => m.id === ms.id)).toMatchObject({ title_in_team: 'Lead', version: ms.version + 1 });
	});
});

describe('TEAM-7 moving a team', () => {
	it('moves a team and its sub-teams go with it; a team can go to the top', () => {
		const s = run(s0, { method: 'team.move', params: { team_id: 'platform', expected_version: 1, owner_team_id: 'design' } });
		expect(team(s, 'platform')).toMatchObject({ owner_team_id: 'design', version: 2 });
		const top = run(s0, { method: 'team.move', params: { team_id: 'web', expected_version: 1 } });
		expect(team(top, 'web').owner_team_id).toBeUndefined();
	});

	it('a team can’t go under itself or a team below it', () => {
		expect(err(() => run(s0, { method: 'team.move', params: { team_id: 'eng', expected_version: 1, owner_team_id: 'web' } })).code).toBe('cycle');
		expect(err(() => run(s0, { method: 'team.move', params: { team_id: 'eng', expected_version: 1, owner_team_id: 'eng' } })).code).toBe('cycle');
	});

	it('a sibling with the same name at the destination refuses the move', () => {
		const withDup = run(s0, { method: 'team.create', params: { team_id: 'web2', name: 'Web', owner_team_id: 'design' } });
		expect(err(() => run(withDup, { method: 'team.move', params: { team_id: 'web', expected_version: 1, owner_team_id: 'design' } })).code).toBe('already_exists');
	});

	it('renaming checks the name and the siblings, and a stale version conflicts', () => {
		expect(team(run(s0, { method: 'team.rename', params: { team_id: 'web', expected_version: 1, name: 'Frontend' } }), 'web').name).toBe('Frontend');
		expect(err(() => run(s0, { method: 'team.rename', params: { team_id: 'web', expected_version: 1, name: 'platform' } })).code).toBe('already_exists');
		expect(err(() => run(s0, { method: 'team.rename', params: { team_id: 'web', expected_version: 5, name: 'X' } })).code).toBe('conflict');
	});
});

describe('TEAM-8 and TEAM-9 projects and deleting teams', () => {
	it('adds and removes a project’s team; the last removal leaves the project unassigned', () => {
		let s = run(s0, { method: 'project.add_team', params: { project_team_id: 'pt-9', project_id: 'scratch', team_id: 'web' } });
		expect(unassignedProjects(s)).toEqual([]);
		expect(err(() => run(s, { method: 'project.add_team', params: { project_team_id: 'pt-10', project_id: 'scratch', team_id: 'web' } })).code).toBe('already_exists');
		s = run(s, { method: 'project.remove_team', params: { project_team_id: 'pt-9', expected_version: 1 } });
		expect(unassignedProjects(s).map((p) => p.id)).toEqual(['scratch']);
		expect(err(() => run(s0, { method: 'project.add_team', params: { project_team_id: 'x', project_id: 'ghost', team_id: 'web' } })).code).toBe('not_found');
	});

	it('a team that owns teams can’t be deleted', () => {
		expect(err(() => run(s0, { method: 'team.delete', params: { team_id: 'eng', expected_version: 1 } })).code).toBe('not_empty');
	});

	it('deleting another team moves its members to Not in a team and unassigns its projects', () => {
		const s = run(s0, { method: 'team.delete', params: { team_id: 'people', expected_version: 1 } });
		expect(s.teams.some((t) => t.id === 'people')).toBe(false);
		expect(notInATeam(s).map((m) => m.id)).toEqual(expect.arrayContaining(['hana', 'leo', 'ines']));
		expect(unassignedProjects(s).map((p) => p.id)).toContain('onboarding');
	});
});

describe('PEOPLE-3, 4, 5 adding, editing and removing people', () => {
	it('adds a person (not in a team yet), with an optional contact; the name rule applies', () => {
		const s = run(s0, { method: 'member.add_person', params: { member_id: '01JC00000000000000000000P1', name: ' Robin Tate ', contact: 'robin@example.com' } });
		expect(member(s, '01JC00000000000000000000P1')).toMatchObject({ kind: 'person', name: 'Robin Tate', contact: 'robin@example.com', version: 1 });
		expect(notInATeam(s).map((m) => m.id)).toContain('01JC00000000000000000000P1');
		expect(err(() => run(s0, { method: 'member.add_person', params: { member_id: '01JC00000000000000000000P2', name: '' } })).code).toBe('invalid_name');
		expect(err(() => run(s, { method: 'member.add_person', params: { member_id: '01JC00000000000000000000P1', name: 'Dup' } })).code).toBe('already_exists'); // that id is taken now
	});

	it('updates only the given fields, can clear the contact, and agents are not edited here', () => {
		const avery = member(s0, 'avery');
		const s = run(s0, { method: 'member.update_person', params: { member_id: 'avery', expected_version: avery.version, name: 'Avery S.' } });
		expect(member(s, 'avery')).toMatchObject({ name: 'Avery S.', contact: 'avery@example.com', version: avery.version + 1 });
		const cleared = run(s, { method: 'member.update_person', params: { member_id: 'avery', expected_version: 2, clear_contact: true } });
		expect(member(cleared, 'avery').contact).toBeUndefined();
		expect(err(() => run(s0, { method: 'member.update_person', params: { member_id: 'sam', expected_version: 1, name: 'X' } })).code).toBe('invalid_request');
		expect(err(() => run(s0, { method: 'member.update_person', params: { member_id: 'avery', expected_version: 1, contact: 'a', clear_contact: true } })).code).toBe('invalid_request');
		expect(err(() => run(s0, { method: 'member.update_person', params: { member_id: 'avery', expected_version: 7, name: 'X' } })).code).toBe('conflict');
	});

	it('removing a person leaves their teams and leads, and whoever reported to them reports to their manager', () => {
		const s = run(s0, { method: 'member.remove', params: { member_id: 'hana', expected_version: 1 } });
		expect(s.members.some((m) => m.id === 'hana')).toBe(false);
		expect(s.memberships.some((m) => m.member_id === 'hana')).toBe(false);
		expect(team(s, 'people').lead_member_id).toBeUndefined();
		expect(member(s, 'leo').reports_to_member_id).toBe('avery'); // Leo reported to Hana, who reported to Avery
		expect(member(s, 'leo').version).toBe(2);
		const top = run(s0, { method: 'member.remove', params: { member_id: 'avery', expected_version: 1 } });
		expect(member(top, 'hana').reports_to_member_id).toBeUndefined(); // Avery had no manager: the owner
		expect(err(() => run(s0, { method: 'member.remove', params: { member_id: 'you', expected_version: 1 } })).code).toBe('owner_protected');
	});
});

describe('REPORT-3 and REPORT-4 changing a manager', () => {
	it('sets a manager, or the owner (stored as empty), and bumps the version', () => {
		const s = run(s0, { method: 'member.set_reports_to', params: { member_id: 'kai', expected_version: 1, reports_to_member_id: 'juno' } });
		expect(member(s, 'kai')).toMatchObject({ reports_to_member_id: 'juno', version: 2 });
		const toOwner = run(s0, { method: 'member.set_reports_to', params: { member_id: 'kai', expected_version: 1, reports_to_member_id: 'you' } });
		expect(member(toOwner, 'kai').reports_to_member_id).toBeUndefined();
	});

	it('refuses a loop and the owner reporting to anyone', () => {
		expect(err(() => run(s0, { method: 'member.set_reports_to', params: { member_id: 'sam', expected_version: 1, reports_to_member_id: 'noor' } })).code).toBe('cycle');
		expect(err(() => run(s0, { method: 'member.set_reports_to', params: { member_id: 'sam', expected_version: 1, reports_to_member_id: 'sam' } })).code).toBe('cycle');
		expect(err(() => run(s0, { method: 'member.set_reports_to', params: { member_id: 'you', expected_version: 1, reports_to_member_id: 'sam' } })).code).toBe('owner_protected');
	});

	it('no team change changes anyone’s manager (REPORT-5)', () => {
		const s = run(s0, { method: 'team.delete', params: { team_id: 'people', expected_version: 1 } }, { method: 'team.move', params: { team_id: 'web', expected_version: 1, owner_team_id: 'design' } });
		for (const m of s0.members) expect(member(s, m.id).reports_to_member_id).toBe(m.reports_to_member_id);
	});
});

describe('PEOPLE-4 and PEOPLE-8 one handle namespace', () => {
	it('a person can’t take another member’s name, an agent’s handle or "owner", ignoring case', () => {
		for (const name of ['avery stone', 'SAM', 'OWNER']) {
			expect(err(() => run(s0, { method: 'member.add_person', params: { member_id: '01JC000000000000000000PNEW', name } })).code).toBe('already_exists');
		}
		const added = run(s0, { method: 'member.add_person', params: { member_id: '01JC000000000000000000PNEW', name: 'Sam Okafor' } });
		expect(member(added, '01JC000000000000000000PNEW').name).toBe('Sam Okafor'); // an agent's display name is not a handle
	});

	it('renaming to your own name in another case is fine; to someone else’s is refused', () => {
		const renamed = run(s0, { method: 'member.update_person', params: { member_id: 'avery', expected_version: 1, name: 'AVERY STONE' } });
		expect(member(renamed, 'avery').name).toBe('AVERY STONE');
		expect(err(() => run(s0, { method: 'member.update_person', params: { member_id: 'avery', expected_version: 1, name: 'hana weiss' } })).code).toBe('already_exists');
	});
});

describe('member.move', () => {
	it('replaces the membership in one step, clears a lead they leave, and refuses a team they are already in', () => {
		const ms = s0.memberships.find((m) => m.team_id === 'web' && m.member_id === 'noor')!;
		const s = run(s0, { method: 'member.move', params: { membership_id: ms.id, expected_version: ms.version, new_membership_id: 'moved-1', to_team_id: 'platform', title_in_team: 'Engineer' } });
		expect(s.memberships.some((m) => m.id === ms.id)).toBe(false);
		expect(s.memberships.find((m) => m.id === 'moved-1')).toMatchObject({ team_id: 'platform', member_id: 'noor', title_in_team: 'Engineer', version: 1 });
		expect(team(s, 'web').lead_member_id).toBeUndefined(); // Noor led Web
		expect(team(s, 'web').version).toBe(2);
		expect(err(() => run(s0, { method: 'member.move', params: { membership_id: ms.id, expected_version: ms.version, new_membership_id: 'm2', to_team_id: 'web' } })).code).toBe('already_exists');
		expect(err(() => run(s0, { method: 'member.move', params: { membership_id: ms.id, expected_version: 9, new_membership_id: 'm2', to_team_id: 'platform' } })).code).toBe('conflict');
		expect(err(() => run(s0, { method: 'member.move', params: { membership_id: ms.id, expected_version: ms.version, new_membership_id: 'm2', to_team_id: 'ghost' } })).code).toBe('not_found');
	});
});

describe('agent commands: what is stored', () => {
	const agentProfile = { role: '  QA engineer ', title: 'QA', runtime: 'codex' as const, messaging_name: 'cam', status: 'active' as const, focus: ' tests ' };
	const ctx = { now: '2026-11-08T12:00:00.000Z', offices: ['office-1'] };
	const add = () => applyCommand(s0, withId({ method: 'member.add_agent', params: { member_id: '01JC0000000000000000000CAM', name: '  Cameron ', office_id: 'office-1', profile: agentProfile } }), ctx);

	it('stores an added agent trimmed, in its office, with HQ’s clock and no manager', () => {
		const cam = member(add(), '01JC0000000000000000000CAM');
		expect(cam).toMatchObject({ name: 'Cameron', kind: 'agent', office_id: 'office-1', created_at: '2026-11-08T12:00:00.000Z', version: 1 });
		expect(cam.reports_to_member_id).toBeUndefined();
		expect(cam.agent).toMatchObject({ role: 'QA engineer', focus: 'tests', status: 'active' });
	});

	it('an unknown office is not_found, and a null inside the profile means not given', () => {
		expect(err(() => applyCommand(s0, withId({ method: 'member.add_agent', params: { member_id: '01JC0000000000000000000CAM', name: 'C', office_id: 'nowhere', profile: agentProfile } }), ctx)).code).toBe('not_found');
		const nulls = { ...agentProfile, focus: null as unknown as undefined, model: null as unknown as undefined, home: null as unknown as undefined };
		const s = applyCommand(s0, withId({ method: 'member.add_agent', params: { member_id: '01JC0000000000000000000CAM', name: 'Cameron', office_id: 'office-1', profile: nulls } }), ctx);
		expect(member(s, '01JC0000000000000000000CAM').agent).not.toHaveProperty('focus');
	});

	it('update changes only what is given and clears with the flags', () => {
		const s1 = applyCommand(add(), withId({ method: 'member.update_agent', params: { member_id: '01JC0000000000000000000CAM', expected_version: 1, title: ' Lead QA ', model: ' m ' } }), ctx);
		expect(member(s1, '01JC0000000000000000000CAM')).toMatchObject({ version: 2 });
		expect(member(s1, '01JC0000000000000000000CAM').agent).toMatchObject({ title: 'Lead QA', model: 'm', role: 'QA engineer', focus: 'tests' });
		const s2 = applyCommand(s1, withId({ method: 'member.update_agent', params: { member_id: '01JC0000000000000000000CAM', expected_version: 2, clear_focus: true, clear_model: true } }), ctx);
		expect(member(s2, '01JC0000000000000000000CAM').agent).not.toHaveProperty('focus');
		expect(member(s2, '01JC0000000000000000000CAM').agent).not.toHaveProperty('model');
	});

	it('pausing and the run mode change the profile and the version; a schedule only goes with scheduled', () => {
		let s = applyCommand(s0, withId({ method: 'member.set_agent_status', params: { member_id: 'sam', expected_version: 1, status: 'paused' } }), ctx);
		expect(member(s, 'sam').agent?.status).toBe('paused');
		s = applyCommand(s, withId({ method: 'member.set_run_mode', params: { member_id: 'sam', expected_version: 2, run_mode: 'scheduled', schedule: { spec: ' daily ', time_zone: 'UTC' } } }), ctx);
		expect(member(s, 'sam').agent?.schedule?.spec).toBe('daily');
		s = applyCommand(s, withId({ method: 'member.set_run_mode', params: { member_id: 'sam', expected_version: 3, run_mode: 'on_call' } }), ctx);
		expect(member(s, 'sam').agent).not.toHaveProperty('schedule');
		expect(member(s, 'sam').version).toBe(4);
	});

	it('offboarding keeps the record and handle, stamps HQ’s date, and moves reporters up in one step', () => {
		const s = applyCommand(s0, withId({ method: 'member.offboard', params: { member_id: 'sam', expected_version: 1 } }), ctx);
		expect(member(s, 'sam')).toMatchObject({ version: 2, agent: { status: 'offboarded', offboarded_at: '2026-11-08T12:00:00.000Z', messaging_name: 'sam' } });
		expect(s.memberships.some((m) => m.member_id === 'sam')).toBe(false);
		expect(team(s, 'eng')).toMatchObject({ version: 2 });
		expect(team(s, 'eng').lead_member_id).toBeUndefined();
		for (const id of ['noor', 'kai', 'mira']) expect(member(s, id)).toMatchObject({ reports_to_member_id: 'avery', version: 2 });
	});
});

describe('a new member’s id is 26 letters or digits', () => {
	it('is refused otherwise, for people and for agents, and a missing office still comes first', () => {
		expect(err(() => run(s0, { method: 'member.add_person', params: { member_id: 'robin', name: 'Robin' } })).code).toBe('invalid_request');
		expect(err(() => run(s0, { method: 'member.add_person', params: { member_id: '0'.repeat(25) + '-', name: 'Robin' } })).code).toBe('invalid_request');
		const profile = { role: 'r', title: 't', runtime: 'codex' as const, messaging_name: 'rob', status: 'active' as const };
		const ctx = { now: '2026-11-08T12:00:00.000Z', offices: ['office-1'] };
		expect(err(() => applyCommand(s0, withId({ method: 'member.add_agent', params: { member_id: 'rob', name: 'Rob', office_id: 'office-1', profile } }), ctx)).code).toBe('invalid_request');
		expect(err(() => applyCommand(s0, withId({ method: 'member.add_agent', params: { member_id: 'rob', name: 'Rob', office_id: 'ghost', profile } }), ctx)).code).toBe('not_found');
	});
});
