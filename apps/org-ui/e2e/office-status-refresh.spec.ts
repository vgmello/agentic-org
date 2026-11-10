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

type PeekPair = {
	started: Promise<void>[];
	release: (index: number) => Promise<void>;
};

async function holdOverlappingRefreshes(page: Page) {
	await page.evaluate(async () => {
		const modulePath = '/src/lib/office/api.ts';
		const { officeApi } = await import(modulePath);
		const peek = officeApi.peekOrganizations.bind(officeApi);
		const connection = (await officeApi.connections()).at(-1)!;
		const starts: (() => void)[] = [];
		const releases: (() => void)[] = [];
		const started = [0, 1].map((index) => new Promise<void>((resolve) => (starts[index] = resolve)));
		const gates = [0, 1].map((index) => new Promise<void>((resolve) => (releases[index] = resolve)));
		const pending: Promise<unknown>[] = [];
		let reads = 0;
		officeApi.peekOrganizations = () => {
			const index = reads++;
			if (index > 1) return peek();
			pending[index] = (async () => {
				const organizations = await peek();
				const office = organizations.find((organization: { id: string }) => organization.id === connection.organization.id).offices.find((office: OfficeInfo) => office.id === connection.office.id);
				office.status = index === 0 ? 'unavailable' : 'active';
				office.status_reason = index === 0 ? 'crashed' : undefined;
				starts[index]();
				await gates[index];
				return organizations;
			})();
			return pending[index];
		};
		// The host admits the restart; its authoritative result arrives in the second peek, with no push.
		officeApi.restartOffice = async () => undefined;
		(window as unknown as { __officePeekPair: PeekPair }).__officePeekPair = {
			started,
			release: async (index) => { releases[index](); await pending[index]; }
		};
	});
}

async function releasePeek(page: Page, index: number) {
	await page.evaluate(async (index) => {
		await (window as unknown as { __officePeekPair: PeekPair }).__officePeekPair.release(index);
		await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
	}, index);
}

test('OFC-11 / BUG-5 LOW: a delayed probe refresh cannot overwrite a newer restart refresh without a push', async ({ page }) => {
	await visit(page, '/create?office=crashed');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await holdOverlappingRefreshes(page);
	await page.getByRole('link', { name: 'Offices', exact: true }).click();
	await page.evaluate(() => (window as unknown as { __officePeekPair: PeekPair }).__officePeekPair.started[0]);
	const hq = page.getByTestId('office-row').first();
	await expect(hq).toContainText('Stopped');
	await shot(page, 'office-refresh-stopped');
	await hq.getByRole('button').first().click();
	await page.getByRole('dialog').getByTestId('restart-office').click();
	await page.evaluate(() => (window as unknown as { __officePeekPair: PeekPair }).__officePeekPair.started[1]);
	await expect(page.getByRole('dialog').getByTestId('restart-office')).toHaveText('Restarting…');
	await shot(page, 'office-refresh-newer-pending', { settle: false });
	await releasePeek(page, 1);
	await expect(hq).toContainText('Online');
	await releasePeek(page, 0);
	expect(await hq.innerText()).toContain('Online');
	expect(await hq.innerText()).not.toContain('Stopped');
	await shot(page, 'office-refresh-newer-restart');
});
