import { expect, type Page } from '@playwright/test';
import { visit } from './ready';
import type { Connection, OfficeInfo, Organization } from '../src/lib/proto';

// The mocked office's own storage keys (src/lib/office/mock.ts).
const CONNECTIONS_KEY = 'agentic-org.mock.connections';
const ORGANIZATIONS_KEY = 'agentic-org.mock.organizations';

/** The mock's HQ office, with the stopped state its `?office=` scenario gives it. */
function hqOffice(organizationId: string, stopped: string | null): OfficeInfo {
	const base: OfficeInfo = {
		id: `office-${organizationId}`,
		organization_id: organizationId,
		name: 'HQ',
		role: 'hq',
		host: 'This Mac',
		status: 'active'
	};
	if (stopped === 'crashed') return { ...base, status: 'unavailable', status_reason: 'crashed' };
	if (stopped === 'restarting')
		return {
			...base,
			status: 'offline',
			status_reason: 'crashed',
			next_restart_at: new Date(Date.now() + 10 * 60_000).toISOString()
		};
	if (stopped === 'stopped_by_owner')
		return { ...base, status: 'unavailable', status_reason: 'stopped_by_owner' };
	if (stopped === 'unknown') return { ...base, status: 'unavailable' };
	return base;
}

/**
 * Enter the mocked shell with an organization called `name`, landing on the Members directory
 * (`/agents` redirects there in the shell).
 *
 * The Create organization page talks to the web API (FC-018), so the specs of other areas no
 * longer create their organization through it. This puts the mocked office (src/lib/office/mock.ts)
 * into the state its own `createOrganization` leaves: the two sample organizations plus the new
 * one in the hosted list, and the new one remembered as the connection the shell opens.
 * `createPath` is the path the old entry visited; only its `?office=` scenario is read.
 */
export async function enterMockedOrganization(page: Page, name = 'Acme', createPath = '/create') {
	const stopped = new URLSearchParams(createPath.split('?')[1] ?? '').get('office');
	const organization = (
		id: string,
		organizationName: string,
		created_at: string
	): Organization => ({ id, name: organizationName, created_at, offices: [hqOffice(id, stopped)] });
	const hosted = [
		organization('sample-northwind', 'Northwind Labs', '2026-09-14T09:30:00.000Z'),
		organization('sample-globex', 'Globex Research', '2026-10-02T16:05:00.000Z')
	];
	const created = organization(
		`org-${Date.now().toString(36)}-${hosted.length}`,
		name,
		new Date().toISOString()
	);
	const connection: Connection = { office: created.offices[0], organization: created };
	// A static file gives the origin its storage without starting the app.
	await page.goto('/favicon.svg');
	await page.evaluate(
		([connectionsKey, organizationsKey, organizations, remembered]) => {
			const kept = (
				JSON.parse(localStorage.getItem(connectionsKey) ?? '[]') as {
					organization: { id: string };
				}[]
			).filter((c) => c.organization.id !== remembered.organization.id);
			const before = localStorage.getItem(organizationsKey);
			localStorage.setItem(
				organizationsKey,
				JSON.stringify([
					...(before ? (JSON.parse(before) as unknown[]) : organizations.slice(0, -1)),
					organizations[organizations.length - 1]
				])
			);
			localStorage.setItem(connectionsKey, JSON.stringify([...kept, remembered]));
		},
		[CONNECTIONS_KEY, ORGANIZATIONS_KEY, [...hosted, created], connection] as const
	);
	await visit(page, '/agents');
	await expect(page).toHaveURL(/\/teams\/members$/);
}
