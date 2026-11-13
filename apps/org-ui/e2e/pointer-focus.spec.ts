import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { setupFocus } from './focus-primitives';

// WebKit on macOS does not focus a button on a mouse click: the activeElement stays on <body>. Chromium and Firefox do,
// so every spec first removes that focus (a capture-phase mousedown preventDefault on buttons), which makes them red on
// the unfixed code in every engine. On real WebKit (PW_WEBKIT=1 --project=webkit) it changes nothing.
async function clickDoesNotFocus(page: Page) {
	await page.addInitScript(() => {
		document.addEventListener('mousedown', (e) => (e.target as Element | null)?.closest?.('button, a, summary') && e.preventDefault(), true);
	});
}
async function create(page: Page) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
}
/** A click on empty page: while a menu is open the body ignores pointers, so it lands on the page itself. */
const blank = async (page: Page) => {
	const size = page.viewportSize()!;
	await page.mouse.click(size.width - 6, size.height - 6);
};

test.describe('BUG-158: a menu opened by pointer gives focus back to its trigger after a click outside it', () => {
	test('BUG-158 BUG-70: the org chart View menu', { tag: '@webkit' }, async ({ page }) => {
		await clickDoesNotFocus(page);
		await create(page);
		await visit(page, '/org-chart');
		const view = page.getByRole('button', { name: 'View', exact: true }), menu = page.getByRole('menu');
		await view.click();
		await expect(menu).toBeVisible();
		await blank(page);
		await expect(menu).toHaveCount(0);
		await expect(view).toBeFocused();
	});

	test('BUG-158 BUG-70: the primitive: a click outside restores focus; Escape and choosing an item behave as before', { tag: '@webkit' }, async ({ page }) => {
		await clickDoesNotFocus(page);
		const fixture = await setupFocus(page, 960, 1);
		const trigger = fixture.getByRole('button', { name: 'Commands', exact: true }), menu = page.getByRole('menu');
		await trigger.click();
		await expect(menu).toBeVisible();
		await blank(page);
		await expect(menu).toHaveCount(0);
		await expect(trigger).toBeFocused();
		// Escape (pointer open, keyboard close) and an item still end on the trigger.
		await trigger.click();
		await expect(menu).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(menu).toHaveCount(0);
		await expect(trigger).toBeFocused();
		await trigger.click();
		await page.getByRole('menuitem', { name: 'Rename item', exact: true }).click();
		await expect(fixture.getByLabel('Selection')).toHaveText('Renamed');
		await expect(trigger).toBeFocused();
	});
});

