import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { enterMockedOrganization } from './mock-organization';

// Teams › Members (offices-and-teams-ux §7.0, §7.2, PEOPLE-1/2, TEAM-12; our-org-in-the-app §5 AC-IA08, §7). The old
// Agents roster (status filters, ?q= search, ?add=1 and Add agent) is replaced: agents come only from Hire.

async function createOrg(page: Page) {
	await enterMockedOrganization(page);
	await expect(page).toHaveURL(/\/teams\/members$/);
}
async function openMembers(page: Page, query = '') {
	await createOrg(page);
	await visit(page, '/teams/members' + query);
}
const list = (page: Page) => page.getByRole('list', { name: 'Members', exact: true });
const rows = (page: Page) => list(page).getByRole('listitem');
const row = (page: Page, id: string) => list(page).locator(`[data-member="${id}"]`);
const filter = (page: Page, name: 'All' | 'People' | 'Agents') =>
	page
		.getByRole('group', { name: 'Show members', exact: true })
		.getByRole('radio', { name: new RegExp(`^${name} `) });

/** Chooses a filter as a person does: by its visible label (the radio itself is visually hidden). */
const choose = (page: Page, name: 'All' | 'People' | 'Agents') =>
	filter(page, name).locator('..').click();

const NAMES = [
	'Alex Kim',
	'Avery Stone',
	'Hana Weiss',
	'Inês Duarte',
	'Juno Park',
	'Kai Lindqvist',
	'Leo Brandt',
	'Mira Castillo',
	'Noor Haddad',
	'Pax Moreau',
	'Quinn Adeyemi',
	'Sam Okafor'
];

test(
	'PEOPLE-1 TEAM-12: Members lists people and agents A–Z with kind in words, handle, role, home team and presence',
	{ tag: '@journey' },
	async ({ page }) => {
		await openMembers(page);
		await expect(page).toHaveTitle('Members · Teams · Agentic Org');
		await expect(page.getByRole('heading', { level: 2, name: /^Members/ })).toContainText('12');
		await expect(rows(page)).toHaveCount(12);
		await expect(rows(page).locator('[data-member-link]')).toHaveText(NAMES);
		await expect(filter(page, 'All')).toBeChecked();
		for (const [name, count] of [
			['All', 12],
			['People', 4],
			['Agents', 8]
		] as const)
			await expect(filter(page, name).locator('..')).toContainText(String(count));

		// The owner appears once, as the person they are, with Owner and Can sign in (§7.1, §7.2).
		const alex = row(page, 'you');
		await expect(alex.getByRole('link', { name: 'Alex Kim, person', exact: true })).toHaveAttribute(
			'href',
			'/members/you'
		);
		await expect(alex).toContainText('Owner');
		await expect(alex).toContainText('Person');
		await expect(alex).toContainText('@alex-kim');
		await expect(alex).toContainText('Can sign in');
		await expect(list(page).getByText('Owner', { exact: true })).toHaveCount(1);

		// An agent: kind in words, its office and freshness, its current task and one presence word (never colour alone).
		const sam = row(page, 'sam');
		await expect(sam.getByRole('link', { name: 'Sam Okafor, agent', exact: true })).toHaveAttribute(
			'href',
			'/members/sam'
		);
		await expect(sam).toContainText('Agent');
		await expect(sam).toContainText('@sam');
		await expect(sam).toContainText('Engineering');
		await expect(sam).toContainText(/Runs at .+ · reported .+ ago/);
		await expect(sam).toContainText('T-A40');
		await expect(sam).toContainText('Split the checkout service');
		await expect(sam.locator('[data-presence]')).toHaveText('Working');
		await expect(row(page, 'ines')).toContainText('Unplaced');
		// A person never shows an office (PEOPLE-6), and nobody shows Can sign in without an account.
		await expect(row(page, 'leo')).not.toContainText('Runs at');
		await expect(row(page, 'leo')).not.toContainText('Can sign in');
		// A missing home team is an invalid record, never "No team" (TEAM-3).
		await expect(row(page, 'quinn')).toContainText(
			'No home team on record: this record is invalid'
		);
		// Unknown presence is omitted in rows (AC-IA08).
		await expect(row(page, 'leo').locator('[data-presence]')).toHaveCount(0);
		await expect(page.locator('[data-presence=unknown]')).toHaveCount(0);

		// Agents come only from Hire: no Add agent, no bring-in, no sample data (our-org-in-the-app §7).
		await expect(page.getByRole('link', { name: 'Hire an agent', exact: true })).toHaveAttribute(
			'href',
			'/teams/hire'
		);
		await expect(page.getByRole('button', { name: /Add agent|bring in/i })).toHaveCount(0);
		await expect(page.getByTestId('sample-data-note')).toHaveCount(0);
		await shot(page, 'members-directory');
	}
);

