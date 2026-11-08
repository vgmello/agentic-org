import { describe, expect, it } from 'vitest';
import { createMockOrg, type MockStorage } from '$lib/org/api';
import type { OrgSession } from '$lib/org/session';
import { sessionFor } from '$lib/org/session';
import type { Member } from '$lib/proto';
import { bringIn, type BringInInput } from './bring-in';
import { applyHints } from './hints';
import { memoryLedger, type LedgerStore } from './ledger';
import { SAMPLE_HINTS, SAMPLE_SCAN } from './mock';
import { buildRows, chooseRole } from './rows';
import { suggestProject, suggestTeams } from './teams';

const memory = (): MockStorage => {
	const m = new Map<string, string>();
	return { getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v) };
};
const make = () => sessionFor(createMockOrg(async () => 'office-1', { storage: memory(), empty: () => true, delayMs: () => 0 }), 'o');

/** The review as the owner leaves it for our own team: everyone filled in, the hints file chosen. */
function reviewed(members: Member[] = []) {
	const { rows } = buildRows(SAMPLE_SCAN, { exclude: ['sample'], members });
	const filled = rows.map((r) => {
		if (r.runtime) return r;
		const withRole = chooseRole(r, 'software engineer');
		return { ...withRole, runtime: r.name === 'server-dev' || r.name === 'codex-dev' ? ('codex' as const) : r.name === 'agy-senior-dev' ? ('antigravity' as const) : ('claude_code' as const) };
	});
	const fitted = applyHints(filled, SAMPLE_HINTS, suggestTeams(filled));
	return { rows: fitted.rows, drafts: fitted.drafts, project: suggestProject(fitted.rows, fitted.drafts) };
}

/** An organization and what the import has kept about it: a run later in the same app. */
const world = () => ({ s: make(), ledger: memoryLedger() as LedgerStore });
type World = ReturnType<typeof world>;
const input = (w: World, r = reviewed()): BringInInput => ({ ...r, organizationId: 'o', officeId: 'office-1', ledger: w.ledger });
const run = (w: World, r = reviewed(), session: OrgSession = w.s) => bringIn(session, input(w, r));
const agentsOf = async (s: OrgSession) => (await s.snapshot()).members.filter((m) => m.kind === 'agent');
const find = async (s: OrgSession) => {
	const snap = await s.snapshot();
	return {
		snap,
		team: (n: string) => snap.teams.find((t) => t.name === n)!,
		member: (h: string) => snap.members.find((m) => m.agent?.messaging_name === h)!
	};
};