test.describe('BUG-166: Retry on the Offices page', () => {
	test('BUG-166 BUG-70: a pointer Retry keeps focus while it works, and a count Retry that worked hands focus to the page heading', { tag: '@webkit' }, async ({ page }) => {
		await clickDoesNotFocus(page);
		await create(page);
		await visit(page, '/offices?org=fails_once');
		await expect(page.getByTestId('count-error')).toBeVisible();
		const retry = page.getByTestId('count-error').getByRole('button', { name: 'Retry' });
		await retry.click();
		await expect(page.getByTestId('count-error')).toHaveCount(0);
		await expect(page.getByRole('heading', { level: 1 })).toBeFocused();
	});

	// The second counting call is held until the test lets it go, so what Retry does while it works can be seen.
	for (const outcome of ['arrive', 'fail again'] as const) {
		test(`BUG-166 BUG-70: a count Retry keeps focus while it works; when the counts ${outcome} focus ${outcome === 'arrive' ? 'goes to the page heading' : 'stays on Retry'}`, { tag: '@webkit' }, async ({ page }) => {
			await clickDoesNotFocus(page);
			await create(page);
			await visit(page, '/offices?org=fails_once');
			await expect(page.getByTestId('count-error')).toBeVisible();
			await page.evaluate(async (fail) => {
				const modulePath = '/e2e/fakes/org-api.ts';
				const { orgApi } = await import(modulePath) as typeof import('../src/lib/org/api') & { orgApi: import('../src/lib/org/api').OrgApi };
				const view = orgApi.view;
				let held = false, release!: () => void;
				const gate = new Promise<void>((resolve) => { release = resolve; });
				(window as unknown as { __releaseCounts: () => void }).__releaseCounts = release;
				orgApi.view = async (id) => {
					if (!held) {
						held = true;
						await gate;
						if (fail) throw new Error('Still unreadable.');
					}
					return view(id);
				};
			}, outcome === 'fail again');
			const retry = page.getByTestId('count-error').getByRole('button', { name: /Retry|Retrying/ });
			await retry.click();
			await expect(retry).toHaveAttribute('aria-busy', 'true');
			await expect(retry).toBeFocused();
			await page.evaluate(() => (window as unknown as { __releaseCounts: () => void }).__releaseCounts());
			if (outcome === 'arrive') {
				await expect(page.getByTestId('count-error')).toHaveCount(0);
				await expect(page.getByRole('heading', { level: 1 })).toBeFocused();
			} else {
				await expect(retry).toHaveAttribute('aria-busy', 'false');
				await expect(page.getByTestId('count-error')).toContainText('Still unreadable.');
				await expect(retry).toBeFocused();
				await expect(page.getByRole('heading', { level: 1 })).not.toBeFocused();
			}
		});
	}

	test('BUG-166 BUG-70: a pointer Retry on an unreachable office is focused while it works', { tag: '@webkit' }, async ({ page }) => {
		await clickDoesNotFocus(page);
		await create(page);
		await visit(page, '/offices?mock=unreachable');
		const retry = page.getByTestId('office-row').first().getByTestId('retry-office');
		await expect(retry).toHaveAccessibleName('Retry reaching HQ');
		await retry.click();
		await expect(retry).toHaveText('Retrying…');
		await expect(retry).toBeFocused();
		await expect(retry).toHaveText('Retry', { timeout: 15_000 });
		await expect(retry).toBeFocused();
	});
});

test.describe('BUG-168: removing a project\'s last team', () => {
	const beacon = (page: Page) => page.locator('[data-testid=project-row][data-project=beacon]');
	async function toLastTeam(page: Page) {
		await clickDoesNotFocus(page);
		await create(page);
		await visit(page, '/projects');
		await beacon(page).getByRole('button', { name: 'Remove Beacon from Web' }).click();
		await expect(page.getByText('Removed Beacon from Web.')).toBeVisible();
		const remove = beacon(page).getByRole('button', { name: 'Remove Beacon from Design' });
		// The first removal put focus on this very button; a person who clicks it in WebKit has not focused it.
		await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
		await remove.click();
		const dialog = page.getByRole('dialog', { name: 'Remove Beacon from Design?' });
		await expect(dialog).toBeVisible();
		return { remove, dialog };
	}

	test('BUG-168 BUG-70: Cancel gives focus back to that Remove button', { tag: '@webkit' }, async ({ page }) => {
		const { remove, dialog } = await toLastTeam(page);
		await dialog.getByRole('button', { name: 'Cancel' }).click();
		await expect(dialog).toHaveCount(0);
		await expect(remove).toBeFocused();
	});

	test('BUG-168 BUG-70: Escape gives focus back to that Remove button', { tag: '@webkit' }, async ({ page }) => {
		const { remove, dialog } = await toLastTeam(page);
		await page.keyboard.press('Escape');
		await expect(dialog).toHaveCount(0);
		await expect(remove).toBeFocused();
	});

	test('BUG-168 BUG-70: Confirm waits for the reload and focus goes to that row\'s Assign team', { tag: '@webkit' }, async ({ page }) => {
		const { dialog } = await toLastTeam(page);
		await dialog.getByRole('button', { name: 'Remove', exact: true }).click();
		await expect(dialog).toHaveCount(0);
		await expect(beacon(page).getByTestId('no-team')).toBeVisible();
		await expect(beacon(page).getByRole('button', { name: 'Assign a team to Beacon' })).toBeFocused();
	});
});
