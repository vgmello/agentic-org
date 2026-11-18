import { describe, expect, it } from 'vitest';
import type { OfficeInfo } from '$lib/proto';
import { layerOffices, satelliteWhere, syncText } from './layer';

const hq: OfficeInfo = { id: 'hq', organization_id: 'o', name: 'HQ', role: 'hq', host: 'This Mac', status: 'active' };
const sat: OfficeInfo = { id: 's', organization_id: 'o', name: 'Studio', role: 'satellite', host: 'Studio Mac', status: 'offline' };
const details = { officeId: 's', where: 'from 192.168.1.31', lastSeenAt: '2026-11-18T10:00:00Z', sync: { kind: 'behind' as const, changes: 34 }, follows: [], tasks: 1 };

describe('layerOffices', () => {
	it('adds the offices that joined and lays new names over', () => {
		const next = layerOffices([hq], { satellites: [sat], names: { s: 'Studio 2', hq: 'Main' } });
		expect(next.map((o) => o.name)).toEqual(['Main', 'Studio 2']);
	});
	it('does not list an office twice', () => {
		expect(layerOffices([hq, sat], { satellites: [sat], names: {} })).toHaveLength(2);
	});
	it('leaves the list alone before anything is known', () => {
		const base = [hq];
		expect(layerOffices(base, undefined)).toBe(base);
	});
});

describe('satelliteWhere', () => {
	const now = Date.parse('2026-11-18T12:00:00Z');
	it('says when an offline office was last seen, and that a joining one is copying records', () => {
		expect(satelliteWhere(sat, details, now)).toBe('last seen 2 h ago');
		expect(satelliteWhere({ ...sat, status: 'starting' }, details, now)).toBe('from 192.168.1.31 · copying records');
		expect(satelliteWhere({ ...sat, status: 'active' }, details, now)).toBe('from 192.168.1.31');
	});
});

describe('syncText', () => {
	it('says up to date or how many changes it is behind', () => {
		expect(syncText(details)).toBe('Catching up: 34 changes');
		expect(syncText({ ...details, sync: { kind: 'up_to_date' } })).toBe('Up to date');
	});
});
