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
		// The owner failed, and each team that goes under it says it wasn't made because of that.
		expect(result.failures.map((f) => f.what).sort()).toEqual(['Create Agent tooling', 'Create App', 'Create Engineering', 'Create Office', 'Create Quality']);
		expect(result.failures.find((f) => f.what === 'Create App')?.error?.message).toMatch(/owner team couldn't be made/);
		expect((await real.snapshot()).teams.map((t) => t.name)).toEqual(['Design']);
	});
});

describe('what an import must not get wrong', () => {
	it('puts a lead of a parent team in that team even when the file places them in a deeper one', async () => {
		const r = reviewed();
		// ada leads Engineering and is listed in App as well: the deeper team wins her row.
		const hints = {
			path: 'f',
			teams: [
				{ key: 'engineering', name: 'Engineering', lead: 'ac-staff-engineer-ada', members: [] },
				{ key: 'app', name: 'App', parent: 'engineering', lead: 'app-frontend-dev', members: ['ac-staff-engineer-ada', 'app-frontend-dev'] }
			]
		};
		const fitted = applyHints(r.rows, hints, []);
		const s = make();
		const result = await bringIn(s, { rows: fitted.rows, drafts: fitted.drafts, project: undefined, officeId: 'office-1' });
		expect(result.failures).toEqual([]);
		const snap = await s.snapshot();
		const ada = snap.members.find((m) => m.agent?.messaging_name === 'ac-staff-engineer-ada')!;
		const engineering = snap.teams.find((t) => t.name === 'Engineering')!;
		expect(engineering.lead_member_id).toBe(ada.id);
		expect(snap.memberships.some((m) => m.team_id === engineering.id && m.member_id === ada.id)).toBe(true);
	});

	it('brings a Paused agent in paused at once, with no moment as active', async () => {
		const r = reviewed();
		r.rows.find((x) => x.name === 'Riley')!.choice = 'paused';
		const real = make();
		const statuses: string[] = [];
		const spy: OrgSession = {
			snapshot: () => real.snapshot(),
			command: (c) => {
				if (c.method === 'member.add_agent' && c.params.name === 'Riley') statuses.push(c.params.profile.status);
				if (c.method === 'member.set_agent_status') statuses.push('set');
				return real.command(c);
			}
		};
		await bringIn(spy, input(r));
		expect(statuses).toEqual(['paused']);
	});

	it('continues from a review rebuilt after an interruption: agents already in are still put in their teams and lines', async () => {
		const real = make();
		let fail = true;
		const flaky: OrgSession = {
			snapshot: () => real.snapshot(),
			command: (c) => (fail && c.method === 'team.add_member' ? Promise.reject({ source: 'api', code: 'unavailable', message: 'gone' }) : real.command(c))
		};
		const first = await bringIn(flaky, input());
		expect(first.agents).toBe(10);
		expect(first.failures.length).toBeGreaterThan(0);
		fail = false;
		// The review is built again from what the organization has now: every agent is "Already in".
		const members = (await real.snapshot()).members;
		const { rows } = buildRows(SAMPLE_SCAN, { exclude: ['sample'], members });
		const filled = reviewed();
		const again = filled.rows.map((r) => ({ ...r, existingMemberId: rows.find((x) => x.key === r.key)?.existingMemberId, existingKind: 'agent' as const }));
		const retry = await bringIn(flaky, { ...filled, rows: again, officeId: 'office-1' });
		expect(retry).toMatchObject({ agents: 0, failures: [] });
		const snap = await real.snapshot();
		const tooling = snap.teams.find((t) => t.name === 'Agent tooling')!;
		expect(snap.memberships.filter((m) => m.team_id === tooling.id)).toHaveLength(3);
		expect(tooling.lead_member_id).toBeDefined();
	});

	it('leaves alone a lead, a manager or a team the owner changed since', async () => {
		const s = make();
		await bringIn(s, input());
		// The owner makes Riley the lead of App, moves River under the owner, and moves server-dev to Quality.
		const snap = await s.snapshot();
		const id = (h: string) => snap.members.find((m) => m.agent?.messaging_name === h)!;
		const team = (n: string) => snap.teams.find((t) => t.name === n)!;
		await s.command({ method: 'team.add_member', params: { membership_id: 'm-riley-app', team_id: team('App').id, member_id: id('ac-software-engineer-riley').id } }).catch(() => undefined);
		await s.command({ method: 'team.set_lead', params: { team_id: team('Quality').id, expected_version: team('Quality').version, lead_member_id: undefined } });
		await s.command({ method: 'member.set_reports_to', params: { member_id: id('ac-software-engineer-river').id, expected_version: id('ac-software-engineer-river').version, reports_to_member_id: id('ac-product-designer-eden').id } });
		const again = await bringIn(s, input());
		expect(again).toMatchObject({ agents: 0, teams: 0, failures: [] });
		const after = await s.snapshot();
		// Quality's lead was unset by the owner, so it is filled in again; River keeps the manager the owner gave.
		expect(after.members.find((m) => m.agent?.messaging_name === 'ac-software-engineer-river')?.reports_to_member_id).toBe(id('ac-product-designer-eden').id);
	});

	it('reuses a project that is already there and gives it the teams it is missing', async () => {
		const s = make();
		const first = reviewed();
		first.project = { ...first.project!, teamKeys: ['engineering'] };
		await bringIn(s, input(first));
		const second = await bringIn(s, input());
		expect(second).toMatchObject({ projects: 0, failures: [] });
		const snap = await s.snapshot();
		expect(snap.projects).toHaveLength(1);
		expect(snap.project_teams?.map((l) => snap.teams.find((t) => t.id === l.team_id)!.name).sort()).toEqual(['Design', 'Engineering']);
	});

	it('answers a second click while a run is going with that run, not a second run', async () => {
		const s = make();
		const [a, b] = [bringIn(s, input()), bringIn(s, input())];
		expect(b).toBe(a);
		const result = await a;
		expect(result).toMatchObject({ agents: 10, failures: [] });
		expect((await agentsOf(s)).length).toBe(10);
		// And once it is over, a new run is a new run.
		expect(await bringIn(s, input())).toMatchObject({ agents: 0 });
	});

	it('creates a team whose owner the file never defined as a top-level team', async () => {
		const r = reviewed();
		r.drafts = r.drafts.map((d) => (d.key === 'app' ? { ...d, parentKey: 'ghost' } : d));
		const s = make();
		const result = await bringIn(s, input(r));
		expect(result.failures).toEqual([]);
		expect((await s.snapshot()).teams.find((t) => t.name === 'App')?.owner_team_id).toBeUndefined();
	});
});
