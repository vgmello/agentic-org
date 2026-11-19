// Teams › Org chart (offices-and-teams-ux §7.1, §7.4, §7.6, §7.8, §7.10; our-org-in-the-app §7; app-shell-ux §2,
// §4, §6a) against the dev mock. The sample: Alex Kim (you) owns the organization and has no explicit manager, nor
// has Avery Stone; Hana Weiss and Sam Okafor report to Avery; Leo Brandt and Inês Duarte (unplaced) to Hana; Noor,
// Kai and Mira to Sam; Juno to Avery and Pax to Juno. Avery is the only other person with an account.
import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { signInAsAdmin } from './web-session';
import { enterMockedOrganization } from './mock-organization';

async function openChart(page: Page, url = '/teams/org-chart') {
	await signInAsAdmin(page, [
		{
			id: 'org-chart-tree',
			name: 'Lumen Labs',
			created_at: '2026-11-18T00:00:00Z',
			recovery: 'acknowledged'
		}
	]);
	await enterMockedOrganization(page, 'Lumen Labs');
	await visit(page, url);
	await expect(page.getByRole('heading', { name: /^Org chart/, level: 2 })).toBeVisible();
}

const tree = (page: Page) => page.getByRole('list', { name: 'Reporting lines' });
const node = (page: Page, id: string) => page.locator(`li[data-node="${id}"]`);
/** The node's own row: its facts, without the reports drawn under it. */
const row = (page: Page, id: string) => node(page, id).locator(':scope > div');
const reportsOf = (page: Page, id: string) => page.locator(`#reports-${id} > li`);

async function nodeAction(page: Page, name: string, item: string): Promise<Locator> {
	await page.getByRole('button', { name: `Actions for ${name}` }).click();
	await page.getByRole('menuitem', { name: item }).click();
	return page.getByRole('dialog');
}

test(
	'OWNER-1 REPORT-1: the owner bar sits above one reporting tree where everyone appears once, without an invented manager',
	{ tag: '@journey' },
	async ({ page }) => {
		await openChart(page);
		await expect(page).toHaveTitle('Org chart · Teams · Agentic Org');
		const bar = page.locator('[data-owner-bar]');
		await expect(bar.getByRole('heading', { level: 3 })).toHaveText(
			'Alex Kim (you) · Owner of Lumen Labs'
		);
		await expect(bar).toContainText('Full access to this organization.');
		await expect(bar.getByRole('link', { name: 'Profile of Alex Kim' })).toHaveAttribute(
			'href',
			'/members/you'
		);
		await expect(bar.getByRole('button', { name: 'Hand over ownership…' })).toBeVisible();
		await expect(
			page.getByText(
				"Reporting lines are explicit. Team leads also manage their team's subtree; ordinary ancestor-team members do not."
			)
		).toBeVisible();

		// The roots are exactly those with no explicit manager, the owner's person among them, in their own place.
		await expect(page.getByRole('heading', { name: /No explicit manager/ })).toBeVisible();
		await expect(tree(page).locator(':scope > li')).toHaveCount(3);
		await expect(row(page, 'you')).toContainText('Owner');
		await expect(row(page, 'you')).toContainText('Person · @alex-kim · Engineering');
		const ids = await tree(page)
			.locator('li[data-node]')
			.evaluateAll((items) => items.map((li) => li.getAttribute('data-node')));
		expect(new Set(ids).size).toBe(ids.length);
		expect(ids).toHaveLength(12);
		await expect(reportsOf(page, 'avery')).toHaveCount(3);
		await expect(reportsOf(page, 'sam')).toHaveCount(3);
		// No zoom, Fit, chart search or team and people canvases (TEAM-11).
		for (const gone of [/Fit/, /Zoom/, /View/, /Search members/])
			await expect(page.getByRole('main').getByRole('button', { name: gone })).toHaveCount(0);
		await expect(page.getByRole('tab')).toHaveCount(0);
	}
);

