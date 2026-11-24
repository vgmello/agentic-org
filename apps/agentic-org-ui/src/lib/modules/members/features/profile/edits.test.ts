import { describe, expect, it } from 'vitest';
import type { Member, OrgSnapshot } from '$lib/proto';
import {
	agentIdentityChange,
	agentIdentityForm,
	agentIdentityMissing,
	offboardPreview
} from './identity';
import { moveBlock, movePreview } from './move';
import { FIRST_POLL_MS, LatestRead, MAX_POLL_MS, awaitingOffice, nextPollDelay } from './pending';
import { hiddenEdges } from './tab-strip';

const kai = {
	id: 'kai',
	kind: 'agent',
	name: 'Kai Lindqvist',
	version: 3,
	agent: {
		role: 'platform engineer',
		title: 'platform engineer',
		focus: 'deploys',
		runtime: 'codex',
		model: 'gpt-5.1-codex',
		home: '/srv/kai',
		messaging_name: 'kai',
		status: 'active'
	}
} as unknown as Member;

describe('Edit from the profile carries identity only (PROF-4)', () => {
	it('sends nothing when nothing changed', () => {
		expect(agentIdentityChange(kai, agentIdentityForm(kai))).toEqual({});
	});
	it('sends only the identity fields that changed, never runtime, model or home', () => {
		const change = agentIdentityChange(kai, {
			...agentIdentityForm(kai),
			title: 'Platform lead',
			focus: '',
			handle: 'kai-l'
		});
		expect(change).toEqual({ title: 'Platform lead', focus: '', messagingName: 'kai-l' });
		for (const key of ['runtime', 'model', 'home']) expect(Object.keys(change)).not.toContain(key);
	});
	it('requires name, role and title but not focus', () => {
		const form = agentIdentityForm(kai);
		expect(agentIdentityMissing({ ...form, focus: '' })).toBe(false);
		expect(agentIdentityMissing({ ...form, role: '  ' })).toBe(true);
	});
});

describe('Offboard preview', () => {
	it('names the handle, not "the name", and keeps the other lines', () => {
		const lines = offboardPreview(
			[
				'Kai Lindqvist leaves Platform.',
				"Kai Lindqvist's record stays, and the name kai stays reserved: nobody else can use it."
			],
			kai,
			'kai'
		);
		expect(lines).toHaveLength(2);
		expect(lines[0]).toBe('Kai Lindqvist leaves Platform.');
		expect(lines[1]).toContain('The handle @kai stays reserved');
	});
});

describe('Move home team', () => {
	const s = {
		teams: [
			{ id: 'app', name: 'App', lead_member_id: 'kai' },
			{ id: 'office', name: 'Office' },
			{ id: 'design', name: 'Design' }
		],
		members: [
			{ id: 'kai', kind: 'agent', name: 'Kai Lindqvist', home_team_id: 'design', version: 1 }
		]
	} as unknown as OrgSnapshot;
	it("previews in the spec's words and names a lead's team left without a lead", () => {
		expect(movePreview(s, kai, 'app', 'office')).toEqual([
			'Kai Lindqvist moves from App to Office. Current project and task assignments stay as recorded.',
			'Kai Lindqvist leads App today; App will have no lead.'
		]);
		expect(movePreview(s, { id: 'x', name: 'Nadia' }, 'app', 'office')).toHaveLength(1);
	});
	it('blocks the current home team and teams they are already in, saying why', () => {
		expect(moveBlock(s, kai, 'app', 'app')).toBe('Current home team');
		expect(moveBlock(s, kai, 'app', 'design')).toBe('Kai Lindqvist is already in this team');
		expect(moveBlock(s, kai, 'app', 'office')).toBeUndefined();
	});
});

describe('waiting for the office after Run now', () => {
	it('re-reads only while a requested turn is unstarted, backing off to 30 s', () => {
		expect(awaitingOffice({ openTurn: { state: 'accepted' } })).toBe(true);
		expect(awaitingOffice({ openTurn: { state: 'running' } })).toBe(false);
		expect(awaitingOffice(null)).toBe(false);
		let d = FIRST_POLL_MS;
		const seen = [d];
		for (let i = 0; i < 6; i++) seen.push((d = nextPollDelay(d)));
		expect(seen.slice(0, 4)).toEqual([2000, 4000, 8000, 16000]);
		expect(Math.max(...seen)).toBe(MAX_POLL_MS);
	});
});

describe('profile reads', () => {
	it('never lets an older read replace a newer profile', () => {
		const reads = new LatestRead();
		const load = reads.start();
		const check = reads.start();
		expect(reads.accept(check)).toBe(true);
		expect(reads.accept(load)).toBe(false);
	});
	it("treats Run now's answer as newest, ahead of a load still in flight", () => {
		const reads = new LatestRead();
		const load = reads.start();
		reads.now();
		expect(reads.accept(load)).toBe(false);
		expect(reads.accept(reads.start())).toBe(true);
	});
});

describe('tab strip', () => {
	it('knows which edges hide tabs', () => {
		expect(hiddenEdges(0, 300, 300)).toBe('');
		expect(hiddenEdges(0, 300, 800)).toBe('right');
		expect(hiddenEdges(200, 300, 800)).toBe('both');
		expect(hiddenEdges(500, 300, 800)).toBe('left');
	});
});