test('PEOPLE-1: All · People · Agents filters from the keyboard and announces how many are shown', async ({
	page
}) => {
	await openMembers(page);
	await filter(page, 'All').focus();
	await page.keyboard.press('ArrowRight');
	await expect(filter(page, 'People')).toBeChecked();
	await expect(filter(page, 'People')).toBeFocused();
	await expect(rows(page)).toHaveCount(4);
	await expect(list(page).locator('[data-kind=agent]')).toHaveCount(0);
	await expect(page.getByRole('status').filter({ hasText: 'shown' })).toHaveText('4 people shown');
	await page.keyboard.press('ArrowRight');
	await expect(filter(page, 'Agents')).toBeChecked();
	await expect(rows(page)).toHaveCount(8);
	await expect(list(page).locator('[data-kind=person]')).toHaveCount(0);
	await expect(page.getByRole('status').filter({ hasText: 'shown' })).toHaveText('8 agents shown');
});

test(
	'AC-IA06: a name in the directory opens that member’s profile',
	{ tag: '@journey' },
	async ({ page }) => {
		await openMembers(page);
		const link = row(page, 'noor').getByRole('link', { name: 'Noor Haddad, agent', exact: true });
		await link.focus();
		await link.press('Enter');
		await expect(page).toHaveURL(/\/members\/noor$/);
		await expect(page.getByRole('heading', { level: 2, name: /^Noor Haddad/ })).toBeVisible();
	}
);

test(
	'PEOPLE-2 §7.4: Add a person adds the row once, focuses it and says agents can reach them',
	{ tag: '@journey' },
	async ({ page }) => {
		await openMembers(page);
		await choose(page, 'Agents');
		await page.getByRole('main').getByRole('button', { name: 'Add a person', exact: true }).click();
		const sheet = page.getByRole('dialog');
		await sheet.getByLabel('Name').fill('Zed Example');
		await sheet.getByRole('button', { name: 'Add person', exact: true }).click();
		await expect(
			page.getByText('Added Zed Example. Agents can now reach Zed.', { exact: true })
		).toBeVisible();
		await expect(filter(page, 'All')).toBeChecked();
		await expect(rows(page)).toHaveCount(13);
		await expect(
			page.getByRole('link', { name: 'Zed Example, person', exact: true })
		).toBeFocused();
		await expect(rows(page).filter({ hasText: 'Zed Example' })).toHaveCount(1);
	}
);