test('REPORT-2 TEAM-12 PEOPLE-6: a node says its kind in words, handle, home team, reports, and an agent its office', async ({
	page
}) => {
	await openChart(page);
	await expect(row(page, 'sam')).toContainText('Agent · @sam · Engineering');
	await expect(row(page, 'sam')).toContainText(/HQ · reported \d+ s ago/);
	await expect(row(page, 'sam')).toContainText('Leads Engineering · 3 reports');
	await expect(row(page, 'ines')).toContainText('Unplaced');
	await expect(row(page, 'hana')).toContainText('Person · @hana.weiss · People');
	await expect(row(page, 'hana')).not.toContainText(/HQ|Unplaced|reported/);
	await expect(row(page, 'leo')).not.toContainText(/report/);
	// The accessible name says the kind; the link opens the profile.
	const noor = page.getByRole('link', { name: 'Noor Haddad, agent' });
	await expect(noor).toHaveAttribute('href', '/members/noor');
	await expect(page.getByRole('link', { name: 'Leo Brandt, person' })).toBeVisible();
	await noor.click();
	await expect(page).toHaveURL(/\/members\/noor$/);
});

test('TEAM-10: arrow keys move through the tree, Left and Right close and open a manager, the toggle names them', async ({
	page
}) => {
	await openChart(page);
	await page.getByRole('link', { name: 'Alex Kim, person' }).focus();
	await page.keyboard.press('ArrowDown');
	await expect(page.getByRole('link', { name: 'Avery Stone, person' })).toBeFocused();
	await page.keyboard.press('ArrowRight');
	await expect(page.getByRole('link', { name: 'Hana Weiss, person' })).toBeFocused();
	await page.keyboard.press('ArrowLeft');
	const hanaToggle = page.getByRole('button', { name: "Collapse Hana Weiss's reports" });
	await expect(page.getByRole('button', { name: "Expand Hana Weiss's reports" })).toHaveAttribute(
		'aria-expanded',
		'false'
	);
	await expect(page.getByRole('link', { name: 'Leo Brandt, person' })).toBeHidden();
	await page.keyboard.press('ArrowDown');
	await expect(page.getByRole('link', { name: 'Juno Park, agent' })).toBeFocused();
	await page.keyboard.press('ArrowUp');
	await page.keyboard.press('ArrowRight');
	await expect(hanaToggle).toHaveAttribute('aria-expanded', 'true');
	await expect(page.getByRole('link', { name: 'Leo Brandt, person' })).toBeVisible();
	await page.keyboard.press('End');
	await expect(page.getByRole('link', { name: 'Quinn Adeyemi, agent' })).toBeFocused();
	await page.keyboard.press('Home');
	await expect(page.getByRole('link', { name: 'Alex Kim, person' })).toBeFocused();
	// The toggle works by pointer too.
	await page.getByRole('button', { name: "Collapse Avery Stone's reports" }).click();
	await expect(page.getByRole('link', { name: 'Sam Okafor, agent' })).toBeHidden();
	await page.getByRole('button', { name: "Expand Avery Stone's reports" }).click();
	await expect(page.getByRole('link', { name: 'Sam Okafor, agent' })).toBeVisible();
});

test('REPORT-3: Change manager offers the owner and No explicit manager, never the employee, and disables their reports with the reason', async ({
	page
}) => {
	await openChart(page);
	const dialog = await nodeAction(page, 'Avery Stone', 'Change manager…');
	await expect(dialog.getByRole('heading', { name: "Change Avery Stone's manager" })).toBeVisible();
	await expect(dialog.getByRole('radio', { name: /No explicit manager/ })).toBeChecked();
	await expect(dialog.getByRole('radio', { name: /^Alex Kim/ })).toBeEnabled();
	await expect(dialog.getByRole('radio', { name: /^Avery Stone/ })).toHaveCount(0);
	for (const name of ['Hana Weiss', 'Sam Okafor', 'Noor Haddad', 'Leo Brandt']) {
		const radio = dialog.getByRole('radio', { name: new RegExp(`^${name}`) });
		await expect(radio).toBeDisabled();
		await expect(radio).toHaveAccessibleDescription('Reports to Avery Stone');
	}
	await expect(dialog.getByRole('radio', { name: /^Quinn Adeyemi/ })).toBeEnabled();
	await dialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(page.getByRole('button', { name: 'Actions for Avery Stone' })).toBeFocused();
});

