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
const focusedRow = (page: Page) =>
	page.locator('[data-access-row]').filter({ has: page.locator(':focus') });
/** The value column of the owner or administrator fact: what a person reads there, without its link. */
const factValue = (page: Page, term: string) =>
	page.locator('dl').first().locator(`div:has(> dt:text-is("${term}")) dd > span`).first();
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
	// One pill per row: Owner is the pill, Administrator a fact in the meta line (app-shell-ux §6a).
	await expect(row(page, 'Alex Kim').locator('[title]')).toHaveCount(1);
	await expect(row(page, 'Avery Stone')).toContainText('Can sign in');
	await expect(row(page, 'Hana Weiss')).toContainText('No account yet');
	await expect(section(page, 'No account yet').locator('[data-access-row]')).toHaveCount(2);
	await expect(row(page, 'Leo Brandt')).toContainText('No account yet');
	await expect(row(page, 'Alex Kim')).toContainText('Engineering');
	await expect(row(page, 'Avery Stone')).toContainText('Engineering');
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
	// Leo moved to another list: focus goes to his action there, never to the body or his profile link.
	await expect(
		section(page, 'Invited, not signed in yet').getByRole('button', { name: 'Invite to sign in' })
	).toBeFocused();
});

test(
	'ACC-3: a new sign-in link is shown once; removing access asks first and names the effect',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOrganization(page);
		await visit(page, '/settings/access');
		await row(page, 'Avery Stone').getByRole('button', { name: 'New sign-in link' }).click();
		const link = page.getByRole('dialog');
		await expect(link).toContainText('New sign-in link for Avery Stone');
		await expect(link).toContainText('That signs the account out everywhere');
		await link.getByRole('button', { name: 'Done' }).click();
		// Avery stays in the same list, so focus returns to the button that opened the dialog: after Done and after Escape.
		const newLink = row(page, 'Avery Stone').getByRole('button', { name: 'New sign-in link' });
		await expect(newLink).toBeFocused();
		await newLink.click();
		await expect(link).toContainText('New sign-in link for Avery Stone');
		await page.keyboard.press('Escape');
		await expect(link).toHaveCount(0);
		await expect(newLink).toBeFocused();

		await row(page, 'Avery Stone').getByRole('button', { name: 'Remove access…' }).click();
		const confirm = page.getByRole('dialog');
		await expect(confirm).toContainText("Remove Avery's access to Lumen Labs?");
		await expect(confirm).toContainText("can't open Lumen Labs");
		await confirm.getByRole('button', { name: 'Remove access' }).click();
		await expect(confirm).toHaveCount(0);
		await expect(section(page, 'No account yet')).toContainText('Avery Stone');
		await expect(section(page, 'Can sign in')).not.toContainText('Avery Stone');
		// Avery moved to No account yet: focus goes to her action there.
		await expect(
			row(page, 'Avery Stone').getByRole('button', { name: 'Invite to sign in' })
		).toBeFocused();
	}
);

test('ACC-9: cancelling Remove access returns focus to the button that opened it', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/access');
	await row(page, 'Avery Stone').getByRole('button', { name: 'Remove access…' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Cancel' }).click();
	await expect(page.getByRole('dialog')).toHaveCount(0);
	await expect(
		row(page, 'Avery Stone').getByRole('button', { name: 'Remove access…' })
	).toBeFocused();
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
	await expect(page.getByRole('link', { name: 'Open Members' })).toHaveAttribute(
		'href',
		'/teams/members'
	);

	await visit(page, '/settings/access?mock=unavailable');
	await expect(
		page.getByRole('heading', { level: 2, name: "Members and access isn't built yet" })
	).toBeVisible();
	await expect(page.locator('main h2')).toHaveCount(1);
});

test(
	'ACC-10: a person who has left is refused with the reason, drops off the list, and focus moves on',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOrganization(page);
		await page.setViewportSize({ width: 390, height: 700 });
		await visit(page, '/settings/access?mock=gone');
		await row(page, 'Leo Brandt').getByRole('button', { name: 'Invite to sign in' }).click();
		// Said in the list Leo was in, where the person is looking.
		const alert = section(page, 'No account yet').getByRole('alert');
		await expect(alert).toContainText("That person isn't in this organization any more.");
		await expect(row(page, 'Leo Brandt')).toHaveCount(0);
		// The removed row's neighbour takes focus (app-shell-ux §6a), never the body.
		await expect(focusedRow(page)).toHaveAttribute('data-member-id', 'hana');
		await expect(alert).toBeInViewport();
	}
);

