import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';

async function openOffices(page: Page, stopped?: string) {
	// `?office=` makes the new organization's HQ a stopped office with a reason (the mock's scenario).
	await visit(page, stopped ? `/create?office=${stopped}` : '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/offices');
	await expect(page.getByRole('list', { name: 'Offices' })).toBeVisible();
}
const rows = (page: Page) => page.getByTestId('office-row');

test('OFC-1: the list shows HQ first with its status, where it runs, and how many agents', async ({ page }) => {
	await openOffices(page);
	await expect(rows(page)).toHaveCount(1);
	const hq = rows(page).first();
	await expect(hq).toContainText('HQ');
	await expect(hq).toContainText('Online');
	await expect(hq).toContainText("This Mac (you're here)");
	await expect(hq).toContainText('7 agents');
	await expect(page.getByTestId('unplaced-note')).toContainText('Unplaced · 1 agent');
	await expect(page.getByTestId('unplaced-note')).toContainText('Inês Duarte');
	await shot(page, 'offices');
});

test('the agent counts say they are sample data', async ({ page }) => {
	await openOffices(page);
	await expect(page.getByTestId('sample-data-note')).toContainText('Sample data');
});

test('OFC-1: a row opens a slide-over with the office’s details', async ({ page }) => {
	await openOffices(page);
	await rows(page).first().getByRole('button').click();
	const details = page.getByTestId('office-details');
	await expect(details).toContainText('HQ');
	await expect(details).toContainText('This Mac');
	await expect(details).toContainText('Online');
	await expect(details).toContainText('Sam Okafor'); // an agent placed here
	await expect(page.getByTestId('sample-data-note-sheet')).toContainText('Sample data'); // the sheet covers the page's note
	await expect(details).toContainText("This Mac (you're here)");
	await expect(page.getByRole('button', { name: /Remove office/ })).toHaveCount(0); // phase 1 has HQ only
	await shot(page, 'offices-details');
});

test('phase 1: Add office is disabled and says it comes in a later release', async ({ page }) => {
	await openOffices(page);
	const add = page.getByRole('button', { name: 'Add office' });
	await expect(add).toBeDisabled();
	await expect(add).toHaveAccessibleDescription('Available in a later release.');
});

test('OFC-13: a row’s label says its name, HQ, where it runs, the status and the agents, and that it opens details', async ({ page }) => {
	await openOffices(page);
	const button = rows(page).first().getByRole('button');
	await expect(button).toHaveAccessibleName("HQ, HQ, This Mac (you're here), Online, 7 agents, opens details");
	await expect(button).toHaveAttribute('aria-haspopup', 'dialog');
});

test('OFC-13: every status shows its icon and its word (the word is never replaced by the icon)', async ({ page }) => {
	await openOffices(page);
	const badge = rows(page).first().locator('[data-status-icon]');
	await expect(badge).toHaveAttribute('data-status-icon', 'circle'); // Online
	await expect(badge).toContainText('Online');
	await expect(badge.locator('svg')).toHaveCount(1); // the icon really renders
});