test(
	'REPORT-4: a changed manager moves the node, says who it reports to now and keeps focus on it',
	{ tag: '@journey' },
	async ({ page }) => {
		await openChart(page);
		const dialog = await nodeAction(page, 'Noor Haddad', 'Change manager…');
		await dialog.getByRole('radio', { name: /^Juno Park/ }).check();
		await dialog.getByRole('button', { name: 'Save manager' }).click();
		await expect(page.getByText('Noor Haddad now reports to Juno Park.')).toBeVisible();
		await expect(dialog).toBeHidden();
		await expect(
			reportsOf(page, 'juno').filter({ has: page.locator('[data-node-link="noor"]') })
		).toHaveCount(1);
		await expect(reportsOf(page, 'sam')).toHaveCount(2);
		await expect(page.getByRole('button', { name: 'Actions for Noor Haddad' })).toBeFocused();
		// Reporting doesn't rewrite lead appointments (REPORT-5).
		await expect(row(page, 'noor')).toContainText('Leads Web');

		// No explicit manager puts them at the top, without inventing one.
		await nodeAction(page, 'Noor Haddad', 'Change manager…');
		await page
			.getByRole('dialog')
			.getByRole('radio', { name: /No explicit manager/ })
			.check();
		await page.getByRole('dialog').getByRole('button', { name: 'Save manager' }).click();
		await expect(page.getByText('Noor Haddad has no explicit manager now.')).toBeVisible();
		await expect(tree(page).locator(':scope > li[data-node="noor"]')).toHaveCount(1);
	}
);

test('REPORT-4: a record changed meanwhile is refused with the latest lines, the choice kept, and a retry works', async ({
	page
}) => {
	await openChart(page, '/teams/org-chart?mock=stale');
	const dialog = await nodeAction(page, 'Leo Brandt', 'Change manager…');
	await dialog.getByRole('radio', { name: /^Sam Okafor/ }).check();
	await dialog.getByRole('button', { name: 'Save manager' }).click();
	await expect(dialog.getByRole('alert')).toHaveText(
		"Leo Brandt changed while you were editing. Here's the latest."
	);
	await expect(dialog.getByRole('radio', { name: /^Sam Okafor/ })).toBeChecked();
	// Someone else cleared Leo's manager: the tree shows it under the dialog already.
	await expect(tree(page).locator(':scope > li[data-node="leo"]')).toHaveCount(1);
	await dialog.getByRole('button', { name: 'Save manager' }).click();
	await expect(page.getByText('Leo Brandt now reports to Sam Okafor.')).toBeVisible();
	await expect(
		reportsOf(page, 'sam').filter({ has: page.locator('[data-node-link="leo"]') })
	).toHaveCount(1);
});

test('REPORT-4: a loop made meanwhile is refused, naming it, and changes nothing', async ({
	page
}) => {
	await openChart(page, '/teams/org-chart?mock=loop');
	const dialog = await nodeAction(page, 'Hana Weiss', 'Change manager…');
	await dialog.getByRole('radio', { name: /^Quinn Adeyemi/ }).check();
	await dialog.getByRole('button', { name: 'Save manager' }).click();
	await expect(dialog.getByRole('alert')).toContainText(
		"Quinn Adeyemi now reports to Hana Weiss, so Hana Weiss can't report to Quinn Adeyemi."
	);
	await expect(dialog.getByRole('radio', { name: /^Quinn Adeyemi/ })).toBeDisabled();
	await expect(dialog.getByRole('radio', { name: /^Quinn Adeyemi/ })).toHaveAccessibleDescription(
		'Reports to Hana Weiss'
	);
	await expect(
		reportsOf(page, 'avery').filter({ has: page.locator('[data-node-link="hana"]') })
	).toHaveCount(1);
});

