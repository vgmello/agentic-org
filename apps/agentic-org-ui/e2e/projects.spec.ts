// Projects (work-items-ux §3, PRJ-1..5; offices-and-teams-ux §7.9; development-processes §3; costs-and-insights-ux
// §1), against the dev mock. The mock's switches on the address select the states:
// ?mock=slow|fails|empty|unavailable|viewer|stale|conflict|save-fails (e2e/fakes/modules/projects.ts).
import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import { enterMockedOrganization } from './mock-organization';

async function open(page: Page, url = '/projects') {
	await enterMockedOrganization(page, 'Lumen Labs');
	await visit(page, url);
}
const row = (page: Page, id: string) =>
	page.locator(`[data-testid=project-row][data-project="${id}"]`);
const tabs = (page: Page) => page.getByRole('navigation', { name: /views$/ }).getByRole('link');

async function noSidewaysScroll(page: Page) {
	expect(
		await page.evaluate(
			() => document.documentElement.scrollWidth - document.documentElement.clientWidth
		)
	).toBeLessThanOrEqual(0);
}

test(
	'PROJ-1: the list shows every project A–Z with its accountable team, contributors, work, bugs and investment',
	{ tag: '@journey' },
	async ({ page }) => {
		await open(page);
		await expect(page).toHaveTitle('Projects · Agentic Org');
		await expect(page.getByRole('heading', { level: 2, name: /^Projects/ })).toBeVisible();
		await expect(page.getByTestId('project-row')).toHaveCount(4);
		const names = await page
			.getByTestId('project-row')
			.locator('a:has(span.absolute)')
			.allInnerTexts();
		expect(names).toEqual(['Atlas', 'Beacon', 'Onboarding kit', 'Scratchpad']);
		const beacon = row(page, 'beacon');
		await expect(beacon).toContainText('P-8');
		await expect(beacon.getByTestId('owner-team')).toHaveText(/Accountable team: Web\s*TEAM-2/);
		await expect(beacon.getByTestId('contributors')).toHaveText(/Contributors: Design/);
		await expect(beacon.getByTestId('open-tasks')).toHaveText('6 open · 1 blocked');
		await expect(beacon.getByTestId('bugs').locator('.sr-only')).toHaveText(
			'5 open bugs: 1 critical · 2 major · 1 trivial · 1 not set'
		);
		await expect(beacon.getByTestId('investment')).toContainText('≈ $');
		// The sample's project that had no team now names one: a project always has its accountable team.
		await expect(row(page, 'scratch').getByTestId('owner-team')).toContainText('Engineering');
		await expect(row(page, 'onboarding').getByTestId('bugs').locator('.sr-only')).toHaveText(
			'No open bugs'
		);
		await expect(page.getByRole('button', { name: 'New project' })).toBeVisible();
		await shot(page, 'projects-list');
	}
);

