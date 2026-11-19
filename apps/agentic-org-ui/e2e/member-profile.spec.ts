import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { signInAsAdmin } from './web-session';

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
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Lumen Labs');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).not.toHaveURL(/\/create$/);
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

const signInAccess = (page: Page) =>
	page.locator('section').filter({
		has: page.getByRole('heading', { level: 3, name: 'Sign-in access' })
	});

test(
	'MP-5: after a new sign-in link, Done and Escape return focus to the button that opened it',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOrganization(page);
		await visit(page, '/members/avery');
		const newLink = signInAccess(page).getByRole('button', { name: 'New sign-in link' });
		const dialog = page.getByRole('dialog');
		await newLink.click();
		await expect(dialog).toContainText('New sign-in link for Avery Stone');
		await dialog.getByRole('button', { name: 'Done' }).click();
		await expect(dialog).toHaveCount(0);
		await expect(newLink).toBeFocused();
		await newLink.click();
		await expect(dialog).toContainText('New sign-in link for Avery Stone');
		await page.keyboard.press('Escape');
		await expect(dialog).toHaveCount(0);
		await expect(newLink).toBeFocused();
	}
);

test(
	'MP-6: after Remove access, focus goes to Invite to sign in, the button that replaced it',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOrganization(page);
		await visit(page, '/members/avery');
		await signInAccess(page).getByRole('button', { name: 'Remove access…' }).click();
		const confirm = page.getByRole('dialog');
		await expect(confirm).toContainText("Remove Avery's access to Lumen Labs?");
		await confirm.getByRole('button', { name: 'Remove access' }).click();
		await expect(confirm).toHaveCount(0);
		await expect(signInAccess(page)).toContainText('No account yet');
		await expect(
			signInAccess(page).getByRole('button', { name: 'Invite to sign in' })
		).toBeFocused();
	}
);

test(
	'MP-7: after Invite to sign in, Done returns focus to the button, and the invitation is said',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOrganization(page);
		await visit(page, '/members/leo');
		const invite = signInAccess(page).getByRole('button', { name: 'Invite to sign in' });
		await invite.click();
		const dialog = page.getByRole('dialog');
		await expect(dialog).toContainText('Invite Leo Brandt to sign in');
		await dialog.getByRole('button', { name: 'Done' }).click();
		await expect(dialog).toHaveCount(0);
		await expect(signInAccess(page)).toContainText('Invited: the link works until');
		await expect(invite).toBeFocused();
	}
);