describe('IMP-7 bringing the reviewed team in', () => {
	it('creates the agents, the teams under their owners, the leads, the reporting lines and the project', async () => {
		const w = world();
		const said: string[] = [];
		const result = await bringIn(w.s, { ...input(w), onProgress: (t) => said.push(t) });
		expect(result).toMatchObject({ agents: 10, teams: 6, projects: 1, paused: 0, failures: [] });
		expect(said).toEqual(['Creating teams…', 'Adding agents…', 'Putting agents in their teams…', 'Linking leads and reporting lines…', 'Setting up the project…']);

		const { snap, team, member } = await find(w.s);
		expect(team('App').owner_team_id).toBe(team('Engineering').id);
		expect(team('Design').owner_team_id).toBeUndefined();
		expect(team('App').lead_member_id).toBe(member('app-frontend-dev').id);
		expect(team('Engineering').lead_member_id).toBe(member('ac-staff-engineer-ada').id);
		const inTeam = (t: string) => snap.memberships.filter((m) => m.team_id === team(t).id).map((m) => snap.members.find((x) => x.id === m.member_id)!.name).sort();
		expect(inTeam('Office')).toEqual(['River', 'server-dev']);
		expect(inTeam('Agent tooling')).toEqual(['Remy', 'agy-senior-dev', 'codex-dev']);
		expect(member('ac-software-engineer-river').reports_to_member_id).toBe(member('ac-staff-engineer-ada').id);
		expect(member('ac-staff-engineer-ada').reports_to_member_id).toBeUndefined();
		expect(member('server-dev').reports_to_member_id).toBeUndefined();
		expect(member('codex-dev').agent).toMatchObject({ runtime: 'codex', status: 'active' });
		expect(member('ac-staff-engineer-ada').agent).toMatchObject({ runtime: 'claude_code', title: 'Staff Engineer', role: 'staff engineer' });
		expect(snap.projects?.map((p) => p.key)).toEqual(['ac']);
		expect(snap.project_teams?.map((l) => snap.teams.find((t) => t.id === l.team_id)!.name).sort()).toEqual(['Design', 'Engineering']);
	});

	it('brings an agent in paused when the owner chose so, paused at once with no moment as active', async () => {
		const r = reviewed();
		r.rows.find((x) => x.name === 'Riley')!.choice = 'paused';
		const w = world();
		const statuses: string[] = [];
		const spy: OrgSession = {
			snapshot: () => w.s.snapshot(),
			command: (c) => {
				if (c.method === 'member.add_agent' && c.params.name === 'Riley') statuses.push(c.params.profile.status);
				if (c.method === 'member.set_agent_status') statuses.push('set');
				return w.s.command(c);
			}
		};
		const result = await run(w, r, spy);
		expect(statuses).toEqual(['paused']);
		expect(result.paused).toBe(1);
		expect((await agentsOf(w.s)).find((m) => m.name === 'Riley')?.agent?.status).toBe('paused');
	});

	it('leaves a skipped agent out, and a team only they would have filled', async () => {
		const r = reviewed();
		r.rows.find((x) => x.name === 'reviewer')!.choice = 'skip';
		const w = world();
		const result = await run(w, r);
		expect(result).toMatchObject({ agents: 9, teams: 5 });
		expect((await w.s.snapshot()).teams.map((t) => t.name)).not.toContain('Quality');
	});

	it('puts a lead of a parent team in that team even when the file places them in a deeper one', async () => {
		const r = reviewed();
		const hints = {
			path: 'f',
			teams: [
				{ key: 'engineering', name: 'Engineering', lead: 'ac-staff-engineer-ada', members: [] },
				{ key: 'app', name: 'App', parent: 'engineering', lead: 'app-frontend-dev', members: ['ac-staff-engineer-ada', 'app-frontend-dev'] }
			]
		};
		const fitted = applyHints(r.rows, hints, []);
		const w = world();
		const result = await run(w, { rows: fitted.rows, drafts: fitted.drafts, project: undefined });
		expect(result.failures).toEqual([]);
		const { snap, team, member } = await find(w.s);
		const ada = member('ac-staff-engineer-ada');
		expect(team('Engineering').lead_member_id).toBe(ada.id);
		expect(snap.memberships.some((m) => m.team_id === team('Engineering').id && m.member_id === ada.id)).toBe(true);
	});

	it('creates a team whose owner the file never defined as a top-level team', async () => {
		const r = reviewed();
		r.drafts = r.drafts.map((d) => (d.key === 'app' ? { ...d, parentKey: 'ghost' } : d));
		const w = world();
		const result = await run(w, r);
		expect(result.failures).toEqual([]);
		expect((await w.s.snapshot()).teams.find((t) => t.name === 'App')?.owner_team_id).toBeUndefined();
	});
});

