import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import type { OfficeInfo } from '../src/lib/proto';

type HeldRefresh = {
	started: Promise<OfficeInfo>;
	release: () => Promise<void>;
};

async function holdFirstRefresh(page: Page, stoppedSnapshot = false) {
	await page.evaluate(async (stoppedSnapshot) => {
		// The same dev module instance used by the route. No production hook is added.
		const modulePath = '/src/lib/office/api.ts';
		const { officeApi } = await import(modulePath);
		const peek = officeApi.peekOrganizations.bind(officeApi);
		const connection = (await officeApi.connections()).at(-1)!;
		let started!: (office: OfficeInfo) => void;
		let release!: () => void;
		const began = new Promise<OfficeInfo>((resolve) => (started = resolve));
		const gate = new Promise<void>((resolve) => (release = resolve));
		let pending: Promise<unknown>;
		let first = true;
		officeApi.peekOrganizations = () => {
			if (!first) return peek();
			first = false;
			pending = (async () => {
				const organizations = await peek();
				const office = organizations.find((organization: { id: string }) => organization.id === connection.organization.id).offices.find((office: OfficeInfo) => office.id === connection.office.id);
				if (stoppedSnapshot) {
					office.status = 'unavailable';
					office.status_reason = 'crashed';
				}
				started(office);
				await gate;
				return organizations;
			})();
			return pending;
		};
		(window as unknown as { __heldOfficeRefresh: HeldRefresh }).__heldOfficeRefresh = {
			started: began,
			release: async () => { release(); await pending; }
		};
	}, stoppedSnapshot);
}

async function releaseRefresh(page: Page) {
	await page.evaluate(async () => {
		await (window as unknown as { __heldOfficeRefresh: HeldRefresh }).__heldOfficeRefresh.release();
		// A render boundary after the deferred response, rather than a sleep or repeated assertion.
		await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
	});
}

for (const unrelated of [false, true]) {
	test(`OFC-11 / BUG-5: ${unrelated ? 'an unrelated push does not suppress a valid refresh' : 'a delayed initial refresh cannot overwrite a newer office status push'}`, async ({ page }) => {
		await visit(page, '/create');
		await page.getByLabel('Organization name').fill('Acme');
		await page.getByRole('button', { name: 'Create organization' }).click();
		await expect(page).toHaveURL(/\/agents$/);
		await holdFirstRefresh(page, unrelated);
		// SPA navigation preserves the patched test-owned API instance.
		await page.getByRole('link', { name: 'Offices', exact: true }).click();
		const office = await page.evaluate(() => (window as unknown as { __heldOfficeRefresh: HeldRefresh }).__heldOfficeRefresh.started);
		const hq = page.getByTestId('office-row').first();
		await expect(hq).toContainText('Online');
		await page.evaluate(({ office, unrelated }) => (window as unknown as { __mockOfficeStatus: (update: object) => void }).__mockOfficeStatus({
			kind: 'office', office: { ...office, organization_id: office.organization_id + (unrelated ? '-other' : ''), status: 'unavailable', status_reason: 'crashed' }
		}), { office, unrelated });
		if (!unrelated) await expect(hq).toContainText('Stopped');
		await releaseRefresh(page);
		expect(await hq.innerText()).toContain('Stopped');
		expect(await hq.innerText()).toContain('It stopped unexpectedly');
		await shot(page, unrelated ? 'office-refresh-unrelated-push' : 'office-refresh-newer-push');
	});
}
