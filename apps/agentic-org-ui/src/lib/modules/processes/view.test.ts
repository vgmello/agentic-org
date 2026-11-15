import { describe, expect, it } from 'vitest';
import { knownPlace, parsePlace, placeKey, viewOf } from './view';
import { sampleWorld } from './world';

const world = sampleWorld();
const keys = (v: ReturnType<typeof viewOf>) => v.set.entries.map((e) => e.key);

describe('the places the Processes screen can show', () => {
	it('round-trips a place through its text', () => {
		for (const p of [{ kind: 'organization' }, { kind: 'team', id: 'web' }, { kind: 'project', key: 'atl' }, { kind: 'agent', id: 'noor' }] as const) expect(parsePlace(placeKey(p))).toEqual(p);
		expect(parsePlace('nonsense:x')).toEqual({ kind: 'organization' });
		expect(parsePlace('team:a:b')).toEqual({ kind: 'team', id: 'a:b' }); // the id may hold a colon
		expect(parsePlace(null)).toEqual({ kind: 'organization' });
	});

	it('the organization shows its own processes', () => {
		const v = viewOf(world, { kind: 'organization' });
		expect(keys(v)).toEqual(['branch-and-merge', 'code-review', 'security-basics', 'weekly-status']);
		expect(v.rule).toBeUndefined();
	});

	it('a team shows the organization’s, its parents’ and its own', () => {
		const v = viewOf(world, { kind: 'team', id: 'web' });
		expect(keys(v)).toContain('incident-response'); // from Engineering, its parent
		expect(v.set.entries.find((e) => e.key === 'code-review')).toMatchObject({ source: 'team', scopeName: 'Web' });
		expect(v.set.entries.find((e) => e.key === 'code-review')!.drift).toMatchObject({ parentVersion: 2 });
	});

	it('a project shows the rule’s state and its conflicts and problems', () => {
		const beacon = viewOf(world, { kind: 'project', key: 'bea' });
		expect(beacon.set.conflicts.map((c) => c.key)).toEqual(['code-review']);
		expect(beacon.set.problems.some((p) => p.kind === 'clash')).toBe(true);
		expect(viewOf(world, { kind: 'project', key: 'atl' }).rule).toEqual({ on: true, text: 'On' });
		expect(viewOf(world, { kind: 'project', key: 'onb' }).rule?.text).toMatch(/^On · turning it off needs approval by someone other than its author \(hana\)/);
	});

	it('an agent with current work follows its project’s set plus its own; with none, the organization’s and its own', () => {
		const noor = viewOf(world, { kind: 'agent', id: 'noor' });
		expect(noor.noCurrentWork).toBe(false);
		expect(keys(noor)).toContain('my-notes');
		const sam = viewOf(world, { kind: 'agent', id: 'sam' });
		expect(sam.noCurrentWork).toBe(true);
		expect(sam.subtitle).toBe("No current work. Showing the organization's processes and its own. Team and project processes apply when it starts a task.");
		expect(keys(sam)).not.toContain('release-checklist');
	});

	it('a place that is not in the world falls back to the organization', () => {
		expect(knownPlace(world, { kind: 'team', id: 'zzz' })).toEqual({ place: { kind: 'organization' }, missing: true });
		expect(knownPlace(world, { kind: 'agent', id: 'noor' })).toEqual({ place: { kind: 'agent', id: 'noor' }, missing: false });
	});

	it('the one open branch rule is on where a project has no branch-and-merge in its set (missing, clashed or invalid leaves it on)', () => {
		expect(viewOf({ ...world, docs: [] }, { kind: 'project', key: 'atl' }).rule).toEqual({ on: true, text: 'On' });
		expect(viewOf({ ...world, docs: [] }, { kind: 'team', id: 'web' }).rule).toBeUndefined(); // a team has no project of its own
	});
});