describe('IMP-8 running it again', () => {
	it('finds nothing new: no duplicate agent, team, membership or project', async () => {
		const w = world();
		await run(w);
		const before = await w.s.snapshot();
		const again = await run(w);
		expect(again).toMatchObject({ agents: 0, teams: 0, projects: 0, paused: 0, failures: [] });
		const after = await w.s.snapshot();
		expect(after.members).toHaveLength(before.members.length);
		expect(after.teams).toHaveLength(before.teams.length);
		expect(after.memberships).toHaveLength(before.memberships.length);
		expect(after.projects).toHaveLength(before.projects?.length ?? 0);
	});

	it('adds only the agent that is new', async () => {
		const w = world();
		const first = reviewed();
		first.rows.find((x) => x.name === 'reviewer')!.choice = 'skip';
		await run(w, first);
		const second = await run(w);
		expect(second).toMatchObject({ agents: 1, teams: 1, failures: [] });
		expect((await agentsOf(w.s)).map((m) => m.name)).toContain('reviewer');
	});

	it('does not undo what the owner changed since: a renamed team, a cleared manager, an agent taken out of its team, a cleared lead', async () => {
		const w = world();
		await run(w);
		const { team, member, snap } = await find(w.s);
		// The owner renames App, makes River report to the owner, takes server-dev out of Office, and clears Quality's lead.
		await w.s.command({ method: 'team.rename', params: { team_id: team('App').id, expected_version: team('App').version, name: 'Desktop' } });
		await w.s.command({ method: 'member.set_reports_to', params: { member_id: member('ac-software-engineer-river').id, expected_version: member('ac-software-engineer-river').version, reports_to_member_id: undefined } });
		const link = snap.memberships.find((m) => m.team_id === team('Office').id && m.member_id === member('server-dev').id)!;
		await w.s.command({ method: 'team.remove_member', params: { membership_id: link.id, expected_version: link.version } });
		await w.s.command({ method: 'team.set_lead', params: { team_id: team('Quality').id, expected_version: team('Quality').version, lead_member_id: undefined } });

		const again = await run(w);
		expect(again).toMatchObject({ agents: 0, teams: 0, projects: 0, failures: [] });
		const after = await find(w.s);
		expect(after.snap.teams.map((t) => t.name).sort()).toEqual(['Agent tooling', 'Design', 'Desktop', 'Engineering', 'Office', 'Quality']);
		expect(after.member('ac-software-engineer-river').reports_to_member_id).toBeUndefined();
		expect(after.snap.memberships.some((m) => m.member_id === after.member('server-dev').id && m.team_id === after.team('Office').id)).toBe(false);
		expect(after.team('Quality').lead_member_id).toBeUndefined();
	});

	it('leaves alone an agent the owner offboarded and a team the owner deleted', async () => {
		const w = world();
		await run(w);
		const { member } = await find(w.s);
		await w.s.command({ method: 'member.offboard', params: { member_id: member('reviewer').id, expected_version: member('reviewer').version } });
		// Offboarding took the reviewer out of Quality, which is now empty: the owner deletes it.
		const quality = (await find(w.s)).team('Quality');
		await w.s.command({ method: 'team.delete', params: { team_id: quality.id, expected_version: quality.version } });
		expect((await w.s.snapshot()).teams.map((t) => t.name)).not.toContain('Quality');
		const again = await run(w);
		expect(again).toMatchObject({ agents: 0, failures: [] });
		expect((await w.s.snapshot()).teams.map((t) => t.name)).not.toContain('Quality');
	});

	it('reuses a project that is already there and gives it the teams it is missing', async () => {
		const w = world();
		const first = reviewed();
		first.project = { ...first.project!, teamKeys: ['engineering'] };
		await run(w, first);
		// Another run with no memory of the first (a ledger that was lost) adopts the project instead of making a second.
		const second = await bringIn(w.s, { ...input(w), ledger: memoryLedger() });
		expect(second).toMatchObject({ projects: 0, failures: [] });
		const snap = await w.s.snapshot();
		expect(snap.projects).toHaveLength(1);
		expect(snap.project_teams?.map((l) => snap.teams.find((t) => t.id === l.team_id)!.name).sort()).toEqual(['Design', 'Engineering']);
	});

	it('with no memory of an earlier run, finds teams and agents by name and fills in only what is empty', async () => {
		const w = world();
		await run(w);
		const { team, member } = await find(w.s);
		await w.s.command({ method: 'member.set_reports_to', params: { member_id: member('ac-software-engineer-river').id, expected_version: member('ac-software-engineer-river').version, reports_to_member_id: member('ac-product-designer-eden').id } });
		const members = (await w.s.snapshot()).members;
		const lost = { ...input(w, reviewed(members)), ledger: memoryLedger() };
		const result = await bringIn(w.s, lost);
		expect(result).toMatchObject({ agents: 0, teams: 0, projects: 0, failures: [] });
		const after = await find(w.s);
		expect(after.team('Office').id).toBe(team('Office').id);
		expect(after.member('ac-software-engineer-river').reports_to_member_id).toBe(member('ac-product-designer-eden').id);
	});

	it('does not take an agent offboarded in the organization into the team again', async () => {
		const w = world();
		await run(w);
		const { member } = await find(w.s);
		await w.s.command({ method: 'member.offboard', params: { member_id: member('codex-dev').id, expected_version: member('codex-dev').version } });
		const members = (await w.s.snapshot()).members;
		const fresh = { ...input(w, reviewed(members)), ledger: memoryLedger() };
		const result = await bringIn(w.s, fresh);
		expect(result.failures).toEqual([]);
		expect(result.agents).toBe(0);
	});
});

