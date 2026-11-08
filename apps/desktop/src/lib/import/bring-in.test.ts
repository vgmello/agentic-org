import { describe, expect, it } from 'vitest';
import { createMockOrg, type MockStorage } from '$lib/org/api';
import type { OrgSession } from '$lib/org/session';
import { sessionFor } from '$lib/org/session';
import { bringIn } from './bring-in';
import { applyHints } from './hints';
import { SAMPLE_HINTS, SAMPLE_SCAN } from './mock';
import { buildRows, chooseRole } from './rows';
import { suggestProject, suggestTeams } from './teams';

const memory = (): MockStorage => {
	const m = new Map<string, string>();
	return { getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v) };
};
const make = () => sessionFor(createMockOrg(async () => 'office-1', { storage: memory(), empty: () => true, delayMs: () => 0 }), 'o');

/** The review as the owner leaves it for our own team: everyone filled in, the hints file chosen. */
function reviewed() {
	const { rows } = buildRows(SAMPLE_SCAN, { exclude: ['sample'] });
	const filled = rows.map((r) => {
		if (r.runtime) return r;
		const withRole = chooseRole(r, 'software engineer');
		return { ...withRole, runtime: r.name === 'server-dev' || r.name === 'codex-dev' ? ('codex' as const) : r.name === 'agy-senior-dev' ? ('antigravity' as const) : ('claude_code' as const) };
	});
	const fitted = applyHints(filled, SAMPLE_HINTS, suggestTeams(filled));
	return { rows: fitted.rows, drafts: fitted.drafts, project: suggestProject(fitted.rows, fitted.drafts) };
}

const input = (r = reviewed()) => ({ ...r, officeId: 'office-1' });
const agentsOf = async (s: OrgSession) => (await s.snapshot()).members.filter((m) => m.kind === 'agent');

describe('IMP-7 bringing the reviewed team in', () => {
	it('creates the agents, the teams under their owners, the leads, the reporting lines and the project', async () => {
		const s = make();
		const said: string[] = [];
		const result = await bringIn(s, { ...input(), onProgress: (t) => said.push(t) });
		expect(result).toMatchObject({ agents: 10, teams: 6, projects: 1, paused: 0, failures: [] });
		expect(said).toEqual(['Creating teams…', 'Adding agents…', 'Putting agents in their teams…', 'Linking leads and reporting lines…', 'Setting up the project…']);

		const snap = await s.snapshot();
		const team = (name: string) => snap.teams.find((t) => t.name === name)!;
		const member = (handle: string) => snap.members.find((m) => m.agent?.messaging_name === handle)!;
		expect(team('App').owner_team_id).toBe(team('Engineering').id);
		expect(team('Design').owner_team_id).toBeUndefined();
		expect(team('App').lead_member_id).toBe(member('app-frontend-dev').id);
		expect(team('Engineering').lead_member_id).toBe(member('ac-staff-engineer-ada').id);
		const inTeam = (t: string) => snap.memberships.filter((m) => m.team_id === team(t).id).map((m) => snap.members.find((x) => x.id === m.member_id)!.name).sort();
		expect(inTeam('Office')).toEqual(['River', 'server-dev']);
		expect(inTeam('Agent tooling')).toEqual(['Remy', 'agy-senior-dev', 'codex-dev']);
		// HR's reporting lines: River and Remy and Riley to ada; ada and Eden to the owner; messaging-only agents to the owner.
		expect(member('ac-software-engineer-river').reports_to_member_id).toBe(member('ac-staff-engineer-ada').id);
		expect(member('ac-staff-engineer-ada').reports_to_member_id).toBeUndefined();
		expect(member('server-dev').reports_to_member_id).toBeUndefined();
		expect(member('codex-dev').agent).toMatchObject({ runtime: 'codex', status: 'active' });
		expect(member('ac-staff-engineer-ada').agent).toMatchObject({ runtime: 'claude_code', title: 'Staff Engineer', role: 'staff engineer' });
		expect(snap.projects?.map((p) => p.key)).toEqual(['ac']);
		expect(snap.project_teams?.map((l) => snap.teams.find((t) => t.id === l.team_id)!.name).sort()).toEqual(['Design', 'Engineering']);
	});

	it('brings an agent in paused when the owner chose so', async () => {
		const r = reviewed();
		r.rows.find((x) => x.name === 'Riley')!.choice = 'paused';
		const s = make();
		const result = await bringIn(s, input(r));
		expect(result.paused).toBe(1);
		expect((await agentsOf(s)).find((m) => m.name === 'Riley')?.agent?.status).toBe('paused');
	});

	it('leaves a skipped agent out, and a team only they would have filled', async () => {
		const r = reviewed();
		r.rows.find((x) => x.name === 'reviewer')!.choice = 'skip';
		const s = make();
		const result = await bringIn(s, input(r));
		expect(result).toMatchObject({ agents: 9, teams: 5 });
		expect((await s.snapshot()).teams.map((t) => t.name)).not.toContain('Quality');
	});
});

