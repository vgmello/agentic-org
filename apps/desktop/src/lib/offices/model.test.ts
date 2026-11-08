import { describe, expect, it } from 'vitest';
import type { OfficeInfo } from '$lib/proto';
import { sampleOrg } from '$lib/org/sample';
import { agentsIn, isHere, orderOffices, secondaryText, statusInfo, unplacedAgents } from './model';

const office = (id: string, name: string, over: Partial<OfficeInfo> = {}): OfficeInfo => ({
	id,
	organization_id: 'org',
	name,
	role: 'satellite',
	host: 'This Mac',
	status: 'active',
	...over
});
const hq = office('hq', 'Main office', { role: 'hq' });

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

	it('the line under a name: This Mac, invitation, or where it is', () => {
		expect(secondaryText(hq)).toBe('This Mac');
		expect(secondaryText(office('i', 'I', { host: 'i.local', status: 'pending' }))).toBe('invitation');
		expect(secondaryText(office('s', 'S', { host: '192.168.1.31' }))).toBe('192.168.1.31');
		expect(isHere(hq)).toBe(true);
		expect(isHere(office('e', 'E', { host: 'studio.local', endpoint: 'unix:///tmp/o.sock' }))).toBe(true); // structural: it has a local endpoint
		expect(isHere(office('s', 'S', { host: '192.168.1.31' }))).toBe(false);
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