test(
	'ACC-6: a refused action says why next to the action and changes nothing',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOrganization(page);
		await page.setViewportSize({ width: 390, height: 700 });
		await visit(page, '/settings/access?mock=refuse');
		const invite = row(page, 'Leo Brandt').getByRole('button', { name: 'Invite to sign in' });
		// By keyboard: Safari doesn't focus a button on click, and the keyboard is where focus must stay put.
		await invite.focus();
		await page.keyboard.press('Enter');
		// The refusal is in Leo's row, under the button, on screen: not somewhere above the lists.
		const alert = row(page, 'Leo Brandt').getByRole('alert');
		await expect(alert).toContainText(
			'Only the owner or the administrator changes sign-in access.'
		);
		await expect(alert).toBeInViewport();
		await expect(page.getByRole('alert')).toHaveCount(1);
		const [button, said] = [await invite.boundingBox(), await alert.boundingBox()];
		expect(said!.y).toBeGreaterThanOrEqual(button!.y + button!.height);
		expect(said!.y - (button!.y + button!.height)).toBeLessThanOrEqual(64);
		await expect(invite).toBeFocused();
		await expect(section(page, 'No account yet')).toContainText('Leo Brandt');
	}
);

test('ACC-15: a refused removal stays inside its confirmation; Cancel returns to the button', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/access?mock=refuse');
	const remove = row(page, 'Avery Stone').getByRole('button', { name: 'Remove access…' });
	await remove.click();
	const confirm = page.getByRole('dialog');
	await confirm.getByRole('button', { name: 'Remove access' }).click();
	await expect(confirm.getByRole('alert')).toContainText(
		'Only the owner or the administrator changes sign-in access.'
	);
	await confirm.getByRole('button', { name: 'Cancel' }).click();
	await expect(confirm).toHaveCount(0);
	await expect(remove).toBeFocused();
	await expect(section(page, 'Can sign in')).toContainText('Avery Stone');
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

	// At 200% text the labels wrap inside the row's column instead of running into its gutter.
	await page.evaluate(() => (document.documentElement.style.fontSize = '32px'));
	const spill = await page.locator('[data-access-row]').evaluateAll((rows) =>
		rows.flatMap((li) => {
			const edge = li.getBoundingClientRect().right - parseFloat(getComputedStyle(li).paddingRight);
			return [...li.querySelectorAll('button')].map((b) => b.getBoundingClientRect().right - edge);
		})
	);
	expect(Math.max(...spill)).toBeLessThanOrEqual(0.5);
	await noSidewaysScroll(page);
});

test('ACC-8: after the page opens, its heading takes focus', async ({ page }) => {
	await openOrganization(page);
	await visit(page, '/settings/access');
	await expect(page.locator('#access-heading')).toBeFocused();
	// Headings focused by the app show no control ring (app-shell-ux §6a).
	const outline = await page
		.locator('#access-heading')
		.evaluate((el) => getComputedStyle(el).outlineStyle);
	expect(outline).toBe('none');
});

test('ACC-11: the owner row says why it has no remove action; the reference covers the administrator', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/access');
	await expect(row(page, 'Alex Kim')).toContainText('Hand over ownership first.');
	await expect(
		section(page, 'Who can do what').locator('div:has(> dt:text-is("Administrator"))')
	).toContainText('removes access');
});

test('ACC-12: at 1440 px the owner row keeps its action in the same column as the other rows', async ({
	page
}) => {
	await openOrganization(page);
	await page.setViewportSize({ width: 1440, height: 900 });
	await visit(page, '/settings/access');
	const owner = await row(page, 'Alex Kim')
		.getByRole('button', { name: 'New sign-in link' })
		.boundingBox();
	const other = await row(page, 'Avery Stone')
		.getByRole('button', { name: 'Remove access…' })
		.boundingBox();
	expect(owner && other).toBeTruthy();
	// Both actions end at the same right edge: the column is shared, not pushed by the owner's note.
	expect(Math.abs(owner!.x + owner!.width - (other!.x + other!.width))).toBeLessThanOrEqual(1);
});

