import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import type { OfficeInfo, StatusUpdate } from '../src/lib/proto';

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
	await expect(rows(page).first()).toContainText("This office's key isn't available on this Mac");
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
	await expect(sheet.getByRole('button', { name: 'Close', exact: true })).toBeFocused();
});

test('OFC-11: a missing key says Try again', async ({ page }) => {
	await openOffices(page, 'key_missing');
	await rows(page).first().getByRole('button').first().click();
	await expect(page.getByRole('dialog').getByTestId('restart-office')).toHaveText('Try again');
});

test('OFC-11: a host that is still starting says so and the button comes back', async ({ page }) => {
	await openOffices(page, 'crashed');
	await visit(page, '/offices?restart=not_ready');
	await rows(page).first().getByRole('button').first().click();
	const sheet = page.getByRole('dialog');
	await sheet.getByTestId('restart-office').click();
	await expect(sheet.getByTestId('restart-error')).toHaveText('The office host is still starting. Try again in a moment.');
	await expect(sheet.getByTestId('restart-office')).toHaveText('Restart office');
	await expect(sheet.getByTestId('restart-office')).toBeEnabled();
});

test('OFC-11: an accepted restart keeps the spinner while the office is still stopped', async ({ page }) => {
	await openOffices(page, 'crashed');
	await visit(page, '/offices?restart=stuck');
	await rows(page).first().getByRole('button').first().click();
	const sheet = page.getByRole('dialog');
	await sheet.getByTestId('restart-office').click();
	await expect(sheet.getByTestId('restart-office')).toHaveText('Restarting…');
	await page.waitForTimeout(5000); // several status re-reads later
	await expect(sheet.getByTestId('restart-office')).toHaveText('Restarting…');
	await expect(rows(page).first()).toContainText('Stopped');
});

test('OFC-11: a status change the host pushes shows at once, without waiting for the next check', async ({ page }) => {
	await openOffices(page);
	const hq = rows(page).first();
	await expect(hq).toContainText('Online');
	const officeId = (await hq.getAttribute('data-office')) ?? '';
	const office = { id: officeId, organization_id: officeId.replace('office-', ''), name: 'HQ', role: 'hq', host: 'This Mac' };
	const push = (update: object) => page.evaluate((u) => (window as unknown as { __mockOfficeStatus: (u: object) => void }).__mockOfficeStatus(u), update);
	await push({ kind: 'office', office: { ...office, status: 'unavailable', status_reason: 'crashed' } });
	await expect(hq).toContainText('Stopped');
	await expect(hq).toContainText('It stopped unexpectedly');
	await push({ kind: 'office', office: { ...office, organization_id: 'another', status: 'active' } }); // another organization: nothing changes
	await expect(hq).toContainText('Stopped');
	await push({ kind: 'offices', offices: [{ ...office, status: 'active' }] }); // the whole list, as after a reconnect
	await expect(hq).toContainText('Online');
	await expect(hq).not.toContainText('It stopped unexpectedly');
});

async function pushOffice(page: Page, changes: Partial<OfficeInfo>) {
	await page.evaluate(async (changes) => {
		const path = '/e2e/fakes/office-api.ts';
		const { officeApi } = await import(path);
		const connection = (await officeApi.connections()).at(-1)!;
		const { __mockOfficeStatus: push } = window as unknown as { __mockOfficeStatus: (update: StatusUpdate) => void };
		push({ kind: 'office', office: { ...connection.office, ...changes } });
	}, changes);
}