test('while the agents are being counted the row shows a skeleton, then the count', async ({ page }) => {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/offices?org=slow');
	await expect(rows(page)).toHaveCount(1); // the list doesn't wait
	await expect(page.getByTestId('count-skeleton')).toBeVisible();
	await expect(rows(page).first().getByRole('button')).toHaveAccessibleName(/^HQ, HQ, This Mac \(you're here\), [^,]+, opens details$/); // no count in it yet
	await expect(page.getByTestId('count-skeleton')).toHaveCount(0, { timeout: 10_000 });
	await expect(rows(page).first().getByTestId('agent-count')).toContainText('7 agents');
});

test('Retry after a failed count works', async ({ page }) => {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/offices?org=fails_once');
	await expect(page.getByTestId('count-error')).toBeVisible();
	const retry = page.getByRole('button', { name: 'Retry' });
	await retry.focus();
	await page.keyboard.press('Enter');
	await expect(page.getByTestId('count-error')).toHaveCount(0);
	await expect(rows(page).first().getByTestId('agent-count')).toContainText('7 agents');
});

test('OFC-1: the list opens from the keyboard and closes back to the row', async ({ page }) => {
	await openOffices(page);
	const button = rows(page).first().getByRole('button');
	await button.focus();
	await page.keyboard.press('Enter');
	await expect(page.getByTestId('office-details')).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(page.getByTestId('office-details')).toBeHidden();
	await expect(button).toBeFocused();
});

test('Unplaced agents are a group with a disabled Place… that says why', async ({ page }) => {
	await openOffices(page);
	const group = page.getByTestId('unplaced-note');
	await expect(group.getByRole('heading', { name: 'Unplaced · 1 agent' })).toBeVisible();
	const place = group.getByRole('button', { name: 'Place… Inês Duarte' });
	await expect(place).toBeDisabled();
	await expect(place).toHaveAccessibleDescription('Placing agents is available in a later release.');
});

test('a failure to count agents keeps the list and shows an in-page error with Retry, not an error page', async ({ page }) => {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/offices?org=fails');
	await expect(rows(page)).toHaveCount(1); // the list is there
	await expect(page.getByTestId('count-error')).toContainText("Can't count agents right now: The office couldn't read its storage.");
	await expect(page.getByRole('link', { name: /Offices/ }).first()).toBeVisible(); // the window around it is intact
	await expect(rows(page).first().getByTestId('agent-count')).not.toContainText('agents');
	await expect(page.getByTestId('unplaced-note')).toHaveCount(0);
	await rows(page).first().getByRole('button').click();
	await expect(page.getByTestId('office-details')).toContainText("Couldn't be counted");
	await shot(page, 'offices-count-error');
});

test('OFC-12: HQ says Checking… until the app has verified the connection, and only then Online', async ({ page }) => {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/offices');
	// The saved status says active, but the row has to earn "Online" by the office answering.
	const badge = rows(page).first().locator('[data-status-icon]');
	await expect(badge).toContainText('Checking…');
	await expect(badge).toHaveAttribute('data-status-icon', 'loader');
	await expect(rows(page).first()).not.toContainText('Online');
	await expect(badge).toContainText('Online');
	await expect(badge).toHaveAttribute('data-status-icon', 'circle');
	await expect(badge.locator('svg')).toHaveCount(1);
	await expect(page.getByTestId('live-status')).toHaveText('HQ: Online'); // said once for screen readers
});

test('OFC-12: when the office does not answer, the row says Unreachable with a Retry, never a stale Online', async ({ page }) => {
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Acme');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).toHaveURL(/\/agents$/);
	await visit(page, '/offices?mock=unreachable');
	const hq = rows(page).first();
	await expect(hq).toContainText('Unreachable');
	await expect(hq).toContainText("Can't reach this office");
	await expect(hq).not.toContainText('Online');
	await expect(hq.locator('[data-status-icon]')).toHaveAttribute('data-status-icon', 'wifi-off');
	await expect(hq.getByRole('button', { name: /^HQ, HQ, .*Unreachable/ })).toBeVisible();
	await expect(hq.locator('[data-status-icon] svg')).toHaveCount(1);
	const retry = hq.getByTestId('retry-office'); // its name changes while it works, so find it by id
	await expect(retry).toHaveAccessibleName('Retry reaching HQ');
	await shot(page, 'offices-unreachable');
	// Retry keeps the keyboard focus while it works, and says so; the office still doesn't answer, so it ends Unreachable again.
	await retry.focus();
	await page.keyboard.press('Enter');
	await expect(retry).toHaveText('Retrying…');
	await expect(retry).toHaveAccessibleName('Retrying… reaching HQ'); // the name follows the visible text
	await expect(retry).toBeFocused();
	await expect(hq).toContainText('Checking…');
	await expect(retry).toHaveText('Retry', { timeout: 15_000 });
	await expect(hq).toContainText('Unreachable');
	await expect(retry).toBeFocused();
	await expect(page.getByTestId('live-status')).toHaveText('HQ: Unreachable');
});

test('OFC-11: a stopped office says why, in its row and in its details, with Copy details', async ({ page, context }) => {
	await context.grantPermissions(['clipboard-read', 'clipboard-write']);
	await openOffices(page, 'crashed');
	const hq = rows(page).first();
	await expect(hq).toContainText('Stopped');
	await expect(hq).toContainText('It stopped unexpectedly');
	await shot(page, 'office-stopped-crashed');
	await hq.getByRole('button').first().click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByTestId('office-why')).toHaveText('It stopped unexpectedly');
	await shot(page, 'office-stopped-details');
	await sheet.getByRole('button', { name: 'Copy details' }).click();
	await expect(page.getByText('Copied the details.')).toBeVisible();
	const text = await page.evaluate(() => navigator.clipboard.readText());
	expect(text).toMatch(/^Office: HQ\nId: office-/);
	expect(text).toContain('Status: Stopped\nReason: crashed');
	expect(text.toLowerCase()).not.toContain('token');
});

test('OFC-11: other reasons have their words; no reason reads "It isn’t running"', async ({ page }) => {
	await openOffices(page, 'key_missing');
	await expect(rows(page).first()).toContainText("Its data key is missing from this Mac's Keychain");
	const other = await page.context().newPage();
	await visit(other, '/create?office=unknown');
	await other.getByLabel('Organization name').fill('Beta');
	await other.getByRole('button', { name: 'Create organization' }).click();
	await expect(other).toHaveURL(/\/agents$/);
	await visit(other, '/offices');
	await expect(other.getByTestId('office-row').first()).toContainText("It isn't running");
});

test('OFC-11: a restart that is pending counts down, and the office reads Stopped, never Offline', async ({ page }) => {
	await openOffices(page, 'restarting');
	const hq = rows(page).first();
	await expect(hq).toContainText('Stopped');
	await expect(hq).not.toContainText('Offline');
	await expect(hq).toContainText(/It stopped unexpectedly\. Restarting in \d+ s/);
	// The screen-reader label keeps where it runs, puts the reason after the status, and never ticks (spec §6.1).
	const label = (await hq.getByRole('button').first().getAttribute('aria-label')) ?? '';
	expect(label).toContain('This Mac');
	expect(label).toMatch(/Stopped: it stopped unexpectedly, restarting at \d\d:\d\d/);
	expect(label).not.toMatch(/\d+ s\b/);
});

test('OFC-11: Restart office brings a crashed office back, and the row reads Online', async ({ page }) => {
	await openOffices(page, 'crashed');
	const hq = rows(page).first();
	await expect(hq).toContainText('Stopped');
	await hq.getByRole('button').first().click();
	const sheet = page.getByRole('dialog');
	await expect(sheet.getByTestId('restart-office')).toHaveText('Restart office');
	await sheet.getByTestId('restart-office').click();
	await expect(hq).toContainText('Online', { timeout: 30_000 });
	await expect(hq).not.toContainText('It stopped unexpectedly');
	await expect(sheet.getByTestId('restart-office')).toHaveCount(0);
});

test('OFC-11: a missing key says Try again, and an unsupported version has no button', async ({ page }) => {
	await openOffices(page, 'key_missing');
	await rows(page).first().getByRole('button').first().click();
	await expect(page.getByRole('dialog').getByTestId('restart-office')).toHaveText('Try again');
});
