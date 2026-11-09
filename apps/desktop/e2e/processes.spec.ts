import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function open(page: Page, query = '') {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, `/processes${query}`);
	await expect(page.getByTestId('process-list').or(page.getByTestId('no-processes'))).toBeVisible();
}
const row = (page: Page, key: string) => page.locator(`[data-testid=process-row][data-key="${key}"]`);

test('PROC-9: the organization’s processes, and the screen says it is sample data', async ({ page }) => {
	await open(page);
	await expect(page.getByTestId('sample-data-note')).toContainText('Sample data');
	await expect(page.getByRole('heading', { name: 'Processes · Organization' })).toBeVisible();
	await expect(page.getByTestId('process-row')).toHaveCount(4);
	await expect(row(page, 'branch-and-merge')).toContainText('Organization v3');
	await expect(row(page, 'security-basics').getByTestId('sealed')).toContainText('Sealed');
	await shot(page, 'processes-org');
});

test('PROC-9: a team’s override that drifted says Parent changed, with the versions', async ({ page }) => {
	await open(page, '?place=team:web');
	const review = row(page, 'code-review');
	await expect(review.getByTestId('kind')).toContainText('Override of code-review@1');
	await expect(review.getByTestId('drift')).toContainText('Parent changed: Organization is now v2');
	await expect(review.getByTestId('drift')).toHaveAttribute('aria-label', 'Parent changed: Organization is now v2');
	await expect(row(page, 'incident-response').getByTestId('kind')).toContainText('Addition');
});

test('PROC-4/PROC-3/PROC-6: a project shows what it turned off, a sealed refusal and the shared project’s conflict', async ({ page }) => {
	await open(page, '?place=project:atl');
	await expect(page.getByTestId('turned-off-row')).toContainText('Turned off for Atlas: we report daily');
	await expect(page.getByTestId('problem').filter({ hasText: "Platform's version of Security basics isn't applied: the organization sealed it. You can add a process instead." })).toBeVisible();
	await expect(page.getByTestId('branch-rule')).toContainText('On');
	await shot(page, 'processes-project');
	await visit(page, '/processes?place=project:bea');
	await expect(page.getByTestId('conflict')).toContainText("Code review: Design and Web disagree. The organization's version applies until the project decides.");
	await expect(page.getByTestId('problem').filter({ hasText: 'Two processes use the key `standup` in Beacon. Neither applies until one is renamed.' })).toHaveCount(2);
	await shot(page, 'processes-conflict');
});

test('PROC-14: a turning-off the author approved herself does not turn the rule off, and says so', async ({ page }) => {
	await open(page, '?place=project:onb');
	await expect(page.getByTestId('branch-rule')).toContainText('On · turning it off needs approval by someone other than its author (hana)');
	await expect(row(page, 'branch-and-merge')).toBeVisible(); // the process is still in force
});

test('PROC-8: a process opens with its text and why: every layer and what happened to it', async ({ page }) => {
	await open(page, '?place=team:web');
	await row(page, 'code-review').click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByRole('heading', { name: 'Code review (Web)' })).toBeVisible();
	const layers = sheet.getByTestId('why-layer');
	await expect(layers).toHaveCount(2);
	await expect(layers.nth(0)).toContainText('Overridden');
	await expect(layers.nth(0)).toContainText('Overridden by team Web.');
	await expect(layers.nth(1)).toContainText('Applies');
	await expect(sheet.getByText('As the organization says, plus a screenshot of any UI change.')).toBeVisible();
	await shot(page, 'processes-why');
});

test('PROC-10: an agent’s page says what it follows and the version it last read, or that it has no current work', async ({ page }) => {
	await open(page, '?place=agent:noor');
	await expect(row(page, 'branch-and-merge').getByTestId('read')).toContainText('Read v2 at');
	await expect(row(page, 'branch-and-merge').getByTestId('read')).toContainText('now v3');
	await expect(row(page, 'my-notes').getByTestId('kind')).toContainText('Addition');
	await expect(page.getByTestId('problem').filter({ hasText: "Private processes only add: My code review isn't applied because it changes an inherited process." })).toBeVisible();
	await visit(page, '/processes?place=agent:sam');
	await expect(page.getByText("No current work. Showing the organization's processes and its own.")).toBeVisible();
});

test('PROC-16: loading shows skeleton rows; an unreachable handbook says so with the last copy’s time; an empty organization invites the first process', async ({ page }) => {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await page.goto('/processes?handbook=down');
	await expect(page.getByTestId('processes-loading')).toBeVisible();
	await expect(page.getByTestId('handbook-down')).toContainText("Can't reach the handbook. Showing the last copy from");
	await visit(page, '/processes?processes=empty');
	await expect(page.getByTestId('no-processes')).toContainText('Write down how your agents work. Start with how changes get reviewed and merged.');
});

test('PROC-18: a level or a kind is a word with an icon, and the place can be chosen from the screen', async ({ page }) => {
	await open(page);
	await expect(page.getByTestId('level-badge').first()).toContainText('Organization');
	await page.getByLabel('Show processes for').selectOption('team:web');
	await expect(page).toHaveURL(/place=team%3Aweb/);
	await expect(page.getByRole('heading', { name: 'Processes · Web (team)' })).toBeVisible();
	await page.getByLabel('Show processes for').selectOption('organization');
	await expect(page.getByRole('heading', { name: 'Processes · Organization' })).toBeVisible();
});