test('ACC-13: the owner and administrator stay readable at a laptop and a phone width', async ({
	page
}) => {
	await openOrganization(page);
	for (const width of [1024, 320]) {
		await page.setViewportSize({ width, height: 800 });
		await visit(page, '/settings/access');
		await expect(factValue(page, 'Owner')).toContainText('Alex Kim');
		// The names keep a real text column (app-shell-ux §2: none narrower than 200 px), with the hand-over below.
		for (const term of ['Owner', 'Administrator']) {
			const box = await factValue(page, term).boundingBox();
			expect(box?.width ?? 0, `${term} at ${width} px`).toBeGreaterThanOrEqual(200);
		}
		await noSidewaysScroll(page);
	}
});

test('ACC-14: at 200% text on a desktop the rows stack, and every action stays whole on screen', async ({
	page
}) => {
	await openOrganization(page);
	await page.setViewportSize({ width: 1440, height: 900 });
	await visit(page, '/settings/access');
	await page.evaluate(() => (document.documentElement.style.fontSize = '32px'));
	const avery = row(page, 'Avery Stone');
	const name = await avery.getByRole('link', { name: 'Avery Stone' }).boundingBox();
	expect(name?.width ?? 0).toBeGreaterThan(100);
	expect(name?.height ?? 999).toBeLessThanOrEqual(2 * 48);
	const edge = await page.evaluate(() => document.documentElement.clientWidth);
	for (const button of await page.locator('[data-access-row] button').all()) {
		const box = await button.boundingBox();
		expect(box!.x + box!.width).toBeLessThanOrEqual(edge);
	}
	for (const link of await page.locator('dl').first().getByRole('link').all()) {
		const box = await link.boundingBox();
		expect(box!.x + box!.width).toBeLessThanOrEqual(edge);
	}
});

test('ACC-16: a deactivated person is listed apart, with no sign-in action', async ({ page }) => {
	await openOrganization(page);
	await visit(page, '/settings/access?mock=deactivated');
	const deactivated = section(page, 'Deactivated');
	await expect(deactivated).toContainText('Hana Weiss');
	await expect(row(page, 'Hana Weiss')).toContainText('Deactivated · no access');
	await expect(row(page, 'Hana Weiss').getByRole('button')).toHaveCount(0);
	await expect(section(page, 'No account yet')).not.toContainText('Hana Weiss');
});

test('ACC-17: this page and the member profile tell the same story in one session', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/access');
	await row(page, 'Avery Stone').getByRole('button', { name: 'Remove access…' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Remove access' }).click();
	await expect(section(page, 'No account yet')).toContainText('Avery Stone');
	// The name opens her profile (a client navigation: the same session), which agrees.
	await row(page, 'Avery Stone').getByRole('link', { name: 'Avery Stone' }).click();
	await expect(page).toHaveURL(/\/members\/avery$/);
	const signIn = page.locator('section').filter({
		has: page.getByRole('heading', { level: 3, name: 'Sign-in access' })
	});
	await expect(signIn).toContainText('No account yet');
	// Invited from the profile: this page lists her as invited.
	await signIn.getByRole('button', { name: 'Invite to sign in' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Done' }).click();
	await page.goBack();
	await expect(page).toHaveURL(/\/settings\/access$/);
	await expect(section(page, 'Invited, not signed in yet')).toContainText('Avery Stone');
});

test('ACC-18: a refusal clears on the next interaction; another person waits while a link is made', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/access?mock=refuse');
	const invite = row(page, 'Leo Brandt').getByRole('button', { name: 'Invite to sign in' });
	await invite.focus();
	await page.keyboard.press('Enter');
	await expect(row(page, 'Leo Brandt').getByRole('alert')).toBeVisible();
	await page.keyboard.press('Shift+Tab');
	await expect(page.getByRole('alert')).toHaveCount(0);

	// While Leo's link is being made, Hana's actions wait and say why; pressing one points at Leo's.
	await visit(page, '/settings/access?mock=slow');
	await row(page, 'Leo Brandt').getByRole('button', { name: 'Invite to sign in' }).click();
	const leo = row(page, 'Leo Brandt').getByRole('button', { name: 'Inviting…' });
	await expect(leo).toBeVisible();
	await expect(row(page, 'Hana Weiss')).toContainText("Wait for Leo Brandt's link first.");
	const hana = row(page, 'Hana Weiss').getByRole('button', { name: 'Invite to sign in' });
	await expect(hana).toHaveAttribute('aria-disabled', 'true');
	await hana.focus();
	await page.keyboard.press('Enter');
	await expect(leo).toBeFocused();
	await expect(page.getByRole('dialog')).toContainText('Invite Leo Brandt to sign in', {
		timeout: 20_000
	});
});