test('OFC-13 / BUG-23: keyboard focus stays on Restart while it runs and after a refusal', async ({ page }) => {
	await openOffices(page, 'crashed');
	await visit(page, '/offices?restart=not_ready');
	await page.evaluate(async () => {
		const path = '/e2e/fakes/office-api.ts';
		const { officeApi } = await import(path);
		const restart = officeApi.restartOffice.bind(officeApi);
		let release!: () => void;
		const gate = new Promise<void>((resolve) => { release = resolve; });
		const held = { release, calls: 0 };
		Object.assign(window, { __heldRestart: held });
		officeApi.restartOffice = async (id: string) => { held.calls++; await gate; return restart(id); };
	});
	await expect(rows(page).first()).toContainText('Stopped');
	await rows(page).first().getByRole('button').press('Enter');
	const sheet = page.getByRole('dialog');
	const restart = sheet.getByTestId('restart-office');
	await restart.press('Enter');
	await expect(restart).toHaveText('Restarting…');
	await expect(restart).toHaveAttribute('aria-disabled', 'true');
	await shot(page, 'office-restart-keyboard-busy');
	await expect.soft(restart).toBeFocused();
	await page.keyboard.press('Enter');
	const calls = await page.evaluate(() => {
		const held = (window as unknown as { __heldRestart: { release: () => void; calls: number } }).__heldRestart;
		held.release();
		return held.calls;
	});
	expect(calls).toBe(1);
	await expect(sheet.getByTestId('restart-error')).toContainText('The office host is still starting');
	await expect(restart).toHaveText('Restart office');
	await shot(page, 'office-restart-keyboard-refused');
	await expect(restart).toBeFocused();
});