describe('IMP-8 running it again', () => {
	it('finds nothing new: no duplicate agent, team, membership or project', async () => {
		const s = make();
		await bringIn(s, input());
		const before = await s.snapshot();
		const again = await bringIn(s, input());
		expect(again).toMatchObject({ agents: 0, teams: 0, projects: 0, paused: 0, failures: [] });
		const after = await s.snapshot();
		expect(after.members).toHaveLength(before.members.length);
		expect(after.teams).toHaveLength(before.teams.length);
		expect(after.memberships).toHaveLength(before.memberships.length);
		expect(after.projects).toHaveLength(before.projects?.length ?? 0);
	});

	it('adds only the agent that is new', async () => {
		const s = make();
		const first = reviewed();
		first.rows.find((x) => x.name === 'reviewer')!.choice = 'skip';
		await bringIn(s, input(first));
		const second = await bringIn(s, input());
		expect(second).toMatchObject({ agents: 1, teams: 1, failures: [] });
		expect((await agentsOf(s)).map((m) => m.name)).toContain('reviewer');
	});
});

describe('IMP-9 an interrupted import continues where it stopped', () => {
	it('reports the agent that failed, brings the others in, and Try again adds only what is missing', async () => {
		const real = make();
		let fail = true;
		const flaky: OrgSession = {
			snapshot: () => real.snapshot(),
			command: (c) => {
				if (fail && c.method === 'member.add_agent' && c.params.name === 'codex-dev') return Promise.reject({ source: 'api', code: 'unavailable', message: 'The office went away.' });
				return real.command(c);
			}
		};
		const first = await bringIn(flaky, input());
		expect(first.agents).toBe(9);
		expect(first.failures).toHaveLength(1);
		expect(first.failures[0]).toMatchObject({ what: 'Add codex-dev', error: { code: 'unavailable' } });
		expect((await agentsOf(real)).map((m) => m.name)).not.toContain('codex-dev');

		fail = false;
		const retry = await bringIn(flaky, input());
		expect(retry).toMatchObject({ agents: 1, teams: 0, failures: [] });
		const snap = await real.snapshot();
		expect(snap.members.filter((m) => m.kind === 'agent')).toHaveLength(10);
		const tooling = snap.teams.find((t) => t.name === 'Agent tooling')!;
		expect(snap.memberships.filter((m) => m.team_id === tooling.id)).toHaveLength(3);
	});

	it('does not make a team whose owner could not be made', async () => {
		const real = make();
		const noEngineering: OrgSession = {
			snapshot: () => real.snapshot(),
			command: (c) => (c.method === 'team.create' && c.params.name === 'Engineering' ? Promise.reject({ source: 'api', code: 'unavailable', message: 'no' }) : real.command(c))
		};
		const result = await bringIn(noEngineering, input());
		expect(result.failures.map((f) => f.what)).toEqual(['Create Engineering']);
		expect((await real.snapshot()).teams.map((t) => t.name)).toEqual(['Design']);
	});
});
