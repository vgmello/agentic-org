import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { signInAsAdmin } from './web-session';
import { enterMockedOrganization } from './mock-organization';

// The member profile against the mock members API (our-org-in-the-app §5, §8; agent-run-modes-ux RUN-4).
async function openOrganization(page: Page) {
	await signInAsAdmin(page, [
		{
			id: 'org-member-profile',
			name: 'Lumen Labs',
			created_at: '2026-11-18T00:00:00Z',
			recovery: 'acknowledged'
		}
	]);
	await enterMockedOrganization(page, 'Lumen Labs');
}

const header = (page: Page) => page.locator('main header').first();

test('MP-1: a recorded home team without a team membership is still the home team', async ({
	page
}) => {
	await openOrganization(page);
	for (const who of ['you', 'avery']) {
		await visit(page, `/members/${who}`);
		await expect(header(page)).toContainText('Home team Engineering');
		await expect(header(page)).not.toContainText('No home team on record');
		await page.getByRole('button', { name: /^More/ }).click();
		await expect(page.getByRole('menuitem', { name: 'Move home team…' })).toBeVisible();
		await page.getByRole('menuitem', { name: 'Move home team…' }).click();
		await expect(page.getByRole('dialog')).toBeVisible();
		await page.getByRole('button', { name: 'Cancel' }).click();
	}
});

test('MP-2: Move home team records the destination and blocks teams they are already in', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/members/mira');
	await expect(header(page)).toContainText('Home team Web');
	await page.getByRole('button', { name: /^More/ }).click();
	await page.getByRole('menuitem', { name: 'Move home team…' }).click();
	const dialog = page.getByRole('dialog');
	await expect(dialog.getByRole('radio', { name: /^Platform/ })).toBeDisabled();
	await expect(dialog).toContainText('Mira Castillo is already in this team');
	await expect(dialog.getByRole('radio', { name: /^Web/ })).toBeDisabled();
	await dialog.getByRole('radio', { name: /^Engineering/ }).check();
	await dialog.getByRole('button', { name: 'Move Mira Castillo' }).click();
	await expect(dialog).toBeHidden();
	await expect(header(page)).toContainText('Home team Engineering');
	// The move is kept where the org mock keeps the membership: a reload shows the same home team.
	await page.reload();
	await expect(header(page)).toContainText('Home team Engineering');

	// Someone recorded without a membership moves too, and stays moved after a reload.
	await visit(page, '/members/avery');
	await page.getByRole('button', { name: /^More/ }).click();
	await page.getByRole('menuitem', { name: 'Move home team…' }).click();
	await dialog.getByRole('radio', { name: /^Design/ }).check();
	await dialog.getByRole('button', { name: 'Move Avery Stone' }).click();
	await expect(dialog).toBeHidden();
	await expect(header(page)).toContainText('Home team Design');
	await page.reload();
	await expect(header(page)).toContainText('Home team Design');
});

test('MP-4: a failed check while a run waits keeps the page and what was typed, and checks again', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/members/kai?mock=refresh_fails_once');
	await page.getByRole('button', { name: 'Run now' }).first().click();
	const dialog = page.getByRole('dialog');
	await dialog.getByRole('button', { name: 'Run now' }).click();
	await expect(dialog).toBeHidden();
	// Start typing in Edit while the page checks in the background.
	await page.getByRole('button', { name: /^More/ }).click();
	await page.getByRole('menuitem', { name: 'Edit…' }).click();
	await dialog.getByLabel(/^Focus/).fill('retries');
	await expect(page.getByText("Couldn't refresh; checking again.")).toBeVisible({
		timeout: 10_000
	});
	await expect(dialog).toBeVisible();
	await expect(dialog.getByLabel(/^Focus/)).toHaveValue('retries');
	await expect(page.getByText("Couldn't load this member")).toHaveCount(0);
	await dialog.getByRole('button', { name: 'Cancel' }).click();
	// It keeps checking and catches up when the office starts the turn.
	await expect(header(page).locator('[data-presence="working"]')).toBeVisible({ timeout: 20_000 });
	await expect(page.getByText("Couldn't refresh; checking again.")).toHaveCount(0);
});

test('MP-3: Run now says requested until the office starts the turn, then the header, Health and Runs agree', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/members/kai');
	await page.getByRole('button', { name: 'Run now' }).first().click();
	const dialog = page.getByRole('dialog');
	await dialog.getByRole('button', { name: 'Run now' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByText('Run requested for Kai Lindqvist.')).toBeVisible();
	await expect(header(page)).toContainText('Available · run requested, waiting for the office');
	const main = page.locator('main');
	await expect(main).toContainText("Run requested · the office hasn't started the turn yet");
	await expect(main).toContainText('Requested · waiting for the office to start it');
	await expect(main).not.toContainText('Accepted · started');
	// Asking again while it waits is Busy, in words that don't call it running.
	await page.getByRole('button', { name: 'Run now' }).first().click();
	await expect(dialog).toContainText('already has a run waiting for the office to start it');
	await dialog.getByRole('button', { name: 'Cancel' }).click();
	// The office starts it: the page reads the profile again by itself, with no reload.
	await expect(header(page).locator('[data-presence="working"]')).toBeVisible({ timeout: 20_000 });
	await expect(main).toContainText('Working · the office started the turn');
	await expect(main).toContainText('Running · started');
});