test(
	'OWNER-2: Hand over ownership names the effect, refuses a wrong password in place, then moves ownership without a reload',
	{ tag: '@journey' },
	async ({ page }) => {
		await openChart(page);
		await page.getByRole('button', { name: 'Hand over ownership…' }).click();
		const dialog = page.getByRole('dialog');
		await expect(
			dialog.getByRole('heading', { name: 'Hand over ownership of Lumen Labs' })
		).toBeVisible();
		// Only active people with an account, never the owner or an agent.
		await expect(dialog.getByRole('radio')).toHaveCount(1);
		await dialog.getByRole('radio', { name: /Avery Stone/ }).check();
		await expect(dialog).toContainText(
			"Make Avery Stone the owner of Lumen Labs? You'll stay in Lumen Labs as a person, without the owner's access."
		);
		await dialog.getByLabel('Your password').fill('not-it');
		await dialog.getByRole('button', { name: 'Hand over ownership' }).click();
		await expect(dialog.getByLabel('Your password')).toBeFocused();
		await expect(dialog.getByLabel('Your password')).toHaveAttribute('aria-invalid', 'true');
		await expect(dialog).toContainText("That password isn't right. Nothing changed.");
		await expect(dialog.getByRole('radio', { name: /Avery Stone/ })).toBeChecked();

		await dialog.getByLabel('Your password').fill('owner-password');
		await dialog.getByRole('button', { name: 'Hand over ownership' }).click();
		await expect(page.getByText('Avery Stone is now the owner of Lumen Labs.')).toBeVisible();
		const bar = page.locator('[data-owner-bar]');
		await expect(bar.getByRole('heading', { level: 3 })).toHaveText(
			'Avery Stone · Owner of Lumen Labs'
		);
		await expect(bar.getByRole('button', { name: 'Hand over ownership…' })).toHaveCount(0);
		await expect(page.locator('#owner-bar-name')).toBeFocused();
		await expect(row(page, 'avery')).toContainText('Owner');
		await expect(row(page, 'you')).not.toContainText('Owner');
	}
);

test(
	'PEOPLE-3: Add a person starts at the top-level team, shows them once under their manager, focused, after HQ accepts',
	{ tag: '@journey' },
	async ({ page }) => {
		await openChart(page);
		await page.getByRole('button', { name: 'Add a person' }).click();
		const sheet = page.getByRole('dialog');
		await expect(sheet.getByLabel('Home team')).toHaveValue('eng');
		await sheet.getByLabel('Name').fill('Sam Ortiz');
		await expect(sheet.getByLabel('Handle')).toHaveValue('sam-ortiz');
		// The owner's person is an ordinary choice of manager (§7.1).
		await expect(
			sheet.getByLabel('Reports to').locator('option', { hasText: 'Alex Kim' })
		).toHaveCount(1);
		await sheet.getByLabel('Reports to').selectOption({ label: 'Hana Weiss' });
		await sheet.getByRole('button', { name: 'Add person' }).click();
		await expect(page.getByText('Added Sam Ortiz. Agents can now reach Sam Ortiz.')).toBeVisible();
		const added = page.getByRole('link', { name: 'Sam Ortiz, person' });
		await expect(added).toBeFocused();
		await expect(
			page.locator('#reports-hana').getByRole('link', { name: 'Sam Ortiz, person' })
		).toHaveCount(1);
		await expect(page.getByRole('heading', { name: /^Org chart/ })).toContainText('13');
	}
);

test('TEAM-6: Move home team from a node changes the home team it shows and keeps its reporting line', async ({
	page
}) => {
	await openChart(page);
	const dialog = await nodeAction(page, 'Leo Brandt', 'Move home team…');
	await dialog.getByRole('radio', { name: 'Design' }).check();
	await dialog.getByRole('button', { name: 'Move Leo Brandt' }).click();
	await expect(page.getByText('Moved Leo Brandt to Design.')).toBeVisible();
	await expect(row(page, 'leo')).toContainText('Person · @leo · Design');
	await expect(
		reportsOf(page, 'hana').filter({ has: page.locator('[data-node-link="leo"]') })
	).toHaveCount(1);
	await expect(page.getByRole('button', { name: 'Actions for Leo Brandt' })).toBeFocused();
});

