import { describe, expect, it } from 'vitest';
import type { ListReply } from './api';
import { readOrganizationsList } from './readers';
import { office, organization } from './testing';
import { createdDate, openOrganization, organizationRows } from './welcome-list';

const OTHER = '0193a5c0-0000-7000-8000-0000000000aa';
const list = (...organizations: unknown[]) =>
	readOrganizationsList({ organizations, creating: [] });
const listed = (...organizations: unknown[]): ListReply => ({
	kind: 'listed',
	list: list(...organizations)
});
const down = {
	...organization,
	offices: [{ ...office, status: 'unavailable', status_reason: 'crashed' }]
};

describe('organizationRows', () => {
	it('shows each organization with its HQ office, host, status, created date and the open one', () => {
		const second = {
			...organization,
			id: OTHER,
			name: 'Lumen Labs',
			offices: [{ ...office, status: 'active', host: 'hq-mac' }]
		};
		const rows = organizationRows(list(organization, second).organizations, OTHER, 'en-GB');
		expect(rows).toEqual([
			{
				id: organization.id,
				name: 'Side Project',
				office: 'HQ · This Mac',
				status: 'Starting',
				created: '10 Nov 2026',
				unavailable: false,
				openNow: false
			},
			{
				id: OTHER,
				name: 'Lumen Labs',
				office: 'HQ · hq-mac',
				status: 'Online',
				created: '10 Nov 2026',
				unavailable: false,
				openNow: true
			}
		]);
	});
	it('names the HQ office even when a satellite is listed first', () => {
		const satellite = {
			...office,
			id: 'sat',
			name: 'Lab',
			role: 'satellite',
			host: 'lab-mac',
			status: 'active'
		};
		const [row] = organizationRows(
			list({ ...organization, offices: [satellite, office] }).organizations,
			undefined
		);
		expect(row.office).toBe('HQ · This Mac');
	});
	it('marks an unavailable HQ, and an organization with no office, as unavailable', () => {
		const rows = organizationRows(
			list(down, { ...organization, id: OTHER, offices: [] }).organizations,
			undefined
		);
		expect(rows.map((r) => [r.unavailable, r.status, r.office])).toEqual([
			[true, 'Unavailable', 'HQ · This Mac'],
			[true, 'Unavailable', undefined]
		]);
	});
	it('leaves the date empty when the timestamp cannot be read', () => {
		expect(createdDate('yesterday')).toBe('');
	});
});

describe('openOrganization', () => {
	it('opens from a fresh list, not from what the page showed', async () => {
		let reads = 0;
		const result = await openOrganization(
			{ list: async () => (reads++, listed(organization)) },
			organization.id
		);
		expect(reads).toBe(1);
		expect(result).toEqual({ kind: 'opened', organization: list(organization).organizations[0] });
	});
	it('reports an organization that vanished, with the list that no longer has it', async () => {
		const result = await openOrganization({ list: async () => listed() }, organization.id);
		expect(result).toEqual({ kind: 'vanished', list: { organizations: [], creating: [] } });
	});
	it('does not open an organization whose HQ went down after the list was shown', async () => {
		const result = await openOrganization({ list: async () => listed(down) }, organization.id);
		expect(result.kind).toBe('unavailable');
	});
	it('passes on an unreachable HQ and an ended session', async () => {
		expect(
			await openOrganization({ list: async () => ({ kind: 'unreachable' }) }, organization.id)
		).toEqual({ kind: 'unreachable' });
		expect(
			await openOrganization({ list: async () => ({ kind: 'session' }) }, organization.id)
		).toEqual({ kind: 'session' });
	});
});
