// Hire an agent and Hire requests (our-org-in-the-app §7, §7.1; STAFF, APPR, HRL), against the dev mock. The mock's
// switches on the address select the states: ?hire=refused|full|fails|givesup, ?hr=not_running|stops|late,
// ?mock=slow|fails|empty (e2e/fakes/modules/employees.ts).
import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';

async function open(page: Page, url: string) {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Hire checks');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/teams\/members$/);
	await visit(page, url);
}

const stepHeading = (page: Page) => page.locator('#hire-step-heading');

test(
	'HIRE: Describe, the HR lead proposes, Review, Hiring in words, Done',
	{ tag: '@webkit' },
	async ({ page }) => {
		await open(page, '/teams/hire?team=web');
		await expect(page).toHaveTitle('Hire an agent · Teams · Agentic Org');
		await expect(stepHeading(page)).toHaveText('Describe the hire');
		await expect(page.getByLabel('Home team')).toHaveValue('web');
		const next = page.getByRole('button', { name: 'Next', exact: true });
		await expect(next).toBeDisabled();
		await expect(page.getByText('Describe what you need to continue')).toBeVisible();
		await page.getByLabel('What do you need?').fill('Someone to test checkout end to end');
		await next.click();
		await expect(page.getByText('HR lead is reading…')).toBeVisible();
		await expect(stepHeading(page)).toHaveText("Check the HR lead's proposal");
		await expect(stepHeading(page)).toBeFocused();
		// The suggested manager is shown, not applied, until the hirer chooses it.
		await expect(page.getByLabel('Reports to')).toHaveValue('');
		await page.getByRole('button', { name: 'Use Noor Haddad' }).click();
		await expect(page.getByLabel('Reports to')).toHaveValue('noor');
		await page.getByRole('button', { name: 'Next', exact: true }).click();
		await expect(stepHeading(page)).toHaveText('Review what will be created');
		await expect(
			page.getByText('Marlowe can be reached by everyone in Hire checks.')
		).toBeVisible();
		await expect(page.getByText(/cost/i)).toHaveCount(0);
		await page.getByRole('button', { name: 'Hire Marlowe' }).click();
		await expect(stepHeading(page)).toHaveText('Hiring Marlowe');
		await expect(page.getByText("Created Marlowe's record")).toBeVisible();
		await expect(stepHeading(page)).toHaveText('Marlowe is hired and in Web.', { timeout: 10_000 });
		await expect(page.getByRole('link', { name: 'View profile' })).toBeVisible();
		await expect(page.getByRole('link', { name: 'View session' })).toHaveCount(0);
		await page.getByRole('link', { name: 'View profile' }).click();
		await expect(page).toHaveURL(/\/members\//);
		await expect(page.getByRole('heading', { name: 'Marlowe', exact: true })).toBeVisible();
	}
);

test('HIRE: someone who may hire nowhere sees the refusal instead of the form', async ({
	page
}) => {
	await open(page, '/teams/hire?hire=refused');
	await expect(page.getByText("You don't have access to hire. Ask your team lead.")).toBeVisible();
	await expect(page.getByLabel('What do you need?')).toHaveCount(0);
});

test('HIRE: a full team is refused with its limit named', async ({ page }) => {
	await open(page, '/teams/hire?hire=full&team=web');
	await page.getByLabel('What do you need?').fill('Another web engineer');
	await expect(
		page.getByText(
			"Web already has 6 members, its limit. Raise the limit on the team's page first."
		)
	).toBeVisible();
	await expect(page.getByRole('button', { name: 'Next', exact: true })).toBeDisabled();
	await expect(page.getByLabel('Home team')).toHaveAttribute('aria-invalid', 'true');
});

test('HIRE: when the HR lead cannot run, the hirer fills in the details, kept across Back', async ({
	page
}) => {
	await open(page, '/teams/hire?hr=not_running');
	await expect(page.getByLabel('What do you need?')).toBeDisabled();
	await expect(
		page.getByText("The HR lead can't run on this server yet.", { exact: false })
	).toBeVisible();
	await page.getByRole('button', { name: 'Fill in the details yourself' }).click();
	await expect(stepHeading(page)).toHaveText('Fill in the details');
	await expect(page.getByText('Your choice needed')).toHaveCount(2);
	await expect(page.locator('#hire-name')).toHaveValue('');
	await expect(page.locator('#hire-handle')).toHaveValue('');
	await expect(page.locator('#hire-office')).toHaveValue('');
	await expect(page.getByRole('button', { name: 'Next', exact: true })).toBeDisabled();
	await expect(
		page.getByText('Choose a name, handle, execution profile, model and office to continue')
	).toBeVisible();
	await page.locator('#hire-name').fill('Juniper');
	await page.getByRole('button', { name: 'Back', exact: true }).click();
	await page.getByRole('button', { name: 'Fill in the details yourself' }).click();
	await expect(page.locator('#hire-name')).toHaveValue('Juniper');
	await expect(page.locator('#hire-handle')).toHaveValue('juniper');
});

test('HIRE: the HR lead stops, the heading says so, and Try again asks again', async ({ page }) => {
	await open(page, '/teams/hire?hr=stops');
	await page.getByLabel('What do you need?').fill('A designer for the account pages');
	await page.getByRole('button', { name: 'Next', exact: true }).click();
	await expect(stepHeading(page)).toHaveText('The HR lead stopped', { timeout: 10_000 });
	await expect(stepHeading(page)).toBeFocused();
	await expect(page.getByText(/^The HR lead stopped: /)).toBeVisible();
	await expect(page.getByText('A designer for the account pages')).toBeVisible();
	await page.getByRole('button', { name: 'Try again' }).click();
	await expect(stepHeading(page)).toHaveText("Check the HR lead's proposal", { timeout: 10_000 });
});

test('HIRE: a late HR answer never replaces a form the hirer fills in', async ({ page }) => {
	// ?hr=late: "still starting" shows at once, each read takes 2.5 s, and the HR lead answers 3.5 s after it was
	// asked, so a read that started before Fill in the details yourself comes back with the answer afterwards.
	await open(page, '/teams/hire?hr=late');
	await page.getByLabel('What do you need?').fill('A designer for the account pages');
	await page.getByRole('button', { name: 'Next', exact: true }).click();
	await expect(page.getByText('HR lead is still starting…')).toBeVisible();
	await page.waitForTimeout(1500);
	await page.getByRole('button', { name: 'Fill in the details yourself' }).click();
	await page.locator('#hire-name').fill('Juniper');
	await page.waitForTimeout(4500);
	await expect(stepHeading(page)).toHaveText('Fill in the details');
	await expect(page.locator('#hire-name')).toHaveValue('Juniper');
	await expect(page.locator('#hire-title')).toHaveValue('');
});

test('HIRE: Back from the hand-filled form and Next on an unchanged Describe return to the form', async ({
	page
}) => {
	await open(page, '/teams/hire?hr=late');
	await page.getByLabel('What do you need?').fill('A designer for the account pages');
	await page.getByRole('button', { name: 'Next', exact: true }).click();
	await page.getByRole('button', { name: 'Fill in the details yourself' }).click();
	await page.locator('#hire-name').fill('Juniper');
	await page.getByRole('button', { name: 'Back', exact: true }).click();
	await expect(stepHeading(page)).toHaveText('Describe the hire');
	await page.getByRole('button', { name: 'Next', exact: true }).click();
	await expect(stepHeading(page)).toHaveText('Fill in the details');
	await expect(page.locator('#hire-name')).toHaveValue('Juniper');
	await page.waitForTimeout(4500);
	await expect(stepHeading(page)).toHaveText('Fill in the details');
	await expect(page.getByText('HR lead is still starting…')).toHaveCount(0);
});

test('HIRE: Back while the HR lead reads keeps the description and the draft', async ({ page }) => {
	await open(page, '/teams/hire?hr=stalls');
	await page.getByLabel('What do you need?').fill('A designer for the account pages');
	await page.getByRole('button', { name: 'Next', exact: true }).click();
	await expect(page.getByText('HR lead is still starting…')).toBeVisible();
	await page.getByRole('button', { name: 'Back', exact: true }).click();
	await expect(stepHeading(page)).toHaveText('Describe the hire');
	await expect(stepHeading(page)).toBeFocused();
	await expect(page.getByLabel('What do you need?')).toHaveValue(
		'A designer for the account pages'
	);
	await page.getByRole('button', { name: 'Next', exact: true }).click();
	await expect(page.getByText('HR lead is still starting…')).toBeVisible();
	await expect(page).toHaveURL(/request=/);
});

test('HIRE: hiring stops partway; Try again finishes without a new confirmation', async ({
	page
}) => {
	await open(page, '/teams/hire?hire=fails&team=platform');
	await page.getByLabel('What do you need?').fill('Weekly numbers on deploys');
	await page.getByRole('button', { name: 'Next', exact: true }).click();
	await expect(stepHeading(page)).toHaveText("Check the HR lead's proposal", { timeout: 10_000 });
	await page.getByRole('button', { name: 'Next', exact: true }).click();
	await page.getByRole('button', { name: 'Hire Marlowe' }).click();
	await expect(stepHeading(page)).toHaveText("Couldn't finish hiring Marlowe", { timeout: 10_000 });
	await expect(
		page.getByText(
			"Couldn't finish hiring Marlowe: couldn't register with messaging. Already done: created Marlowe's record, set up the home folder."
		)
	).toBeVisible();
	await page.getByRole('button', { name: 'Try again' }).click();
	// Focus moves at once, while it is still hiring (the mock takes over 4 s to settle after Try again).
	await expect(stepHeading(page)).toHaveText('Hiring Marlowe', { timeout: 1500 });
	await expect(stepHeading(page)).toBeFocused({ timeout: 1000 });
	await expect(stepHeading(page)).toHaveText('Marlowe is hired and in Platform.', {
		timeout: 10_000
	});
});

test('HIRE: a hire the office gives up says so with its reason', async ({ page }) => {
	await open(page, '/teams/hire?hire=givesup&team=web');
	await page.getByLabel('What do you need?').fill('A tester');
	await page.getByRole('button', { name: 'Next', exact: true }).click();
	await expect(stepHeading(page)).toHaveText("Check the HR lead's proposal", { timeout: 10_000 });
	await page.getByRole('button', { name: 'Next', exact: true }).click();
	await page.getByRole('button', { name: 'Hire Marlowe' }).click();
	await expect(stepHeading(page)).toHaveText('Hiring Marlowe was given up', { timeout: 10_000 });
	await expect(
		page.getByText('Given up: The office stopped and the hire was given up.', { exact: false })
	).toBeVisible();
});

test('HIRE requests: Withdraw a draft and Try again a stopped hire, focus staying on the row', async ({
	page
}) => {
	await open(page, '/teams/hires');
	await expect(page).toHaveTitle('Hire requests · Teams · Agentic Org');
	const list = page.getByRole('list', { name: 'Hire requests' });
	await expect(list.getByRole('listitem')).toHaveCount(5);
	await expect(list.getByText("Not finished: couldn't register with messaging")).toBeVisible();
	// The hirer's own draft: Review, then Withdraw.
	const mine = list.getByRole('listitem').filter({ hasText: 'Marlowe' });
	await expect(mine.getByRole('link').or(mine.getByRole('button'))).toHaveText([
		'Marlowe',
		'Review',
		'Withdraw'
	]);
	// Another hirer's draft is read-only: no link, Review or Withdraw.
	const theirs = list.getByRole('listitem').filter({ hasText: 'Ellis' });
	await expect(theirs.getByRole('link')).toHaveCount(0);
	await expect(theirs.getByRole('button')).toHaveCount(0);
	await page.getByRole('button', { name: 'Withdraw the hire of Marlowe' }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Withdraw', exact: true }).click();
	await expect(page.getByRole('dialog')).toHaveCount(0);
	await expect(page.locator('#hire-h4')).toBeFocused();
	await expect(mine).toContainText('Withdrawn');
	await page.getByRole('button', { name: 'Try again: finish hiring Rowan' }).click();
	await expect(page.locator('#hire-h3')).toBeFocused();
	await expect(list.getByRole('listitem').filter({ hasText: 'Rowan' })).toContainText('Hired', {
		timeout: 15_000
	});
});

test('HIRE requests: loading, error with Retry, and empty', async ({ page }) => {
	await open(page, '/teams/hires?mock=slow');
	await expect(
		page.getByRole('status').filter({ hasText: 'Loading hire requests' })
	).toBeAttached();
	await visit(page, '/teams/hires?mock=fails');
	await expect(page.getByText("Can't load hire requests")).toBeVisible();
	await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();
	await visit(page, '/teams/hires?mock=empty');
	await expect(page.getByText('No hire requests')).toBeVisible();
	await expect(page.getByText('Hires you start show here.')).toBeVisible();
});

test("HIRE: someone else's draft opens read-only, and the flow can't confirm it", async ({
	page
}) => {
	await open(page, '/teams/hire?request=h5');
	await expect(stepHeading(page)).toHaveText('Only its hirer confirms this draft');
	await expect(
		page.getByText('Sam Okafor started this hire request', { exact: false })
	).toBeVisible();
	await expect(page.getByRole('button', { name: /^Hire / })).toHaveCount(0);
});

test('HIRE requests: a failed re-read keeps the rows and says so', async ({ page }) => {
	await open(page, '/teams/hires?hire=refreshfails');
	const list = page.getByRole('list', { name: 'Hire requests' });
	await expect(list.getByRole('listitem')).toHaveCount(5);
	await page.getByRole('button', { name: 'Try again: finish hiring Rowan' }).click();
	await expect(page.getByText("Couldn't check on hires:", { exact: false })).toBeVisible();
	await expect(list.getByRole('listitem')).toHaveCount(5);
	await expect(page.getByText("Can't load hire requests")).toHaveCount(0);
});
