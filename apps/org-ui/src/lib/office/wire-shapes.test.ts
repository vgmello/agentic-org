import { describe, expect, it } from 'vitest';
import wire from '../../../../../crates/org-proto/test-vectors/office-wire-shapes.json';
import { API_ERROR_CODES } from '$lib/proto';
import type { AgentProfile, CommandAccepted, Member, Membership, OfficeInfo, Organization, OrgSnapshot, Position, ProjectSummary, ProjectTeam, Team } from '$lib/proto';

// Require every mirror key (including optional ones) so a TS-side rename also breaks this gate.
const exact = <T>() => <const K extends readonly (keyof T)[]>(keys: K & (Exclude<keyof T, K[number]> extends never ? unknown : never)) => keys;
const keys = {
	accepted: exact<CommandAccepted>()(['status', 'position']),
	office: exact<OfficeInfo>()(['id', 'organization_id', 'name', 'role', 'host', 'endpoint', 'status', 'status_reason', 'next_restart_at']),
	organization: exact<Organization>()(['id', 'name', 'created_at', 'offices']),
	snapshot: exact<OrgSnapshot>()(['position', 'teams', 'members', 'memberships', 'projects', 'project_teams', 'areas']),
	member: exact<Member>()(['id', 'kind', 'name', 'contact', 'owner', 'office_id', 'reports_to_member_id', 'agent', 'created_at', 'version']),
	profile: exact<AgentProfile>()(['role', 'title', 'focus', 'runtime', 'model', 'run_mode', 'schedule', 'home', 'messaging_name', 'status', 'offboarded_at']),
	team: exact<Team>()(['id', 'name', 'owner_team_id', 'lead_member_id', 'version']),
	membership: exact<Membership>()(['id', 'team_id', 'member_id', 'title_in_team', 'version']),
	project: exact<ProjectSummary>()(['id', 'key', 'name', 'description', 'repo', 'integration_branch', 'archived', 'created_at', 'version']),
	projectTeam: exact<ProjectTeam>()(['id', 'project_id', 'team_id', 'version']),
	position: exact<Position>()(['epoch', 'seq'])
};
function expectKeys<K extends string>(value: object | undefined, all: readonly K[], omitted: readonly K[] = []) {
	expect(value).toBeDefined();
	expect(Object.keys(value ?? {}).sort()).toEqual(all.filter((key) => !omitted.includes(key)).sort());
}

// No Tauri wrapper or app-local fixture: this side of the serialization gate survives F1-03.
describe('canonical office wire payloads shared with Rust', () => {
	it('keeps API errors and command outcomes independent of desktop wrappers', () => {
		expect(Object.keys(wire.api_error).sort()).toEqual(['code', 'message']);
		expect(API_ERROR_CODES).toContain(wire.api_error.code);
		expectKeys(wire.command_accepted, keys.accepted);
		expect(wire.command_accepted.status).toBe('committed');
		expect(wire.command_accepted.position).toEqual({ epoch: 1, seq: 2 });
	});

	it('keeps office and organization payloads without a desktop connection pair', () => {
		expectKeys(wire.office, keys.office, ['endpoint', 'status_reason', 'next_restart_at']);
		expectKeys(wire.organization, keys.organization);
		expect(wire.organization.offices).toEqual([wire.office]);
	});

	it('keeps the snapshot, placement and profile contract without the sample flag', () => {
		expectKeys(wire.snapshot, keys.snapshot);
		const agent = wire.snapshot.members.find((member) => member.kind === 'agent');
		expectKeys(agent, keys.member, ['contact', 'owner', 'created_at']);
		expectKeys(agent?.agent, keys.profile, ['focus', 'model', 'schedule', 'home', 'offboarded_at']);
		expectKeys(wire.snapshot.teams[0], keys.team, ['owner_team_id']);
		expectKeys(wire.snapshot.memberships[0], keys.membership);
		expectKeys(wire.snapshot.projects[0], keys.project, ['description', 'repo', 'integration_branch', 'archived', 'created_at']);
		expectKeys(wire.snapshot.project_teams[0], keys.projectTeam);
		expectKeys(wire.snapshot.position, keys.position);
		expect(agent).toMatchObject({ office_id: 'office-1', reports_to_member_id: 'you' });
		expect(agent?.agent).toMatchObject({ runtime: 'claude_code', status: 'active', run_mode: 'on_call' });
		expect(wire.snapshot.members.find((member) => member.owner)).not.toHaveProperty('office_id');
	});
});
