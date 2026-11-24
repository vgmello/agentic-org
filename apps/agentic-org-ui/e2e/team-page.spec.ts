// A team's page (offices-and-teams-ux §7.2, §7.4, §7.10; app-shell-ux §2) against the dev mock: narrow widths and
// large text, Archive and Delete refused before they are offered while anything still points at the team, Add a
// person with one home team and a handle, live changes applied in place, the Conversations destination, the
// unavailable state and a new team's reference. Engineering has two teams under it and three home employees; Web
// has two home employees, owns Beacon and has three open tasks.
import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { signInAsAdmin } from './web-session';
import { enterMockedOrganization } from './mock-organization';

async function openTeam(page: Page, url: string, name = 'Engineering') {
	await signInAsAdmin(page, [
		{
			id: 'org-team-page',
			name: 'Team page',
			created_at: '2026-11-18T00:00:00Z',
			recovery: 'acknowledged'
		}
	]);
	await enterMockedOrganization(page, 'Team page');
	await visit(page, url);
	if (name) await expect(page.getByRole('heading', { name, exact: true })).toBeVisible();
}

async function teamAction(page: Page, team: string, item: string) {
	await page.getByRole('button', { name: `Actions for ${team}` }).click();
	await page.getByRole('menuitem', { name: item }).click();
	return page.getByRole('dialog');
}

test('TEAM-UX: at 390 px every tab is on screen; the strip wraps instead of scrolling', async ({
	page
}) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await openTeam(page, '/teams/eng');
	const strip = page.getByRole('navigation', { name: 'Engineering views' });
	const tabs = strip.getByRole('link');
	await expect(tabs).toHaveText([
		'Overview',
		/^Members\s*3$/,
		/^Work(?:\s*\d+)?$/,
		'Projects',
		'Activity',
		'Conversations',
		'Processes'
	]);
	expect(await strip.evaluate((nav) => nav.scrollWidth - nav.clientWidth)).toBe(0);
	const edge = await strip.evaluate((nav) => nav.getBoundingClientRect().right);
	for (const tab of await tabs.all()) {
		const box = (await tab.boundingBox())!;
		expect(box.x).toBeGreaterThanOrEqual(0);
		expect(box.x + box.width).toBeLessThanOrEqual(edge);
		expect(box.height).toBeGreaterThanOrEqual(44);
	}
	await expect(tabs.last()).toBeInViewport();
});