test(
	'PROJ-2: New project creates its own team by default, previews the development process and opens the project',
	{ tag: '@journey' },
	async ({ page }) => {
		await open(page);
		await page.getByRole('button', { name: 'New project' }).click();
		const sheet = page.getByRole('dialog', { name: 'New project' });
		await sheet.getByRole('button', { name: 'Create project' }).click();
		await expect(sheet.getByRole('textbox', { name: 'Name' })).toBeFocused();
		await expect(sheet.getByText('Enter a name for the project.')).toBeVisible();
		await sheet.getByRole('textbox', { name: 'Name' }).fill('Reservations');
		await expect(sheet.getByLabel('Slug')).toHaveValue('reservations');
		await expect(sheet.getByRole('radio', { name: /New team: Reservations/ })).toBeChecked();
		// The new team goes under the organization's top-level team unless the person chooses another place.
		await expect(sheet.locator('#new-project-parent')).toHaveValue('eng');
		await sheet.getByLabel('Development process').selectOption({ label: 'Feature delivery' });
		await expect(sheet.getByTestId('process-preview')).toContainText(
			'Create the roles Product lead, Engineering lead'
		);
		await expect(sheet.getByTestId('process-preview')).toContainText(
			'Use the existing roles Software Engineer'
		);
		await expect(sheet.getByTestId('process-preview')).toContainText('Nobody is hired or started');
		await sheet.getByRole('button', { name: 'Create project' }).click();
		await expect(page).toHaveURL(/\/projects\/[0-9A-Za-z-]+$/);
		await expect(page.locator('#project-heading')).toContainText('Reservations');
		await expect(page.locator('#project-heading')).toBeFocused();
		await expect(page).toHaveTitle('Reservations · Projects · Agentic Org');
		await expect(page.getByText(/Created Reservations \(P-\d+\)\./)).toBeVisible();
		await expect(page.getByTestId('owner-team')).toContainText('Accountable team: Reservations');
		await expect(page.getByTestId('process-line')).toContainText('Follows Feature delivery');
		// The shell's navigation list is read once per organization (src/routes/(shell)/+layout.svelte), so it does not
		// list the new project until the page reloads: a shared-file change reported to the lead.
		await expect(
			page.getByTestId('owner-team').getByRole('link', { name: 'Reservations' })
		).toHaveAttribute('href', /\/teams\//);
	}
);

test('PROJ-2: a taken name is refused on its field, and an existing team can be chosen', async ({
	page
}) => {
	await open(page, '/projects?new&team=platform');
	const sheet = page.getByRole('dialog', { name: 'New project' });
	await expect(sheet.getByRole('radio', { name: 'An existing team' })).toBeChecked();
	await expect(sheet.locator('#new-project-team')).toHaveValue('platform');
	await sheet.getByRole('textbox', { name: 'Name' }).fill('Atlas');
	await sheet.getByLabel('Slug').fill('atlas-two');
	await sheet.getByRole('button', { name: 'Create project' }).click();
	await expect(
		sheet.getByText("There's already a project called Atlas. Choose another name.")
	).toBeVisible();
	await expect(sheet.getByRole('textbox', { name: 'Name' })).toBeFocused();
	await expect(sheet.getByRole('textbox', { name: 'Name' })).toHaveAttribute(
		'aria-invalid',
		'true'
	);
});

test(
	'PROJ-3: a project page has its header, the served tabs in order, and an Overview of real figures',
	{ tag: '@journey' },
	async ({ page }) => {
		await open(page);
		await row(page, 'beacon').getByRole('link', { name: 'Beacon' }).click();
		await expect(page).toHaveURL(/\/projects\/beacon$/);
		await expect(page).toHaveTitle('Beacon · Projects · Agentic Org');
		await expect(page.getByRole('link', { name: 'Projects', exact: true }).last()).toBeVisible();
		const header = page.getByTestId('project-header');
		await expect(header.locator('h2')).toContainText('Beacon');
		await expect(header).toContainText('P-8');
		await expect(header.getByTestId('owner-team')).toContainText('Accountable team: Web');
		await expect(header.getByTestId('contributors')).toContainText('Contributors: Design');
		await expect(header).toContainText('6 of 12 tasks done');
		await expect(tabs(page)).toHaveText([
			'Overview',
			'Work',
			'Features',
			'Releases',
			'Docs',
			'Team',
			'Activity',
			'Conversations',
			'Settings'
		]);
		await expect(tabs(page).first()).toHaveAttribute('aria-current', 'page');
		const figures = page.getByTestId('project-figures');
		await expect(figures).toContainText('Open tasks');
		await expect(figures).toContainText('1 blocked');
		// The features line, word for word (features-ux "Overview tile").
		await expect(page.getByTestId('features-line')).toHaveText(
			'Features: 2 in progress · 1 planned · 5 live'
		);
		await expect(figures).toContainText('1 critical, 2 major, 1 trivial, 1 not set');
		await expect(figures).toContainText('Investment');
		await expect(page.getByRole('heading', { name: 'Brief' })).toBeVisible();
		await expect(page.getByText('Pay in two taps on a phone')).toBeVisible();
		const latest = page.locator('section[aria-labelledby=latest-heading] li');
		await expect(latest).toHaveCount(5);
		await shot(page, 'project-overview');

		await tabs(page).filter({ hasText: 'Team' }).click();
		await expect(page).toHaveURL(/\/projects\/beacon\/team$/);
		await expect(page).toHaveTitle('Team · Beacon · Projects · Agentic Org');
		await expect(page.locator('#project-tab-heading')).toBeFocused();
		await expect(page.locator('[data-team=web]')).toContainText('Accountable');
		await expect(page.locator('[data-team=design]')).toContainText('Contributor');
		await expect(page.getByRole('link', { name: 'Noor Haddad' })).toBeVisible();

		await tabs(page).filter({ hasText: 'Work' }).click();
		await expect(page).toHaveURL(/\/projects\/beacon\/work$/);
		// Step 5 Work tab shows the unified Work queue scoped to this project: Tasks, Bugs, Requests.
		const workViews = page.getByRole('tablist', { name: 'Work', exact: true });
		await expect(workViews.getByRole('tab')).toHaveText([/^Tasks/, /^Bugs/, /^Requests/]);
		await expect(workViews.getByRole('tab', { name: /^Tasks/ })).toHaveAttribute(
			'aria-selected',
			'true'
		);

		await tabs(page).filter({ hasText: 'Activity' }).click();
		await expect(
			page.getByText('assigned T-A40 Split the checkout service to Web · Sam Okafor')
		).toBeVisible();
		await tabs(page).filter({ hasText: 'Conversations' }).click();
		await expect(page.getByRole('link', { name: '#web' })).toBeVisible();
	}
);

test('PRJ-2: empty Activity, Conversations, brief and no development process say so honestly', async ({
	page
}) => {
	await open(page, '/projects/scratch');
	await expect(page.getByTestId('no-brief')).toContainText('No brief yet');
	await expect(page.getByTestId('process-line')).toContainText('None');
	await expect(page.getByText('No activity yet.')).toBeVisible();
	await visit(page, '/projects/scratch/conversations');
	await expect(page.getByRole('heading', { name: 'No conversations yet' })).toBeVisible();
	await visit(page, '/projects/scratch/activity');
	await expect(page.getByRole('heading', { name: 'No activity yet' })).toBeVisible();
});

test('PRJ-3: Settings renames, changes the accountable team after a preview, and keeps a branch per repository', async ({
	page
}) => {
	await open(page, '/projects/beacon/settings');
	await expect(page).toHaveTitle('Settings · Beacon · Projects · Agentic Org');
	const name = page.locator('#settings-name');
	await name.fill('');
	await page
		.locator('section[aria-labelledby=settings-name-heading]')
		.getByRole('button', { name: 'Save' })
		.click();
	await expect(page.getByText('Enter a name for the project.')).toBeVisible();
	await expect(name).toBeFocused();
	await name.fill('Beacon checkout');
	await page
		.locator('section[aria-labelledby=settings-name-heading]')
		.getByRole('button', { name: 'Save' })
		.click();
	await expect(page.locator('#project-heading')).toContainText('Beacon checkout');

	await page.locator('#settings-owner').selectOption('platform');
	const teams = page.locator('section[aria-labelledby=settings-teams-heading]');
	await teams.getByRole('button', { name: 'Save' }).click();
	const dialog = page.getByRole('dialog', {
		name: 'Make Platform accountable for Beacon checkout?'
	});
	await expect(dialog.getByTestId('owner-preview')).toHaveText(
		'14 tasks move their accountable team from Web to Platform, with history.'
	);
	await expect(dialog).toContainText('Assigned teams and assignees stay as they are.');
	await dialog.getByRole('button', { name: 'Change accountable team' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByTestId('owner-team')).toContainText('Accountable team: Platform');

	const repos = page.locator('section[aria-labelledby=settings-repositories-heading]');
	await expect(repos.locator('[data-repository]')).toHaveCount(2);
	await page.locator('#settings-add-repository').selectOption({ label: 'infra' });
	await repos.getByRole('button', { name: 'Add', exact: true }).click();
	const branch = repos.locator('[data-repository] input').last();
	await expect(branch).toBeFocused();
	await branch.fill('release/../x');
	await repos.getByRole('button', { name: 'Save' }).click();
	await expect(repos.getByText(/not a valid branch name/)).toBeVisible();
	await expect(branch).toBeFocused();
	await branch.fill('deploy');
	await repos.getByRole('button', { name: 'Save' }).click();
	await expect(repos.getByText('Saved', { exact: true })).toBeVisible();
	expect(
		await repos
			.locator('[data-repository] input')
			.evaluateAll((inputs) => inputs.map((input) => (input as HTMLInputElement).value))
	).toEqual(['main', 'release/beacon', 'deploy']);

	const process = page.locator('#settings-process-select');
	await process.selectOption({ label: 'Desired State Specification' });
	await expect(page.getByTestId('process-preview')).toContainText('Choosing it will');
	await page
		.locator('section[aria-labelledby=settings-process-heading]')
		.getByRole('button', { name: 'Save' })
		.click();
	await expect(
		page
			.locator('section[aria-labelledby=settings-process-heading]')
			.getByText('Saved', { exact: true })
	).toBeVisible();
	await shot(page, 'project-settings');
});

test('PRJ-4: a newer revision keeps what was typed and says so', async ({ page }) => {
	await open(page, '/projects/atlas/settings?mock=conflict');
	await page.locator('#settings-name').fill('Atlas pipeline');
	await page
		.locator('section[aria-labelledby=settings-name-heading]')
		.getByRole('button', { name: 'Save' })
		.click();
	await expect(page.getByRole('alert')).toContainText(
		"Atlas changed while you were editing. Here's the latest; what you typed is kept."
	);
	await expect(page.locator('#settings-name')).toHaveValue('Atlas pipeline');
});

test('PROJ states: loading, error with Retry, empty, unavailable, not found', async ({ page }) => {
	await open(page, '/projects?mock=slow');
	await expect(
		page.getByRole('status', { name: /Loading projects/ }).or(page.getByText('Loading projects'))
	).toHaveCount(1);
	await visit(page, '/projects?mock=fails');
	await expect(page.getByRole('alert')).toContainText("Couldn't load the projects");
	await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();
	await visit(page, '/projects?mock=empty');
	await expect(page.getByRole('heading', { name: 'No projects yet' })).toBeVisible();
	await expect(page.getByRole('button', { name: 'New project' })).toHaveCount(1);
	await visit(page, '/projects?mock=unavailable');
	await expect(
		page.getByText("This server doesn't serve projects yet.", { exact: false })
	).toBeVisible();
	await visit(page, '/projects/nope');
	await expect(page.getByRole('heading', { name: 'Project not found' })).toBeVisible();
	await expect(page).toHaveTitle('Project not found · Projects · Agentic Org');
	await expect(page.getByRole('link', { name: 'Projects', exact: true }).last()).toBeVisible();
});

test('PROJ access: a viewer without rights sees no New project, no amounts and read-only Settings; the last copy pauses changes', async ({
	page
}) => {
	await open(page, '/projects?mock=viewer');
	await expect(row(page, 'beacon')).toBeVisible();
	await expect(page.getByRole('button', { name: 'New project' })).toHaveCount(0);
	await expect(page.getByTestId('investment')).toHaveCount(0);
	await expect(page.getByText('≈ $')).toHaveCount(0);
	await visit(page, '/projects/beacon/settings?mock=viewer');
	await expect(page.getByTestId('settings-locked')).toContainText("Only the organization's owner");
	await expect(page.locator('#settings-name')).toBeDisabled();
	await expect(page.getByRole('button', { name: 'Save' })).toHaveCount(0);

	await visit(page, '/projects?mock=stale');
	await expect(page.getByTestId('stale')).toContainText(
		"Can't reach HQ. Showing the last copy from"
	);
	await expect(page.getByRole('button', { name: 'New project' })).toBeDisabled();
	await expect(page.getByText("HQ can't be reached, so changes are paused.")).toBeVisible();
});

test('PROJ layout: no sideways scroll at 320 px with 200% text; 44 px targets below 640 px', async ({
	page
}) => {
	await page.setViewportSize({ width: 320, height: 800 });
	await open(page);
	await page.addStyleTag({ content: 'html { font-size: 200%; }' });
	await expect(row(page, 'beacon')).toBeVisible();
	await noSidewaysScroll(page);
	const create = page.getByRole('button', { name: 'New project' });
	expect((await create.boundingBox())!.height).toBeGreaterThanOrEqual(44);
	for (const path of ['/projects/beacon', '/projects/beacon/team', '/projects/beacon/settings']) {
		await visit(page, path);
		await page.addStyleTag({ content: 'html { font-size: 200%; }' });
		await expect(page.locator('#project-heading')).toBeVisible();
		await noSidewaysScroll(page);
	}
	for (const link of await tabs(page).all())
		expect((await link.boundingBox())!.height).toBeGreaterThanOrEqual(44);
});

// ── Review fixes (P2) ─────────────────────────────────────────────────────────────────────────────────────────────

test('PROJ-2: a lost answer to Create is checked by the same operation, never sent again blind', async ({
	page
}) => {
	await open(page, '/projects?mock=create-unknown');
	await page.getByRole('button', { name: 'New project' }).click();
	const sheet = page.getByRole('dialog', { name: 'New project' });
	await sheet.getByRole('textbox', { name: 'Name' }).fill('Lantern');
	await sheet.getByRole('button', { name: 'Create project' }).click();
	await expect(sheet.getByTestId('create-outcome')).toContainText(
		"Couldn't confirm whether Lantern was created"
	);
	// Nothing can be changed or sent again until HQ says what became of it.
	await expect(sheet.getByRole('textbox', { name: 'Name' })).toBeDisabled();
	await expect(sheet.getByRole('button', { name: 'Create project' })).toHaveCount(0);
	// Closing and reopening keeps the unresolved attempt, not a fresh one.
	await sheet.getByRole('button', { name: 'Cancel' }).click();
	await page.getByRole('button', { name: 'New project' }).click();
	await expect(sheet.getByRole('textbox', { name: 'Name' })).toHaveValue('Lantern');
	await sheet.getByRole('button', { name: 'Check status' }).click();
	await expect(page).toHaveURL(/\/projects\/[0-9A-Za-z-]+$/);
	await expect(page.locator('#project-heading')).toContainText('Lantern');
	await page.getByRole('link', { name: 'Projects', exact: true }).last().click();
	await expect(page.getByTestId('project-row').filter({ hasText: 'Lantern' })).toHaveCount(1);
});

test('PROJ-2: the new team goes where the person chooses, and Create waits for the teams', async ({
	page
}) => {
	await open(page, '/projects?mock=slow');
	await page.getByRole('button', { name: 'New project' }).click();
	const sheet = page.getByRole('dialog', { name: 'New project' });
	await sheet.getByRole('textbox', { name: 'Name' }).fill('Racer');
	const create = sheet.getByRole('button', { name: 'Create project' });
	await expect(create).toBeDisabled();
	await expect(sheet.getByText('Reading the teams first')).toBeVisible();
	await expect(sheet.locator('#new-project-parent')).toHaveValue('eng', { timeout: 10_000 });
	await sheet.locator('#new-project-parent').selectOption('platform');
	await expect(create).toBeEnabled();
	await create.click();
	await expect(page.locator('#project-heading')).toContainText('Racer', { timeout: 20_000 });
	await page.getByTestId('owner-team').getByRole('link', { name: 'Racer' }).click();
	await expect(page.getByText(/Part of\s+Platform/)).toBeVisible({ timeout: 20_000 });
});

test('PROJ-2: Back with New project open closes the sheet only and returns focus', async ({
	page
}) => {
	await open(page, '/projects/beacon');
	await page.getByRole('link', { name: 'Projects', exact: true }).last().click();
	await expect(page).toHaveURL(/\/projects$/);
	const opener = page.getByRole('button', { name: 'New project' });
	await opener.click();
	await expect(page.getByRole('dialog', { name: 'New project' })).toBeVisible();
	await page.goBack();
	await expect(page.getByRole('dialog', { name: 'New project' })).toHaveCount(0);
	await expect(page).toHaveURL(/\/projects$/);
	await expect(opener).toBeFocused();
});

test('PRJ-3: the accountable team and the contributors change in one save, whatever revision HQ answers', async ({
	page
}) => {
	await open(page, '/projects/atlas/settings');
	await page.locator('#settings-owner').selectOption('web');
	await page
		.locator('section[aria-labelledby=settings-teams-heading]')
		.getByRole('checkbox', { name: /Design/ })
		.check();
	await page
		.locator('section[aria-labelledby=settings-teams-heading]')
		.getByRole('button', { name: 'Save' })
		.click();
	const dialog = page.getByRole('dialog', { name: 'Make Web accountable for Atlas?' });
	// The confirmation opens only once the number of tasks that move is known.
	await expect(dialog.getByTestId('owner-preview')).toHaveText(
		'18 tasks move their accountable team from Platform to Web, with history.'
	);
	await dialog.getByRole('button', { name: 'Change accountable team' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByTestId('owner-team')).toContainText('Accountable team: Web');
	await expect(page.getByTestId('contributors')).toContainText('Design');
	await expect(
		page
			.locator('section[aria-labelledby=settings-teams-heading]')
			.getByText('Saved', { exact: true })
	).toBeVisible();
});

test("PROJ-1: a project manager sees the investment of the projects they lead, and no other's", async ({
	page
}) => {
	await open(page, '/projects?mock=manager');
	await expect(row(page, 'beacon').getByTestId('investment')).toContainText('≈ $');
	await expect(row(page, 'atlas').getByTestId('investment')).toContainText('Not shown');
	await expect(row(page, 'atlas').getByTestId('investment')).not.toContainText('$');
	await visit(page, '/projects/beacon?mock=manager');
	await expect(page.getByTestId('project-figures')).toContainText('Investment');
	await visit(page, '/projects/atlas?mock=manager');
	await expect(page.getByTestId('project-figures')).not.toContainText('Investment');
});

test('PROJ-2: New project opens from the New menu, Home and a team with no projects', async ({
	page
}) => {
	await open(page, '/home');
	await page.getByRole('button', { name: 'New', exact: true }).click();
	await page.getByRole('menuitem', { name: 'Project', exact: true }).click();
	const sheet = page.getByRole('dialog', { name: 'New project' });
	await expect(sheet).toBeVisible();
	await expect(page).toHaveURL(/\/projects$/);
	await sheet.getByRole('button', { name: 'Cancel' }).click();

	await visit(page, '/home?mock=noprojects');
	await page.getByTestId('create-project').click();
	await expect(sheet).toBeVisible();
	await expect(sheet.getByRole('radio', { name: /New team/ })).toBeChecked();

	await visit(page, '/teams/web/projects?mock=empty');
	await page.getByRole('link', { name: 'New project' }).click();
	await expect(sheet).toBeVisible();
	await expect(sheet.getByRole('radio', { name: 'An existing team' })).toBeChecked();
	await expect(sheet.locator('#new-project-team')).toHaveValue('web');
});

test('PROJ-Activity: Project Activity and Overview latest activity list newest first', async ({
	page
}) => {
	await open(page, '/projects/beacon/activity?mock=oldest-first');
	const items = page.locator('section[aria-labelledby=project-tab-heading] ul > li');
	await expect(items).toHaveCount(7);
	const times = await items
		.locator('time')
		.evaluateAll((els) => els.map((el) => Date.parse(el.getAttribute('datetime') ?? '')));
	for (let i = 1; i < times.length; i++) expect(times[i - 1]).toBeGreaterThanOrEqual(times[i]);
	await expect(items.first()).toContainText('assigned T-A40 Split the checkout service');
	await expect(items.last()).toContainText('updated the brief');

	await visit(page, '/projects/beacon?mock=oldest-first');
	const latest = page.locator('section[aria-labelledby=latest-heading] li');
	await expect(latest).toHaveCount(5);
	await expect(latest.first()).toContainText('assigned T-A40 Split the checkout service');
});

test('PROJ-Work: Work tabs have aria-controls, arrow-key navigation, and work at 320 px with 200% text', async ({
	page
}) => {
	await open(page, '/projects/beacon/work');
	const tablist = page.getByRole('tablist', { name: 'Work', exact: true });
	const tasksTab = tablist.getByRole('tab', { name: /^Tasks/ });
	const bugsTab = tablist.getByRole('tab', { name: /^Bugs/ });
	const requestsTab = tablist.getByRole('tab', { name: /^Requests/ });
	const panel = page.getByRole('tabpanel');

	const panelId = await panel.getAttribute('id');
	const tasksId = await tasksTab.getAttribute('id');
	const bugsId = await bugsTab.getAttribute('id');
	const requestsId = await requestsTab.getAttribute('id');
	expect(panelId).toBeTruthy();
	await expect(tasksTab).toHaveAttribute('aria-controls', panelId!);
	await expect(bugsTab).toHaveAttribute('aria-controls', panelId!);
	await expect(requestsTab).toHaveAttribute('aria-controls', panelId!);
	await expect(tasksTab).toHaveAttribute('aria-selected', 'true');
	await expect(tasksTab).toHaveAttribute('tabindex', '0');
	await expect(bugsTab).toHaveAttribute('tabindex', '-1');
	await expect(panel).toHaveAttribute('aria-labelledby', tasksId!);

	await tasksTab.focus();
	await page.keyboard.press('Shift+ArrowRight');
	await page.keyboard.press('Control+ArrowRight');
	await expect(tasksTab).toBeFocused();
	await expect(tasksTab).toHaveAttribute('aria-selected', 'true');

	await page.keyboard.press('ArrowRight');
	await expect(bugsTab).toBeFocused();
	await expect(bugsTab).toHaveAttribute('aria-selected', 'true');
	await expect(bugsTab).toHaveAttribute('tabindex', '0');
	await expect(tasksTab).toHaveAttribute('tabindex', '-1');
	await expect(panel).toHaveAttribute('aria-labelledby', bugsId!);

	await page.keyboard.press('ArrowRight');
	await expect(requestsTab).toBeFocused();
	await expect(requestsTab).toHaveAttribute('aria-selected', 'true');
	await expect(panel).toHaveAttribute('aria-labelledby', requestsId!);

	await page.keyboard.press('ArrowRight');
	await expect(tasksTab).toBeFocused();
	await expect(tasksTab).toHaveAttribute('aria-selected', 'true');

	await page.keyboard.press('ArrowLeft');
	await expect(requestsTab).toBeFocused();
	await expect(requestsTab).toHaveAttribute('aria-selected', 'true');

	await page.keyboard.press('Home');
	await expect(tasksTab).toBeFocused();
	await page.keyboard.press('End');
	await expect(requestsTab).toBeFocused();

	await page.setViewportSize({ width: 320, height: 800 });
	await page.addStyleTag({ content: 'html { font-size: 200%; }' });
	await expect(bugsTab).toBeVisible();
	expect((await bugsTab.boundingBox())!.height).toBeGreaterThanOrEqual(44);
	await noSidewaysScroll(page);

	await tasksTab.click();
	await expect(tasksTab).toHaveAttribute('aria-selected', 'true');
	expect((await tasksTab.boundingBox())!.height).toBeGreaterThanOrEqual(44);
	await noSidewaysScroll(page);
});