test('OFC-11 / BUG-24: an unsupported release gives local repair steps for the selected office', async ({ page, context }) => {
	await context.grantPermissions(['clipboard-read', 'clipboard-write']);
	await openOffices(page);
	const row = rows(page).first();
	await expect(row).toContainText('Online');
	await page.setViewportSize({ width: 320, height: 700 });
	const host = 'northwindregionalagentservicesproductionheadquarterscluster.example';
	await pushOffice(page, { host, status: 'unavailable', status_reason: 'unsupported_version' });
	const reason = `The release components on ${host} don't match. Run diagnostics and repair the selected instance there.`;
	await expect(row).toContainText(reason);
	const opener = row.getByRole('button');
	expect((await opener.boundingBox())?.width).toBeGreaterThanOrEqual(200);
	await shot(page, 'office-incompatible-row-320');
	await opener.click();
	const sheet = page.getByRole('dialog');
	await expect(sheet).toHaveCSS('width', '320px');
	await shot(page, 'office-incompatible-release');
	await expect(sheet.getByTestId('office-why')).toHaveText(reason);
	await expect(sheet.getByTestId('restart-office')).toHaveCount(0);
	await expect(sheet.getByRole('button', { name: 'Copy details', exact: true })).toBeVisible();
	const show = sheet.getByRole('button', { name: 'Show local CLI repair instructions', exact: true });
	await expect(show).toHaveAttribute('aria-expanded', 'false');
	const id = await show.getAttribute('aria-controls');
	expect(id).toBeTruthy();
	await show.press('Enter');
	const hide = sheet.getByRole('button', { name: 'Hide repair instructions', exact: true });
	await expect(hide).toBeFocused();
	await expect(hide).toHaveAttribute('aria-expanded', 'true');
	const instructions = sheet.locator(`[id="${id}"]`);
	await expect(instructions.getByText(`On ${host}, run:`, { exact: true })).toBeVisible();
	await expect(instructions.locator('pre code')).toHaveText('agentic-org doctor');
	await expect(instructions).toContainText('It checks the Agentic Org instance on that machine and says what to repair. Follow its steps there. This page updates on its own when the office is back.');
	await expect(instructions).toContainText(`If ${host} runs more than one instance, add --name <instance>.`);
	await expect(instructions.locator('p code')).toHaveText('--name <instance>');
	expect(await sheet.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
	await shot(page, 'office-local-cli-repair');
	await instructions.getByRole('button', { name: 'Copy command', exact: true }).click();
	await expect(instructions.getByRole('button', { name: 'Copied', exact: true })).toBeVisible();
	expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('agentic-org doctor');
	await shot(page, 'office-repair-command-copied');
	const details = sheet.getByRole('button', { name: 'Copy details', exact: true });
	await details.scrollIntoViewIfNeeded();
	await expect(details).toBeInViewport();
	await shot(page, 'office-repair-steps-end-320');
	await hide.press('Enter');
	await expect(show).toBeFocused();
	await expect(instructions).not.toBeVisible();
});

test('BUG-25: the Offices footer text meets 4.5:1 contrast', async ({ page }) => {
	await openOffices(page);
	const footer = page.getByText('Shown from this Mac.', { exact: true });
	await shot(page, 'offices-footer-contrast');
	const contrast = await footer.evaluate((el) => {
		const canvas = document.createElement('canvas');
		canvas.width = canvas.height = 1;
		const ctx = canvas.getContext('2d')!;
		const ancestors = [];
		for (let node: Element | null = el; node; node = node.parentElement) ancestors.unshift(node);
		for (const node of ancestors) {
			ctx.fillStyle = getComputedStyle(node).backgroundColor;
			ctx.fillRect(0, 0, 1, 1);
		}
		const luminance = () => [...ctx.getImageData(0, 0, 1, 1).data].slice(0, 3)
			.map((v) => v / 255)
			.map((v) => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4)
			.reduce((sum, v, i) => sum + v * [.2126, .7152, .0722][i], 0);
		const bg = luminance();
		ctx.fillStyle = getComputedStyle(el).color;
		ctx.fillRect(0, 0, 1, 1);
		const fg = luminance();
		return (Math.max(fg, bg) + .05) / (Math.min(fg, bg) + .05);
	});
	expect(contrast).toBeGreaterThanOrEqual(4.5);
});

test('OFC-11 / BUG-26: a new restart countdown uses the time of the status push', async ({ page }) => {
	await page.clock.install();
	await openOffices(page);
	const row = rows(page).first();
	await expect(row).toContainText('Online');
	await page.clock.pauseAt(new Date((await page.evaluate(() => Date.now())) + 1_000));
	await page.clock.runFor(10_000); // Establish the existing countdown clock's last sample.
	const current = (await page.evaluate(() => Date.now())) + 4_000;
	await page.clock.setSystemTime(new Date(current)); // A push arrives before the next timer tick.
	const pending = { status: 'offline', status_reason: 'crashed', next_restart_at: new Date(current + 45_000).toISOString() } as const;
	await pushOffice(page, pending);
	await expect(row).toContainText('Stopped');
	await shot(page, 'office-countdown-new-push', { settle: false });
	await expect(row).toContainText('Restarting in 45 s');
	await expect(row.getByRole('button')).toHaveAccessibleName(/restarting shortly/);
	await page.clock.setSystemTime(new Date(current + 2_000));
	await pushOffice(page, pending); // The same deadline is reported again before a tick.
	await expect(row).toContainText('Restarting in 45 s');
	await page.clock.runFor(10_000);
	await expect(row).toContainText(/Restarting in (?:3\d|4[0-3]) s/);
	await expect(row.getByRole('button')).toHaveAccessibleName(/restarting shortly/);
});

test('OFC-13 / BUG-27: the office sheet has one Close and returns focus to the opener', async ({ page }) => {
	await openOffices(page);
	const opener = rows(page).first().getByRole('button');
	await opener.press('Enter');
	const sheet = page.getByRole('dialog');
	await expect(sheet).toBeVisible();
	await shot(page, 'office-details-single-close');
	const close = sheet.getByRole('button', { name: 'Close', exact: true });
	await expect(close).toHaveCount(1);
	await close.press('Enter');
	await expect(sheet).not.toBeVisible();
	await expect(opener).toBeFocused();
});

test('OFC-13 / BUG-27: pointer activation without focus still returns to the office row', async ({ page, browserName }) => {
	await openOffices(page);
	const opener = rows(page).first().getByRole('button');
	await page.getByRole('link', { name: 'Offices', exact: true }).focus();
	// WebKit doesn't focus a button on pointer activation. Exercise the same path in Chromium.
	if (browserName === 'webkit') await opener.click();
	else await opener.dispatchEvent('click');
	const sheet = page.getByRole('dialog');
	await expect(sheet).toBeVisible();
	await sheet.getByRole('button', { name: 'Close', exact: true }).click();
	await expect(sheet).not.toBeVisible();
	await expect(opener).toBeFocused();
	await shot(page, 'office-details-pointer-return-focus');
});
