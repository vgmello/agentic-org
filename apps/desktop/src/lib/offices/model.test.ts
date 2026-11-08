import { describe, expect, it } from 'vitest';
import type { OfficeInfo } from '$lib/proto';
import { sampleOrg } from '$lib/org/sample';
import { agentsIn, displayStatus, displayStatusFor, isHere, orderOffices, roleInfo, rowLabel, secondaryText, statusIcon, statusInfo, unplacedAgents } from './model';

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
		expect(statusInfo(undefined)).toEqual({ label: 'Status unknown', tone: 'muted' });
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

	it('an unrecognised role is never described as HQ or as an office that keeps a copy', () => {
		expect(roleInfo(hq).label).toBe('HQ');
		expect(roleInfo(office('s', 'S')).label).toBe('Office');
		for (const role of ['unknown', undefined] as const) {
			const info = roleInfo(office('x', 'X', { role }));
			expect(info.label).toBe('Unknown');
			expect(info.description).not.toMatch(/keeps/);
		}
	});

	it('the other statuses have their words too', () => {
		expect(statusInfo('starting').label).toBe('Starting');
		expect(statusInfo('offline')).toEqual({ label: 'Offline', tone: 'muted' });
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

describe('OFC-13 each status has its icon, and a row says everything to a screen reader', () => {
	it('gives every status its own icon from the spec, and an unknown one a question mark', () => {
		expect(
			(['active', 'starting', 'offline', 'unavailable', 'update_needed', 'pending', 'unknown', undefined] as const).map((s) => statusIcon(s))
		).toEqual(['circle', 'loader', 'circle-outline', 'circle-x', 'circle-arrow-up', 'mail', 'circle-help', 'circle-help']);
		expect(statusIcon('unreachable')).toBe('wifi-off');
		expect(statusIcon('checking')).toBe('loader');
	});

	it('the row label has the name, HQ, where it runs, the status and the agents', () => {
		expect(rowLabel(hq, 6, true, 'active')).toBe("Main office, HQ, This Mac (you're here), Online, 6 agents, opens details");
		expect(rowLabel(office('s', 'Studio', { host: '192.168.1.31', status: 'offline' }), 1, false, 'offline')).toBe('Studio, 192.168.1.31, Offline, 1 agent, opens details');
	});

	it('leaves the count out while the agents are still being counted', () => {
		expect(rowLabel(hq, undefined, false, 'active')).toBe('Main office, HQ, This Mac, Online, opens details');
	});
});

describe('OFC-12 status is never shown stale', () => {
	it('the connected office is Online only while it is verified, Unreachable when it does not answer, Checking meanwhile', () => {
		expect(displayStatus(true, 'up')).toBe('active');
		// The host answering is not the office being up: another reported status wins over Online.
		expect(displayStatus(true, 'up', 'active')).toBe('active');
		expect(displayStatus(true, 'up', 'unavailable')).toBe('unavailable');
		expect(displayStatus(true, 'up', 'starting')).toBe('starting');
		expect(displayStatus(true, 'down', 'active')).toBe('unreachable');
		expect(displayStatus(true, 'down')).toBe('unreachable');
		expect(displayStatus(true, 'checking')).toBe('checking');
	});

	it('any office the app is not connected to is "Status unknown", whatever was saved', () => {
		for (const check of ['up', 'down', 'checking'] as const) expect(statusInfo(displayStatus(false, check))).toEqual({ label: 'Status unknown', tone: 'muted' });
	});

	it('the words and tones of the new states', () => {
		expect(statusInfo('unreachable')).toEqual({ label: 'Unreachable', tone: 'bad' });
		expect(statusInfo('checking')).toEqual({ label: 'Checking…', tone: 'info' });
	});

	it('the row label says the verified status, not the saved one', () => {
		expect(rowLabel(hq, 1, true, 'unreachable')).toBe("Main office, HQ, Can't reach this office, Unreachable, 1 agent, opens details");
		expect(rowLabel(hq, 1, false, 'status_unknown')).toBe('Main office, HQ, This Mac, Status unknown, 1 agent, opens details');
	});
});

describe('OFC-12 which office is verified', () => {
	it('only the office the app is connected to can be Online; another office with the same saved status reads unknown', () => {
		expect(displayStatusFor('hq', 'hq', 'up')).toBe('active');
		expect(displayStatusFor('office-2', 'hq', 'up')).toBe('status_unknown');
		expect(displayStatusFor('office-2', 'hq', 'down')).toBe('status_unknown');
	});
});