test('our-org §7: no Add agent anywhere; Hire an agent opens the Hire flow', async ({ page }) => {
	await openChart(page);
	await expect(page.getByText(/Add (an )?agent/)).toHaveCount(0);
	await page.getByRole('button', { name: 'New' }).click();
	await expect(page.getByRole('menuitem', { name: /Add (an )?agent/ })).toHaveCount(0);
	await page.keyboard.press('Escape');
	await page.getByRole('link', { name: 'Hire an agent' }).click();
	await expect(page).toHaveURL(/\/teams\/hire$/);
});

test.describe('states', () => {
	test('loading is shaped like the page and says so, then the tree', async ({ page }) => {
		await openChart(page);
		await page.goto('/teams/org-chart?mock=slow');
		const status = page.getByRole('status').filter({ hasText: /oading the org chart/ });
		await expect(status).toBeAttached();
		await expect(
			page.getByRole('status').filter({ hasText: 'Still loading the org chart' })
		).toBeAttached();
		await expect(tree(page)).toBeVisible({ timeout: 10_000 });
	});

	test('an error says what failed with Retry', async ({ page }) => {
		await openChart(page, '/teams/org-chart?mock=fails');
		await expect(page.getByRole('heading', { name: "Couldn't load the org chart" })).toBeVisible();
		await expect(page.getByText("HQ didn't answer.")).toBeVisible();
		await expect(page.getByRole('button', { name: /Retry|Try again/ })).toBeVisible();
		await expect(tree(page)).toHaveCount(0);
	});

	test('unavailable invents nothing and offers no actions that need the chart', async ({
		page
	}) => {
		await openChart(page, '/teams/org-chart?mock=unavailable');
		await expect(page.getByText("This server doesn't serve the org chart yet.")).toBeVisible();
		await expect(page.locator('[data-owner-bar]')).toHaveCount(0);
		await expect(page.getByRole('button', { name: 'Add a person' })).toHaveCount(0);
	});

	test('empty explains the page', async ({ page }) => {
		await openChart(page, '/teams/org-chart?mock=empty');
		await expect(
			page.getByRole('heading', { name: 'No one is on the org chart yet' })
		).toBeVisible();
		await expect(page.getByText('The org chart shows who reports to whom.')).toBeVisible();
		// The page's one next step stays offered: Add a person in the header.
		await expect(page.getByRole('button', { name: 'Add a person' })).toBeVisible();
	});

	test('a large organization starts collapsed below level two and still opens', async ({
		page
	}) => {
		await openChart(page, '/teams/org-chart?mock=large');
		await expect(page.getByRole('heading', { name: /^Org chart/ })).toContainText('63');
		await expect(page.getByRole('link', { name: 'Sam Okafor, agent' })).toBeVisible();
		await expect(page.getByRole('link', { name: 'Noor Haddad, agent' })).toBeHidden();
		await page.getByRole('button', { name: "Expand Sam Okafor's reports" }).click();
		await expect(page.getByRole('link', { name: 'Noor Haddad, agent' })).toBeVisible();
	});
});

test('REPORT-4 §6a: HQ going away after a refused save keeps the dialog, the choice and its message', async ({
	page
}) => {
	await openChart(page, '/teams/org-chart?mock=drop');
	const dialog = await nodeAction(page, 'Leo Brandt', 'Change manager…');
	await dialog.getByRole('radio', { name: /^Sam Okafor/ }).check();
	await dialog.getByRole('button', { name: 'Save manager' }).click();
	// The re-read failed too: the page says so behind the dialog, which stays as it was.
	await expect(page.getByRole('heading', { name: "Couldn't load the org chart" })).toBeVisible();
	await expect(dialog).toBeVisible();
	await expect(dialog.getByRole('alert')).toContainText("HQ didn't answer.");
	await expect(dialog.getByRole('radio', { name: /^Sam Okafor/ })).toBeChecked();
	// Its node is gone with the chart: focus goes to the page heading, never body.
	await dialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(page.getByRole('heading', { name: /^Org chart/, level: 2 })).toBeFocused();
});