test('WCAG 3.2.2 §7.4: a row arriving late does not take focus from where the person moved it', async ({
	page
}) => {
	// ?mock=race: the add's read resolves first but is superseded; a newer read brings the row 1.5 s later.
	await openMembers(page, '?mock=race');
	await expect(rows(page)).toHaveCount(12);
	await page.getByRole('main').getByRole('button', { name: 'Add a person', exact: true }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByLabel('Name').fill('Vale Example');
	await sheet.getByRole('button', { name: 'Add person', exact: true }).click();
	await expect(sheet).toHaveCount(0);
	// Before the row arrives, the person moves on to the filters.
	const people = filter(page, 'People');
	await people.focus();
	await expect(people).toBeFocused();
	await expect(rows(page).filter({ hasText: 'Vale Example' })).toHaveCount(1);
	// The row is shown and highlighted, but focus stays where the person put it.
	await expect(list(page).locator('li.bg-primary\\/10')).toHaveCount(1);
	await expect(people).toBeFocused();
});

test('§7.4: the added row is focused even when a live change supersedes the read the add started', async ({
	page
}) => {
	// ?mock=race: the add's read resolves first but is superseded; a newer read brings the row 1.5 s later.
	await openMembers(page, '?mock=race');
	await expect(rows(page)).toHaveCount(12);
	await page.getByRole('main').getByRole('button', { name: 'Add a person', exact: true }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByLabel('Name').fill('Wren Example');
	await sheet.getByRole('button', { name: 'Add person', exact: true }).click();
	await expect(
		page.getByText('Added Wren Example. Agents can now reach Wren.', { exact: true })
	).toBeVisible();
	await expect(page.getByRole('link', { name: 'Wren Example, person', exact: true })).toBeFocused();
	await expect(rows(page).filter({ hasText: 'Wren Example' })).toHaveCount(1);
	await expect(rows(page)).toHaveCount(13);
});

test('§7.1: a person added elsewhere appears in the open directory without a reload', async ({
	page
}) => {
	await openMembers(page);
	await choose(page, 'People');
	await expect(rows(page)).toHaveCount(4);
	await page.evaluate(async () => {
		const saved = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]');
		const id = saved[saved.length - 1].organization.id as string;
		await (
			window as unknown as { __mockOrgChange: (id: string, command: object) => Promise<unknown> }
		).__mockOrgChange(id, {
			method: 'member.add_person',
			params: { member_id: '01JC00000000000000000000Y1', name: 'Yara Example' }
		});
	});
	// The page was not reloaded: the filter choice made before the add is still in place.
	await expect(filter(page, 'People')).toBeChecked();
	await expect(rows(page).filter({ hasText: 'Yara Example' })).toHaveCount(1);
	await expect(rows(page)).toHaveCount(5);
	await expect(page.getByRole('heading', { level: 2, name: /^Members/ })).toContainText('13');
});

test('§7.4 PEOPLE-5: a former member is listed apart, without presence, account or work', async ({
	page
}) => {
	await createOrg(page);
	// Offboard from the member's profile (More › Offboard…), as a person does.
	await visit(page, '/members/sam');
	await page.getByRole('button', { name: /^More/ }).click();
	await page.getByRole('menuitem', { name: 'Offboard…', exact: true }).click();
	await page.getByRole('dialog').getByRole('button', { name: 'Offboard', exact: true }).click();
	await expect(page.getByText('Offboarded Sam Okafor.')).toBeVisible();
	await visit(page, '/teams/members');
	await expect(rows(page)).toHaveCount(11);
	await expect(filter(page, 'Agents').locator('..')).toContainText('7');
	const former = page.getByTestId('former-members');
	await former.getByText('Former members').click();
	const sam = page
		.getByRole('list', { name: 'Former members', exact: true })
		.locator('[data-member=sam]');
	await expect(sam).toContainText(/Former member|Left /);
	await expect(sam.locator('[data-presence]')).toHaveCount(0);
	await expect(sam).not.toContainText('Split the checkout service');
	await expect(sam).not.toContainText('Can sign in');
	await shot(page, 'members-former');
});

test('every directory state is real: empty, error with Retry, unavailable, filtered-empty and loading', async ({
	page
}) => {
	await openMembers(page, '?mock=empty');
	await expect(page.getByRole('heading', { name: 'No members yet', exact: true })).toBeVisible();
	await expect(
		page.getByRole('main').getByRole('button', { name: 'Add a person', exact: true })
	).toHaveCount(2);

	await visit(page, '/teams/members?mock=fails');
	await expect(page.getByText("Couldn't load the members", { exact: true })).toBeVisible();
	await expect(page.getByRole('button', { name: 'Retry', exact: true })).toBeVisible();
	await expect(rows(page)).toHaveCount(0);

	await visit(page, '/teams/members?mock=unavailable');
	await expect(
		page.getByRole('heading', { name: "Members isn't built yet", exact: true })
	).toBeVisible();
	await expect(rows(page)).toHaveCount(0);

	await visit(page, '/teams/members?mock=no-agents');
	await choose(page, 'Agents');
	const empty = page.getByTestId('members-filtered-empty');
	await expect(empty).toContainText('No agents in Acme yet. An agent joins only through Hire.');
	await expect(empty.getByRole('link', { name: 'Hire an agent', exact: true })).toHaveAttribute(
		'href',
		'/teams/hire'
	);

	await visit(page, '/teams/members?mock=all-former');
	await expect(page.getByTestId('members-filtered-empty')).toContainText(
		'No current members: everyone listed here is a former member.'
	);
	await choose(page, 'Agents');
	await expect(page.getByTestId('members-filtered-empty')).toContainText(
		'No current agents in Acme.'
	);
	await expect(page.getByTestId('former-members')).toContainText('Former members');

	await visit(page, '/teams/members?mock=slow');
	await expect(
		page.getByRole('status').filter({ hasText: /Loading members|Still loading members/ })
	).toBeAttached();
	await expect(rows(page)).toHaveCount(12, { timeout: 10_000 });
});

test('retired roster URLs: ?add=1 opens nothing and ?q= no longer filters; /agents opens Members', async ({
	page
}) => {
	await createOrg(page);
	await visit(page, '/agents');
	await expect(page).toHaveURL(/\/teams\/members$/);
	await visit(page, '/teams/members?add=1');
	await expect(rows(page)).toHaveCount(12);
	await expect(page.getByRole('dialog')).toHaveCount(0);
	await visit(page, '/teams/members?q=noor');
	await expect(rows(page)).toHaveCount(12);
	await expect(page.getByRole('searchbox')).toHaveCount(0);
});

for (const [width, scale] of [
	[320, 1],
	[320, 2],
	[390, 1],
	[1024, 1],
	[1440, 1],
	[1440, 2]
] as const) {
	test(`§6a reflow: rows stay readable at ${width}px and ${scale * 100}% text`, async ({
		page
	}) => {
		await page.setViewportSize({ width, height: 900 });
		await openMembers(page);
		await page.locator('html').evaluate((html, pixels) => {
			html.style.fontSize = pixels + 'px';
		}, 16 * scale);
		await expect(rows(page)).toHaveCount(12);
		// Columns appear only when the list itself is at least 56rem wide; otherwise rows stack.
		const columns = page
			.getByTestId('members-list')
			.locator('[aria-hidden=true]')
			.filter({ hasText: 'Working on' });
		if (width === 1440 && scale === 1) await expect(columns).toBeVisible();
		else await expect(columns).toBeHidden();
		expect(await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)).toBe(0);
		// Every presence pill keeps whole words inside its row: nothing clipped or cut (Kai's is the longest).
		const pills = await list(page)
			.locator('[data-presence]')
			.evaluateAll((pills) =>
				pills.map((pill) => {
					const li = pill.closest('li')!.getBoundingClientRect(),
						box = pill.getBoundingClientRect();
					const label = pill.lastElementChild as HTMLElement;
					return {
						inside: box.left >= li.left - 1 && box.right <= li.right + 1,
						clipped: label.scrollWidth > label.clientWidth + 1
					};
				})
			);
		expect(pills.length).toBeGreaterThan(0);
		for (const p of pills) expect(p).toEqual({ inside: true, clipped: false });
		// Names never break one word per line: the name column is at least a few words wide.
		const nameWidth = await row(page, 'avery')
			.locator('[data-member-link]')
			.evaluate((a) => a.getBoundingClientRect().width);
		expect(nameWidth).toBeGreaterThan(60 * scale);
		await shot(page, `members-reflow-${width}-${scale}`);
	});
}
