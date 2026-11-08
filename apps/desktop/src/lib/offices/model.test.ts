import { describe, expect, it } from 'vitest';
import type { OfficeWithExtras } from '$lib/proto';
import { sampleOrg } from '$lib/org/sample';
import { ADD_STEPS, agentsIn, ago, removeSummary, isHere, orderOffices, removeConsequences, secondaryText, statusInfo, suggestOfficeName, unplacedAgents, validateOfficeName } from './model';

const office = (id: string, name: string, over: Partial<OfficeWithExtras> = {}): OfficeWithExtras => ({
	id,
	organization_id: 'org',
	name,
	role: 'satellite',
	host: 'This Mac',
	status: 'active',
	...over
});
const hq = office('hq', 'Main office', { role: 'hq' });
const NOW = Date.parse('2026-11-08T12:00:00.000Z');

describe('OFC-1 the list order and what each row says', () => {
	it('HQ, this Mac’s offices, other offices A–Z, then invitations', () => {
		const list = orderOffices([
			office('i', 'Laptop', { status: 'pending', host: 'laptop.local' }),
			office('b', 'Build box', { host: 'build.local', status: 'offline' }),
			office('l2', 'Office 2'),
			hq,
			office('s', 'Studio Mac', { host: '192.168.1.31' }),
			office('l1', 'Aardvark'),
			office('b2', 'Zeta box', { host: 'z.local' })
		]);
		expect(list.map((o) => o.name)).toEqual(['Main office', 'Aardvark', 'Office 2', 'Build box', 'Studio Mac', 'Zeta box', 'Laptop']);
	});

	it('every status has its word and a tone, and an unknown one is never read as healthy', () => {
		expect(statusInfo('active')).toEqual({ label: 'Online', tone: 'ok' });
		expect(statusInfo('unavailable').label).toBe('Stopped');
		expect(statusInfo('update_needed').tone).toBe('warn');
		expect(statusInfo('pending').label).toBe('Invited');
		expect(statusInfo(undefined)).toEqual({ label: 'Unknown', tone: 'muted' });
		expect(statusInfo('unknown').tone).not.toBe('ok');
	});

	it('the line under a name: This Mac, last seen, invitation, or where it is', () => {
		expect(secondaryText(hq, NOW)).toBe('This Mac');
		expect(secondaryText(office('b', 'B', { host: 'b.local', status: 'offline', last_seen: '2026-11-08T10:00:00.000Z' }), NOW)).toBe('last seen 2 h ago');
		expect(secondaryText(office('i', 'I', { host: 'i.local', status: 'pending' }), NOW)).toBe('invitation');
		expect(secondaryText(office('x', 'X', { status: 'unavailable', stopped_reason: "The office server didn't answer in time." }), NOW)).toBe("The office server didn't answer in time."); // a stopped office says why
		expect(secondaryText(office('y', 'Y', { status: 'unavailable' }), NOW)).toBe('This Mac');
		expect(secondaryText(office('s', 'S', { host: '192.168.1.31' }), NOW)).toBe('192.168.1.31');
		expect(isHere(hq)).toBe(true);
	});

	it('time ago: just now, minutes, hours, days, and unreadable text passes through', () => {
		expect(ago('2026-11-08T11:59:40.000Z', NOW)).toBe('just now');
		expect(ago('2026-11-08T11:55:00.000Z', NOW)).toBe('5 min ago');
		expect(ago('2026-11-08T09:00:00.000Z', NOW)).toBe('3 h ago');
		expect(ago('2026-11-08T11:00:01.000Z', NOW)).toBe('59 min ago'); // never "60 min ago"
		expect(ago('2026-11-05T12:00:00.000Z', NOW)).toBe('3 d ago');
		expect(ago('nonsense', NOW)).toBe('nonsense');
	});

	it('counts the agents placed in an office; people are never placed', () => {
		const s = sampleOrg('office-1');
		expect(agentsIn(s, 'office-1').length).toBe(7);
		expect(agentsIn(s, 'office-1').some((m) => m.kind !== 'agent')).toBe(false);
		expect(agentsIn(s, 'nowhere')).toEqual([]);
	});

	it('agents whose office is gone, or who have none, are unplaced', () => {
		const s = sampleOrg('office-1');
		expect(unplacedAgents(s, [office('office-1', 'HQ', { role: 'hq' })]).map((m) => m.id)).toEqual(['ines']);
		expect(unplacedAgents(s, []).length).toBe(8);
	});
});

describe('OFC-2 naming an office', () => {
	const existing = [hq, office('o2', 'Office 2')];

	it('is required, 1–64 characters, no control characters', () => {
		expect(validateOfficeName('   ', existing)).toEqual({ ok: false, message: 'Give the office a name.' });
		expect(validateOfficeName('x'.repeat(65), existing).ok).toBe(false);
		expect(validateOfficeName('x'.repeat(64), existing).ok).toBe(true);
		expect(validateOfficeName('a\nb', existing).ok).toBe(false);
		expect(validateOfficeName('  Studio Mac  ', existing)).toEqual({ ok: true, name: 'Studio Mac' });
	});

	it('is unique among the organization’s offices, ignoring case', () => {
		expect(validateOfficeName('office 2', existing)).toEqual({ ok: false, message: "There's already an office called Office 2." });
		expect(validateOfficeName('MAIN OFFICE', existing).ok).toBe(false);
	});

	it('suggests the first free "Office N"', () => {
		expect(suggestOfficeName([hq])).toBe('Office 2');
		expect(suggestOfficeName(existing)).toBe('Office 3');
		expect(suggestOfficeName([hq, office('x', 'office 2'), office('y', 'Office 3')])).toBe('Office 4');
	});
});

describe('OFC-8 removing an office', () => {
	const s = sampleOrg('office-1');
	it('HQ can’t be removed', () => {
		expect(removeConsequences(hq, s).allowed).toBe(false);
	});

	it('names the agents that stop running there, and whether it is offline', () => {
		const o = office('office-1', 'Office 2');
		const r = removeConsequences(o, s);
		expect(r.allowed).toBe(true);
		expect(r.agents.length).toBe(7);
		expect(r.offline).toBe(false);
		expect(removeConsequences({ ...o, status: 'offline' }, s).offline).toBe(true);
		expect(removeConsequences(office('empty', 'Empty'), s).agents).toEqual([]);
	});
});

describe('OFC-8 the removal wording', () => {
	const s = sampleOrg('office-1');
	it('names the agents that become unplaced and says HQ takes back their tasks', () => {
		const text = removeSummary(office('office-1', 'Office 2'), 'Acme', s).join(' ');
		expect(text).toContain("Office 2 can't sync again.");
		expect(text).toContain('Its 7 agents (');
		expect(text).toContain('Sam Okafor');
		expect(text).toContain('become unplaced');
		expect(text).toContain('HQ takes back any open tasks');
		expect(text).toContain('You can add it to Acme again later.');
	});

	it('says no agents run there when none do, and explains the offline case', () => {
		const empty = removeSummary(office('empty', 'Empty'), 'Acme', s).join(' ');
		expect(empty).toContain('No agents run there.');
		expect(empty).toContain('If Empty is offline when you remove it, it keeps its copy until it next connects');
		const offline = removeSummary(office('empty', 'Empty', { status: 'offline' }), 'Acme', s).join(' ');
		expect(offline).toContain('keeps its copy until it next connects, then deletes it');
	});

	it('the add steps are creating, copying records, starting', () => {
		expect([...ADD_STEPS]).toEqual(['Creating', 'Copying records', 'Starting']);
	});
});
