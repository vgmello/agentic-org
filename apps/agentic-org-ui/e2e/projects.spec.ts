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
			'Team',
			'Activity',
			'Conversations',
			'Settings'
		]);
		await expect(tabs(page).first()).toHaveAttribute('aria-current', 'page');
		const figures = page.getByTestId('project-figures');
		await expect(figures).toContainText('Open tasks');
		await expect(figures).toContainText('1 blocked');
		await expect(figures).toContainText('2 in progress, 1 planned, 5 live');
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
		await expect(page.getByText("Work isn't built yet")).toBeVisible();
		await expect(page.getByText("It's coming in this release.")).toBeVisible();

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