test('OWNER-2: with nobody else holding an account, Hand over explains how and offers nothing that cannot work', async ({
	page
}) => {
	await openChart(page, '/teams/org-chart?mock=solo');
	await page.getByRole('button', { name: 'Hand over ownership…' }).click();
	const dialog = page.getByRole('dialog');
	await expect(dialog).toContainText(
		'Nobody can take over yet: only a person with an account can own Lumen Labs.'
	);
	await expect(dialog.getByRole('radio')).toHaveCount(0);
	await expect(dialog.getByLabel('Your password')).toHaveCount(0);
	await expect(dialog.getByRole('button')).toHaveText(['Close', 'Close']);
});

test('§7.10: a live change re-reads in place, keeping what is collapsed and where focus is', async ({
	page
}) => {
	await openChart(page);
	await page.getByRole('button', { name: "Collapse Hana Weiss's reports" }).click();
	const juno = page.getByRole('link', { name: 'Juno Park, agent' });
	await juno.focus();
	const organizationId = await page.evaluate(() => {
		const saved = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]');
		return saved[saved.length - 1].organization.id as string;
	});
	// Someone else makes Mira report to Avery.
	await page.evaluate(
		(id) =>
			(
				window as unknown as {
					__mockOrgChange: (id: string, command: object) => Promise<unknown>;
				}
			).__mockOrgChange(id, {
				method: 'member.set_reports_to',
				params: { member_id: 'mira', expected_version: 1, reports_to_member_id: 'avery' }
			}),
		organizationId
	);
	await expect(reportsOf(page, 'avery')).toHaveCount(4);
	await expect(reportsOf(page, 'sam')).toHaveCount(2);
	await expect(page.getByRole('button', { name: "Expand Hana Weiss's reports" })).toHaveAttribute(
		'aria-expanded',
		'false'
	);
	await expect(juno).toBeFocused();
});

test('§7.0 §7.1: the built-in HR lead is an employee in the tree, once, with its home team', async ({
	page
}) => {
	await openChart(page, '/teams/org-chart?mock=hr');
	await expect(page.getByRole('link', { name: 'HR lead, agent' })).toHaveCount(1);
	await expect(tree(page).locator(':scope > li[data-node="hr-lead"]')).toHaveCount(1);
	await expect(row(page, 'hr-lead')).toContainText('Agent · @hr · Engineering');
	await expect(page.getByRole('heading', { name: /^Org chart/ })).toContainText('13');
});

for (const [width, scale] of [
	[1440, 1],
	[1024, 1],
	[640, 1],
	[390, 1],
	[320, 1],
	[320, 2]
] as const)
	test(`§2: at ${width} px and ${scale * 100}% text nothing scrolls sideways and the node controls are big enough`, async ({
		page
	}) => {
		await page.setViewportSize({ width, height: 900 });
		await openChart(page);
		if (scale !== 1)
			await page.addStyleTag({ content: `html { font-size: ${scale * 100}% !important }` });
		await expect(tree(page)).toBeVisible();
		expect(
			await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)
		).toBeLessThanOrEqual(0);
		const main = page.getByRole('main');
		expect(await main.evaluate((m) => m.scrollWidth - m.clientWidth)).toBeLessThanOrEqual(0);
		const min = width < 640 ? 44 : 24;
		for (const control of [
			page.getByRole('button', { name: 'Actions for Sam Okafor' }),
			page.getByRole('button', { name: "Collapse Sam Okafor's reports" }),
			page.getByRole('link', { name: 'Sam Okafor, agent' }),
			page.getByRole('button', { name: 'Hand over ownership…' })
		]) {
			const box = (await control.boundingBox())!;
			expect(box.height).toBeGreaterThanOrEqual(min);
			expect(box.x).toBeGreaterThanOrEqual(0);
			expect(box.x + box.width).toBeLessThanOrEqual(width);
		}
		// app-shell §2: no text column under 200 px. Every node's facts keep at least that, at every level.
		const narrowest = await tree(page)
			.locator('[data-facts]')
			.evaluateAll((facts) =>
				Math.min(
					...facts
						.filter((f) => (f as HTMLElement).offsetParent)
						.map((f) => f.getBoundingClientRect().width)
				)
			);
		expect(narrowest).toBeGreaterThanOrEqual(200);
	});