test('TEAM-UX: Archive names what stops it before it is offered, with a link to each', async ({
	page
}) => {
	await openTeam(page, '/teams/eng');
	await page.getByRole('button', { name: 'Actions for Engineering' }).click();
	await page.getByRole('menuitem', { name: 'Archive…' }).click();
	const dialog = page.getByRole('dialog');
	await expect(dialog.getByRole('heading', { name: "Can't archive Engineering" })).toBeVisible();
	await expect(dialog).toContainText(
		'Engineering still has 2 child teams and 3 home employees. Resolve them first.'
	);
	await expect(dialog.getByRole('link', { name: 'Platform' })).toHaveAttribute(
		'href',
		'/teams/platform'
	);
	await expect(dialog.getByRole('link', { name: 'Web' })).toHaveAttribute('href', '/teams/web');
	for (const name of ['Alex Kim', 'Avery Stone', 'Sam Okafor'])
		await expect(dialog.getByRole('link', { name })).toHaveAttribute('href', /^\/members\//);
	// Nothing to confirm: the refusal is said before the click, not after it.
	await expect(dialog.getByRole('button', { name: /^Archive/ })).toHaveCount(0);
	await dialog.getByRole('link', { name: 'Web' }).click();
	await expect(page).toHaveURL(/\/teams\/web$/);
	await expect(page.getByRole('dialog')).toHaveCount(0);
});

test('TEAM-UX: at 320 px and 200% text each member row keeps a text column of 200 px', async ({
	page
}) => {
	await page.setViewportSize({ width: 320, height: 800 });
	await openTeam(page, '/teams/eng/members');
	await page.evaluate(() => {
		document.documentElement.style.fontSize = '200%';
	});
	const rows = page
		.getByTestId('team-members-roster')
		.getByRole('listitem')
		.filter({
			has: page.locator('[data-member-text]')
		});
	await expect(rows).toHaveCount(3);
	const widths = await rows.evaluateAll((items) =>
		items.map((li) => li.querySelector('[data-member-text] > div')!.getBoundingClientRect().width)
	);
	for (const width of widths) expect(width).toBeGreaterThanOrEqual(200);
	expect(
		await page.evaluate(
			() => document.documentElement.scrollWidth - document.documentElement.clientWidth
		)
	).toBe(0);
});

test('TEAM-9: Delete names what still points at the team, with links, and offers no Delete button', async ({
	page
}) => {
	await openTeam(page, '/teams/web', 'Web');
	const dialog = await teamAction(page, 'Web', 'Delete…');
	await expect(dialog.getByRole('heading', { name: "Can't delete Web" })).toBeVisible();
	await expect(dialog).toContainText(
		'Web still has 2 home employees, 1 project it owns and 3 open tasks. Resolve them first.'
	);
	await expect(dialog.getByRole('link', { name: 'Beacon' })).toHaveAttribute(
		'href',
		'/projects/beacon'
	);
	for (const name of ['Mira Castillo', 'Noor Haddad'])
		await expect(dialog.getByRole('link', { name })).toHaveAttribute('href', /^\/members\//);
	await expect(dialog).toContainText('T-A40 Split the checkout service');
	await expect(dialog.getByRole('button', { name: /^Delete/ })).toHaveCount(0);
	await dialog.getByRole('button', { name: 'Close' }).last().click();
	// Nothing changed: the team, its people and its project are all still there.
	await expect(page.getByRole('heading', { name: 'Web', exact: true })).toBeVisible();
	await expect(page.getByTestId('team-overview')).toContainText('Noor Haddad');
});

test('TEAM-ARCHIVE: Archive also waits for the projects the team owns and its open tasks', async ({
	page
}) => {
	await openTeam(page, '/teams/web', 'Web');
	const dialog = await teamAction(page, 'Web', 'Archive…');
	await expect(dialog.getByRole('heading', { name: "Can't archive Web" })).toBeVisible();
	await expect(dialog.getByRole('heading', { name: 'Projects Web owns' })).toBeVisible();
	await expect(dialog.getByRole('heading', { name: 'Open tasks' })).toBeVisible();
	await expect(dialog.getByRole('button', { name: /^Archive/ })).toHaveCount(0);
});

test(
	'PEOPLE-3/4: Add a person from a team gives one home team and an explicit handle',
	{ tag: '@webkit' },
	async ({ page }) => {
		await page.setViewportSize({ width: 390, height: 844 });
		await openTeam(page, '/teams/web/members', 'Web');
		await page.getByRole('button', { name: 'Add a person' }).click();
		const sheet = page.getByRole('dialog', { name: 'Add person' });
		// One home team, preset to this team; no team checkboxes.
		await expect(sheet.getByLabel('Home team')).toHaveValue('web');
		await expect(sheet.getByRole('checkbox')).toHaveCount(0);
		await sheet.getByLabel('Name').fill('Sam Ortiz');
		await expect(sheet.getByLabel('Handle')).toHaveValue('sam-ortiz');
		// The footer stays on screen at 390 px.
		await expect(sheet.getByRole('button', { name: 'Add person' })).toBeInViewport();
		await sheet.getByLabel('Handle').fill('sam');
		await sheet.getByRole('button', { name: 'Add person' }).click();
		await expect(sheet.getByText('@sam is taken. Try @sam-2.')).toBeVisible();
		await expect(sheet.getByLabel('Handle')).toBeFocused();
		await expect(sheet.getByLabel('Handle')).toHaveAttribute('aria-invalid', 'true');
		// A name change no longer rewrites a handle the person typed.
		await sheet.getByLabel('Name').fill('Sam Ortiz-Lee');
		await expect(sheet.getByLabel('Handle')).toHaveValue('sam');
		await sheet.getByLabel('Name').fill('Sam Ortiz');
		await sheet.getByLabel('Handle').fill('sam-ortiz');
		await sheet.getByRole('button', { name: 'Add person' }).click();
		await expect(sheet).toBeHidden();
		await expect(page.getByText('Added Sam Ortiz. Agents can now reach Sam Ortiz.')).toBeVisible();
		const members = page.getByTestId('team-members-roster');
		await expect(members).toContainText('Sam Ortiz');
		await expect(members).toContainText('@sam-ortiz');
	}
);

test('TEAM-LIVE: a change made elsewhere shows on the open team page without a reload', async ({
	page
}) => {
	await openTeam(page, '/teams/web/members', 'Web');
	const members = page.getByTestId('team-members-roster');
	await expect(members).toContainText('2 of');
	await orgChange(page, {
		method: 'member.add_person',
		params: {
			member_id: '01JC00000000000000000000R1',
			name: 'Rae Lindgren',
			home_team_id: 'web'
		}
	});
	// No reload: the page re-read the team when the organization changed.
	await expect(members).toContainText('Rae Lindgren');
	await expect(members).toContainText('3 of');
});

test('TEAM-CONVERSATIONS: a team channel opens that channel in Chat', async ({ page }) => {
	await openTeam(page, '/teams/web/conversations', 'Web');
	await page.getByRole('link', { name: '#web' }).click();
	await expect(page).toHaveURL(/\/chat\?c=c-team-web$/);
	await expect(page.getByRole('heading', { name: '#web', exact: true })).toBeVisible();
});

test('TEAM-UNAVAILABLE: a server that does not serve teams yet says so, with no Retry', async ({
	page
}) => {
	await openTeam(page, '/teams/web?mock=unavailable', '');
	await expect(page.getByText('Not available yet')).toBeVisible();
	await expect(page.getByText("This server doesn't serve the team yet.")).toBeVisible();
	await expect(page.getByRole('button', { name: 'Retry' })).toHaveCount(0);
	await expect(
		page.getByRole('main').getByRole('link', { name: 'Teams', exact: true })
	).toBeVisible();
});

test('TEAM-REF: a team created here gets the next TEAM reference on its page', async ({ page }) => {
	await openTeam(page, '/teams/eng');
	await teamAction(page, 'Engineering', 'Add team under Engineering…');
	const sheet = page.getByRole('dialog', { name: 'New team' });
	await sheet.getByLabel('Name').fill('QA');
	await sheet.getByRole('button', { name: 'Create team' }).click();
	await expect(sheet).toBeHidden();
	await visit(page, '/teams');
	await page.getByRole('link', { name: /^QA , level 2/ }).click();
	await expect(page.getByRole('heading', { name: 'QA', exact: true })).toBeVisible();
	await expect(
		page
			.getByRole('main')
			.locator('header', { has: page.getByRole('heading', { name: 'QA', exact: true }) })
	).toContainText('TEAM-6');
});

/** Adds a team under Engineering from its page and opens the new team's page, all without a reload. */
async function newTeamUnderEngineering(page: Page, name: string): Promise<string> {
	await openTeam(page, '/teams/eng');
	await teamAction(page, 'Engineering', 'Add team under Engineering…');
	const sheet = page.getByRole('dialog', { name: 'New team' });
	await sheet.getByLabel('Name').fill(name);
	await sheet.getByRole('button', { name: 'Create team' }).click();
	await expect(sheet).toBeHidden();
	await page.getByRole('main').getByRole('link', { name: 'Teams', exact: true }).click();
	await page.getByRole('link', { name: new RegExp(`^${name} , level 2`) }).click();
	await expect(page.getByRole('heading', { name, exact: true })).toBeVisible();
	return new URL(page.url()).pathname.split('/').pop()!;
}

const orgChange = (page: Page, command: object) =>
	page.evaluate(async (command) => {
		const saved = JSON.parse(localStorage.getItem('agentic-org.mock.connections') ?? '[]');
		const id = saved[saved.length - 1].organization.id as string;
		await (
			window as unknown as { __mockOrgChange: (id: string, command: object) => Promise<unknown> }
		).__mockOrgChange(id, command);
	}, command);

test('TEAM-ARCHIVE PEOPLE-3: an archived team is kept, focused, and never taken as a home team', async ({
	page
}) => {
	const qa = await newTeamUnderEngineering(page, 'QA');
	const dialog = await teamAction(page, 'QA', 'Archive…');
	await dialog.getByRole('button', { name: 'Archive QA' }).click();
	await expect(page.getByText('Archived QA. Its ID and history stay.')).toBeVisible();
	await expect(page.getByRole('heading', { name: 'QA', exact: true })).toBeFocused();

	await page.getByRole('main').getByRole('link', { name: 'Teams', exact: true }).click();
	await page.getByRole('link', { name: /^Web , level 2/ }).click();
	await page.getByRole('button', { name: 'Add a person' }).click();
	const sheet = page.getByRole('dialog', { name: 'Add person' });
	const home = sheet.getByLabel('Home team');
	await expect(home.locator('option', { hasText: 'Engineering › Web' })).toHaveCount(1);
	await expect(home.locator('option', { hasText: 'QA' })).toHaveCount(0);
	// HQ refuses it on its own, whatever a form sends.
	await home.evaluate((select, id) => {
		const option = document.createElement('option');
		option.value = id;
		option.text = 'Engineering › QA';
		select.append(option);
	}, qa);
	await home.selectOption(qa);
	await sheet.getByLabel('Name').fill('Ivo Marsh');
	await sheet.getByRole('button', { name: 'Add person' }).click();
	await expect(sheet.getByRole('alert')).toContainText(
		"QA is archived, so it can't be a home team. Choose a team that is still active."
	);
	await expect(sheet).toBeVisible();
	await expect(sheet.getByLabel('Name')).toHaveValue('Ivo Marsh');
});

test('TEAM-9: Delete leaves for Teams without showing the team as not found, and focuses the Teams heading', async ({
	page
}) => {
	await newTeamUnderEngineering(page, 'Temp');
	await page.evaluate(() => {
		const seen = { notFound: false };
		(window as unknown as { __seen: typeof seen }).__seen = seen;
		new MutationObserver(() => {
			if (document.querySelector('main h2')?.textContent?.includes('Team not found'))
				seen.notFound = true;
		}).observe(document.body, { subtree: true, childList: true, characterData: true });
	});
	const dialog = await teamAction(page, 'Temp', 'Delete…');
	await dialog.getByRole('button', { name: 'Delete Temp' }).click();
	await expect(page).toHaveURL(/\/teams$/);
	await expect(page.getByText('Deleted Temp.')).toBeVisible();
	await expect(page.getByRole('heading', { name: /^Teams\b/ })).toBeFocused();
	await expect(page.getByRole('link', { name: /^Temp , level 2/ })).toHaveCount(0);
	expect(
		await page.evaluate(
			() => (window as unknown as { __seen: { notFound: boolean } }).__seen.notFound
		)
	).toBe(false);
});

for (const removal of ['Archive', 'Delete'] as const)
	test(`TEAM-9: HQ refuses a ${removal.toLowerCase()} that became blocked after the check, and keeps the team`, async ({
		page
	}) => {
		const temp = await newTeamUnderEngineering(page, 'Temp');
		const dialog = await teamAction(page, 'Temp', `${removal}…`);
		await expect(dialog.getByRole('button', { name: `${removal} Temp` })).toBeEnabled();
		// Someone adds a team under Temp after the dialog checked; live blocker re-check blocks removal in place (§7.10).
		await orgChange(page, {
			method: 'team.create',
			params: { team_id: 'temp-child', name: 'Child', owner_team_id: temp }
		});
		await expect(dialog.getByRole('button', { name: `${removal} Temp` })).toHaveCount(0);
		await expect(dialog).toContainText('Temp still has 1 child team.');
		await dialog.getByRole('button', { name: 'Close' }).last().click();
		await expect(page).toHaveURL(new RegExp(`/teams/${temp}$`));
		await expect(page.getByRole('heading', { name: 'Temp', exact: true })).toBeVisible();
		await expect(page.getByText('Archived')).toHaveCount(0);
	});

for (const width of [390, 1024])
	test(`SHELL-33: team links inside lines are full-size targets at ${width} px without growing the line`, async ({
		page
	}) => {
		await page.setViewportSize({ width, height: 900 });
		const min = width < 640 ? 44 : 24;
		const reachable = async (link: Locator) => {
			const box = (await link.boundingBox())!;
			expect(box.height).toBeGreaterThanOrEqual(min);
			// Not clipped by the line's wrapper: the target's top and bottom edges hit the link.
			for (const y of [box.y + 1, box.y + box.height - 1])
				expect(
					await link.evaluate(
						(a, [x, y]) => a.contains(document.elementFromPoint(x, y)),
						[box.x + box.width / 2, y]
					)
				).toBe(true);
		};
		await openTeam(page, '/teams/web', 'Web');
		const header = page
			.getByRole('main')
			.locator('header', { has: page.getByRole('heading', { name: 'Web', exact: true }) });
		for (const name of ['Noor Haddad', 'Engineering']) {
			const link = header.getByRole('link', { name, exact: true });
			await reachable(link);
			// The fact holding the link is one line tall, like its neighbours.
			const lineHeight = await link.evaluate(
				(a) => a.closest('li')!.getBoundingClientRect().height
			);
			expect(lineHeight).toBeLessThanOrEqual(24);
		}
		await page.getByRole('main').getByRole('link', { name: 'Teams', exact: true }).click();
		await page.getByRole('link', { name: /^Design , level 1/ }).click();
		await page
			.getByRole('navigation', { name: 'Design views' })
			.getByRole('link', { name: 'Projects', exact: true })
			.click();
		const owned = page.getByRole('main').getByRole('link', { name: 'Web', exact: true });
		await reachable(owned);
		expect(
			await owned.evaluate((a) => a.closest('li')!.getBoundingClientRect().height)
		).toBeLessThanOrEqual(20);
	});

test('TEAM-OVERVIEW: opens on Overview with reconciled KPIs, real project progress, frame-owned Work badge, legacy task detail open, header scoped search, and M/Arrow tab shortcuts', async ({
	page
}) => {
	// Direct visit to Members already shows the live Work tab count without visiting Overview or Work first.
	await openTeam(page, '/teams/web/members', 'Web');
	const membersNav = page.getByRole('navigation', { name: 'Web views' });
	await expect(membersNav.getByRole('link', { name: /^Work\s+3$/ })).toBeVisible();

	await membersNav.getByRole('link', { name: 'Overview', exact: true }).click();
	const overview = page.getByTestId('team-overview');
	await expect(overview).toBeVisible();
	await expect(overview.locator('[data-kpi="blocked"]')).toContainText('1');
	await expect(overview.locator('[data-kpi="blocked"]')).toContainText('Of 3 open items');
	await expect(overview.getByTestId('team-completions-unavailable')).toBeVisible();
	// Active project Beacon renders real progress bar and health pill + alphabetical contributors
	await expect(overview).toContainText('Beacon');
	await expect(overview.getByRole('progressbar', { name: 'Beacon progress' })).toBeVisible();
	const contributorNames = await overview
		.locator('table tbody th')
		.evaluateAll((cells) => cells.map((c) => c.textContent?.trim()));
	expect(contributorNames).toEqual(['Mira Castillo', 'Noor Haddad', 'Sam Okafor']);

	// ArrowRight moves focus across primary tabs without navigating.
	const nav = page.getByRole('navigation', { name: 'Web views' });
	await nav.getByRole('link', { name: 'Overview', exact: true }).focus();
	await page.keyboard.press('ArrowRight');
	await expect(nav.getByRole('link', { name: /^Members/ })).toBeFocused();

	// M activates Page menu navigation; Escape cancels and restores focus; repeated P cycles Projects -> Processes.
	await page.locator('#team-heading').focus();
	await page.keyboard.press('m');
	await expect(nav.getByRole('link', { name: 'Overview', exact: true })).toBeFocused();
	await page.keyboard.press('p');
	await expect(nav.getByRole('link', { name: 'Projects', exact: true })).toBeFocused();
	await page.keyboard.press('p');
	await expect(nav.getByRole('link', { name: 'Processes', exact: true })).toBeFocused();
	await page.keyboard.press('Escape');
	await expect(page.locator('#team-heading')).toBeFocused();

	// Legacy deep link (`/teams/web?item=T-A44` -> `/teams/web/work?item=T-A44`) opens T-A44 detail reading panel.
	await visit(page, '/teams/web?item=T-A44');
	await expect(page).toHaveURL(/\/teams\/web\/work\?item=T-A44$/);
	const panel = page.getByTestId('work-reading-panel');
	await expect(panel).toBeVisible();
	await expect(panel).toContainText('T-A44');
	await expect(panel).toContainText('Decide the retry policy for payments');
	await page.keyboard.press('Escape');
	await expect(panel).toHaveCount(0);

	// Scoped search renders in the shared top header beside Workspace: `/` focuses, typing filters, Escape preserves query.
	const headerSearchHost = page.getByTestId('header-scoped-search');
	await expect(headerSearchHost).toBeVisible();
	const search = headerSearchHost.getByTestId('scoped-work-search');
	await page.locator('#team-heading').focus();
	await page.keyboard.press('/');
	await expect(search).toBeFocused();
	await search.fill('Retry');
	await expect(page.getByText('Decide the retry policy for payments')).toBeVisible();
	await expect(page.getByText('Split the checkout service')).toHaveCount(0);
	await page.keyboard.press('Escape');
	await expect(search).toHaveValue('Retry');
	await expect(page.locator('#team-heading')).toBeFocused();
});
