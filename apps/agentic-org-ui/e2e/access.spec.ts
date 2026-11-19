import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { signInAsAdmin } from './web-session';

// Organization settings › Members and access against the mock access API (server-installation-and-access-ux §4a,
// offices-and-teams-ux §5b, §7.1, §7.4). Mock: e2e/fakes/modules/access.ts; `?mock=` switches are listed there.
async function openOrganization(page: Page) {
	await signInAsAdmin(page, [
		{
			id: 'org-access',
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

const section = (page: Page, title: string) =>
	page.locator('section').filter({ has: page.getByRole('heading', { level: 3, name: title }) });
const row = (page: Page, name: string) =>
	page.locator('[data-access-row]').filter({ hasText: name });
const noSidewaysScroll = async (page: Page) => {
	const overflow = await page.evaluate(
		() => document.documentElement.scrollWidth - document.documentElement.clientWidth
	);
	expect(overflow).toBeLessThanOrEqual(0);
};

test('ACC-1: the owner and administrator are shown, and people are grouped by sign-in state', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/access');
	await expect(page).toHaveTitle('Members and access · Organization settings · Agentic Org');
	await expect(page.getByRole('heading', { level: 2, name: 'Members and access' })).toBeVisible();

	const facts = page.locator('dl').first();
	await expect(facts).toContainText('Alex Kim');
	await expect(facts).toContainText('alex');

	await expect(row(page, 'Alex Kim')).toContainText('Owner');
	await expect(row(page, 'Alex Kim')).toContainText('Administrator');
	await expect(row(page, 'Avery Stone')).toContainText('Can sign in');
	await expect(row(page, 'Hana Weiss')).toContainText('Can sign in');
	await expect(section(page, 'No account yet').locator('[data-access-row]')).toHaveCount(1);
	await expect(row(page, 'Leo Brandt')).toContainText('No account yet');
	// The owner's own row names why it has no remove action.
	await expect(row(page, 'Alex Kim')).toContainText('Hand over ownership first.');
	await expect(row(page, 'Alex Kim').getByRole('button', { name: 'Remove access…' })).toHaveCount(
		0
	);
	// Agents never appear: they never sign in.
	await expect(page.getByText('Sam Okafor')).toHaveCount(0);
	await noSidewaysScroll(page);
});

test('ACC-2: inviting a person shows the one-time link once, then lists them as invited', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/access');
	await row(page, 'Leo Brandt').getByRole('button', { name: 'Invite to sign in' }).click();
	const dialog = page.getByRole('dialog');
	await expect(dialog).toContainText('Invite Leo Brandt to sign in');
	await expect(dialog).toContainText('It works once and expires in 24 hours');
	await expect(dialog.getByRole('textbox', { name: 'Registration link' })).toHaveValue(
		/127\.0\.0\.1:7420\/register#/
	);
	await dialog.getByRole('button', { name: 'Done' }).click();
	await expect(dialog).toHaveCount(0);
	await expect(section(page, 'Invited, not signed in yet')).toContainText('Leo Brandt');
	await expect(row(page, 'Leo Brandt')).toContainText('The link works until');
});

test('ACC-3: a new sign-in link is shown once; removing access asks first and names the effect', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/access');
	await row(page, 'Avery Stone').getByRole('button', { name: 'New sign-in link' }).click();
	const link = page.getByRole('dialog');
	await expect(link).toContainText('New sign-in link for Avery Stone');
	await expect(link).toContainText('That signs the account out everywhere');
	await link.getByRole('button', { name: 'Done' }).click();

	await row(page, 'Avery Stone').getByRole('button', { name: 'Remove access…' }).click();
	const confirm = page.getByRole('dialog');
	await expect(confirm).toContainText("Remove Avery's access to Lumen Labs?");
	await expect(confirm).toContainText("can't open Lumen Labs");
	await confirm.getByRole('button', { name: 'Remove access' }).click();
	await expect(confirm).toHaveCount(0);
	await expect(section(page, 'No account yet')).toContainText('Avery Stone');
	await expect(section(page, 'Can sign in')).not.toContainText('Avery Stone');
});

test('ACC-4: a viewer without manage rights sees the lists and no actions, with the reason', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/access?mock=viewer');
	await expect(row(page, 'Avery Stone')).toBeVisible();
	await expect(
		page.getByRole('button', { name: /Invite to sign in|New sign-in link|Remove access/ })
	).toHaveCount(0);
	await expect(
		page.getByText('Only the owner or the administrator changes sign-in access.')
	).toBeVisible();
});

test('ACC-5: loading, error with Retry, empty and unavailable are each shown in words', async ({
	page
}) => {
	await openOrganization(page);

	await visit(page, '/settings/access?mock=slow');
	await expect(page.getByText('Loading members and access')).toBeVisible();

	await visit(page, '/settings/access?mock=fails');
	await expect(page.getByRole('heading', { name: "Couldn't load who can sign in" })).toBeVisible();
	await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();

	await visit(page, '/settings/access?mock=empty');
	await expect(page.getByRole('heading', { name: 'No people yet' })).toBeVisible();

	await visit(page, '/settings/access?mock=unavailable');
	await expect(page.getByText("Members and access isn't built yet")).toBeVisible();
});

test('ACC-6: a refused action says why inline and changes nothing', async ({ page }) => {
	await openOrganization(page);
	await visit(page, '/settings/access?mock=refuse');
	await row(page, 'Leo Brandt').getByRole('button', { name: 'Invite to sign in' }).click();
	await expect(row(page, 'Leo Brandt').getByRole('alert')).toContainText(
		'Only the owner or the administrator changes sign-in access.'
	);
	await expect(section(page, 'No account yet')).toContainText('Leo Brandt');
});

test('ACC-7: at 320 px the page has no sideways scroll and its actions are 44 px targets', async ({
	page
}) => {
	await openOrganization(page);
	await page.setViewportSize({ width: 320, height: 800 });
	await visit(page, '/settings/access?mock=invited');
	await expect(row(page, 'Leo Brandt')).toBeVisible();
	await noSidewaysScroll(page);
	const button = row(page, 'Leo Brandt').getByRole('button', { name: 'Invite to sign in' });
	const box = await button.boundingBox();
	expect(box?.height ?? 0).toBeGreaterThanOrEqual(44);
});

test('ACC-8: after the page opens, its heading takes focus', async ({ page }) => {
	await openOrganization(page);
	await visit(page, '/settings/access');
	await expect(page.locator('#access-heading')).toBeFocused();
});