describe('IMP-9 an interrupted import continues where it stopped', () => {
	it('reports the agent that failed, brings the others in, and Try again adds only what is missing', async () => {
		const w = world();
		let fail = true;
		const flaky: OrgSession = {
			snapshot: () => w.s.snapshot(),
			command: (c) => {
				if (fail && c.method === 'member.add_agent' && c.params.name === 'codex-dev') return Promise.reject({ source: 'api', code: 'unavailable', message: 'The office went away.' });
				return w.s.command(c);
			}
		};
		const first = await run(w, reviewed(), flaky);
		expect(first.agents).toBe(9);
		expect(first.failures).toHaveLength(1);
		expect(first.failures[0]).toMatchObject({ what: 'Add codex-dev', error: { code: 'unavailable' } });
		expect((await agentsOf(w.s)).map((m) => m.name)).not.toContain('codex-dev');

		fail = false;
		const retry = await run(w, reviewed(), flaky);
		expect(retry).toMatchObject({ agents: 1, teams: 0, failures: [] });
		const { snap, team } = await find(w.s);
		expect(snap.members.filter((m) => m.kind === 'agent')).toHaveLength(10);
		expect(snap.memberships.filter((m) => m.team_id === team('Agent tooling').id)).toHaveLength(3);
	});

	it('continues from a review rebuilt after the interruption: agents already in still get their teams and lines', async () => {
		const w = world();
		let fail = true;
		const flaky: OrgSession = {
			snapshot: () => w.s.snapshot(),
			command: (c) => (fail && c.method === 'team.add_member' ? Promise.reject({ source: 'api', code: 'unavailable', message: 'gone' }) : w.s.command(c))
		};
		const first = await run(w, reviewed(), flaky);
		expect(first.agents).toBe(10);
		expect(first.failures.length).toBeGreaterThan(0);
		fail = false;
		// The review is built again from what the organization has now: every agent is "Already in".
		const rebuilt = reviewed((await w.s.snapshot()).members);
		expect(rebuilt.rows.every((r) => r.existingMemberId)).toBe(true);
		const retry = await run(w, rebuilt, flaky);
		expect(retry).toMatchObject({ agents: 0, failures: [] });
		const { snap, team } = await find(w.s);
		expect(snap.memberships.filter((m) => m.team_id === team('Agent tooling').id)).toHaveLength(3);
		expect(team('Agent tooling').lead_member_id).toBeDefined();
	});

	it('does not make a team whose owner could not be made, and says why for each', async () => {
		const w = world();
		const noEngineering: OrgSession = {
			snapshot: () => w.s.snapshot(),
			command: (c) => (c.method === 'team.create' && c.params.name === 'Engineering' ? Promise.reject({ source: 'api', code: 'unavailable', message: 'no' }) : w.s.command(c))
		};
		const result = await run(w, reviewed(), noEngineering);
		expect(result.failures.map((f) => f.what).sort()).toEqual(['Create Agent tooling', 'Create App', 'Create Engineering', 'Create Office', 'Create Quality']);
		expect(result.failures.find((f) => f.what === 'Create App')?.error?.message).toMatch(/owner team couldn't be made/);
		expect((await w.s.snapshot()).teams.map((t) => t.name)).toEqual(['Design']);
	});

	it('names the lead from the version it saw, so a change made meanwhile is a conflict, not overwritten', async () => {
		const w = world();
		const racing: OrgSession = {
			snapshot: () => w.s.snapshot(),
			command: async (c) => {
				if (c.method === 'team.set_lead' && c.params.team_id) {
					// Someone renames the team between the read and the write.
					const t = (await w.s.snapshot()).teams.find((x) => x.id === c.params.team_id)!;
					await w.s.command({ method: 'team.rename', params: { team_id: t.id, expected_version: t.version, name: `${t.name}!` } });
				}
				return w.s.command(c);
			}
		};
		const result = await run(w, reviewed(), racing);
		expect(result.failures.length).toBeGreaterThan(0);
		expect(result.failures.every((f) => f.what.startsWith('Set the lead') && f.error?.code === 'conflict')).toBe(true);
		// ...and the next run, with the leads still owed, sets them.
		const after = await run(w, reviewed());
		expect(after.failures).toEqual([]);
	});
});

describe('one run at a time', () => {
	it('answers a second click while a run is going with that run, not a second run, for the organization', async () => {
		const w = world();
		const [a, b] = [bringIn(w.s, input(w)), bringIn(make(), input(w))];
		expect(b).toBe(a);
		const result = await a;
		expect(result).toMatchObject({ agents: 10, failures: [] });
		expect((await agentsOf(w.s)).length).toBe(10);
		// And once it is over, a new run is a new run.
		expect(await run(w)).toMatchObject({ agents: 0 });
	});
});
