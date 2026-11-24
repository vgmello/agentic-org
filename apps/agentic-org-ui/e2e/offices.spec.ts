import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { shot } from './shot';
import type { OfficeInfo, StatusUpdate } from '../src/lib/proto';
import { enterMockedOrganization } from './mock-organization';

async function openOffices(page: Page, stopped?: string) {
	// `?office=` makes the new organization's HQ a stopped office with a reason (the mock's scenario).
	await enterMockedOrganization(page, 'Acme', stopped ? `/create?office=${stopped}` : '/create');
	await expect(page).toHaveURL(/\/teams\/members$/);
	await visit(page, '/offices?offices=empty');
	await expect(page.getByRole('list', { name: 'Offices', exact: true })).toBeVisible();
}
const rows = (page: Page) => page.getByTestId('office-row');

/**
 * Offices sits in the Organization settings frame: from 960 px its 14rem column of section links is beside the
 * fixed 264px ExecutiveSidebar, so the content column starts at 264 + 14rem.
 */
const settingsChrome = (rootFont: number) => 264 + 14 * rootFont;

test(
	'OFC-1: the list shows HQ first with its status, where it runs, and how many agents',
	{ tag: '@journey' },
	async ({ page }) => {
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
	}
);

test('the agent counts say they are sample data', async ({ page }) => {
	await openOffices(page);
	await expect(page.getByTestId('sample-data-note')).toContainText('Sample data');
});

test(
	'OFC-1: a row opens a slide-over with the office’s details',
	{ tag: '@journey' },
	async ({ page }) => {
		await openOffices(page);
		await rows(page).first().getByRole('button').click();
		const details = page.getByTestId('office-details');
		await expect(details).toContainText('HQ');
		await expect(details).toContainText('This Mac');
		await expect(details).toContainText('Online');
		await expect(details).toContainText('Sam Okafor'); // an agent placed here
		await expect(page.getByTestId('sample-data-note-sheet')).toContainText('Sample data'); // the sheet covers the page's note
		await expect(details).toContainText("This Mac (you're here)");
		await expect(page.getByRole('button', { name: /Remove office/ })).toHaveCount(0); // never on HQ (§6.1)
		await shot(page, 'offices-details');
	}
);

test('OFC-7: Add office is the page’s primary action and offers the administrator both places', async ({
	page
}) => {
	await openOffices(page);
	const add = page.getByRole('main').getByRole('button', { name: 'Add office', exact: true });
	await expect(add).toBeEnabled();
	await expect(page.getByTestId('hq-only-hint')).toHaveText(
		'Add an office to run agents on another machine, or to keep a copy of the records there.'
	);
	await add.click();
	const dialog = page.getByRole('dialog');
	await expect(dialog.getByRole('radio', { name: /On this instance/ })).toBeVisible();
	await expect(dialog.getByRole('radio', { name: /On another machine/ })).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(add).toBeFocused();
});

test('OFC-13: a row’s label says its name, HQ, where it runs, the status and the agents, and that it opens details', async ({
	page
}) => {
	await openOffices(page);
	const button = rows(page).first().getByRole('button');
	await expect(button).toHaveAccessibleName(
		"HQ, HQ, This Mac (you're here), Online, 7 agents, opens details"
	);
	await expect(button).toHaveAttribute('aria-haspopup', 'dialog');
});

test('OFC-13: every status shows its icon and its word (the word is never replaced by the icon)', async ({
	page
}) => {
	await openOffices(page);
	const badge = rows(page).first().locator('[data-status-icon]');
	await expect(badge).toHaveAttribute('data-status-icon', 'circle'); // Online
	await expect(badge).toContainText('Online');
	await expect(badge.locator('svg')).toHaveCount(1); // the icon really renders
});

test(
	'OFC-13 BUG-70: while the agents are being counted the row shows a skeleton, then the count',
	{ tag: '@webkit' },
	async ({ page }) => {
		await enterMockedOrganization(page);
		await expect(page).toHaveURL(/\/teams\/members$/);
		await visit(page, '/offices?offices=empty&org=slow');
		await expect(rows(page)).toHaveCount(1); // the list doesn't wait
		await expect(page.getByTestId('count-skeleton')).toBeVisible();
		await shot(page, 'of2-count-loading', { context: rows(page).first(), settle: false });
		await expect(rows(page).first().getByRole('button')).toHaveAccessibleName(
			/^HQ, HQ, This Mac \(you're here\), [^,]+, opens details$/
		); // no count in it yet
		await expect(page.getByTestId('count-skeleton')).toHaveCount(0, { timeout: 10_000 });
		await expect(rows(page).first().getByTestId('agent-count')).toContainText('7 agents');
	}
);

test('Retry after a failed count works', async ({ page }) => {
	await enterMockedOrganization(page);
	await expect(page).toHaveURL(/\/teams\/members$/);
	await visit(page, '/offices?offices=empty&org=fails_once');
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

test('OFC-10: unplaced agents are a group, each with Place… and Offboard…', async ({ page }) => {
	await openOffices(page);
	const group = page.getByTestId('unplaced-note');
	await expect(group.getByRole('heading', { name: 'Unplaced · 1 agent' })).toBeVisible();
	await expect(group.getByRole('button', { name: 'Offboard Inês Duarte…' })).toBeEnabled();
	const place = group.getByRole('button', { name: 'Place Inês Duarte…' });
	await place.click();
	const dialog = page.getByRole('dialog');
	await expect(dialog).toContainText('Inês Duarte');
	await dialog.getByLabel('Office').selectOption({ label: 'HQ' });
	await dialog.getByRole('button', { name: 'Place', exact: true }).click();
	await expect(group).toHaveCount(0); // placed in HQ, the only office
	await expect(rows(page).first().getByTestId('agent-count')).toContainText('8 agents');
});

test(
	'OFC-13 BUG-70: a failure to count agents keeps the list and shows an in-page error with Retry, not an error page',
	{ tag: '@webkit' },
	async ({ page }) => {
		await enterMockedOrganization(page);
		await expect(page).toHaveURL(/\/teams\/members$/);
		await visit(page, '/offices?offices=empty&org=fails');
		await expect(rows(page)).toHaveCount(1); // the list is there
		await expect(page.getByTestId('count-error')).toContainText(
			"Can't count agents right now: The office couldn't read its storage."
		);
		await shot(page, 'of2-count-error', { context: rows(page).first() });
		await expect(page.getByRole('link', { name: /Offices/ }).first()).toBeVisible(); // the window around it is intact
		await expect(rows(page).first().getByTestId('agent-count')).not.toContainText('agents');
		await expect(page.getByTestId('unplaced-note')).toHaveCount(0);
		await rows(page).first().getByRole('button').click();
		await expect(page.getByTestId('office-details')).toContainText("Couldn't be counted");
		await shot(page, 'offices-count-error');
	}
);

test(
	'OFC-12: HQ says Checking… until the app has verified the connection, and only then Online',
	{ tag: '@journey' },
	async ({ page }) => {
		await enterMockedOrganization(page);
		await expect(page).toHaveURL(/\/teams\/members$/);
		await visit(page, '/offices?offices=empty');
		// The saved status says active, but the row has to earn "Online" by the office answering.
		const badge = rows(page).first().locator('[data-status-icon]');
		await expect(badge).toContainText('Checking…');
		await expect(badge).toHaveAttribute('data-status-icon', 'loader');
		await expect(rows(page).first()).not.toContainText('Online');
		await expect(badge).toContainText('Online');
		await expect(badge).toHaveAttribute('data-status-icon', 'circle');
		await expect(badge.locator('svg')).toHaveCount(1);
		await expect(page.getByTestId('live-status')).toHaveText('HQ: Online'); // said once for screen readers
	}
);

test(
	'OFC-12: when the office does not answer, the row says Unreachable with a Retry, never a stale Online',
	{ tag: '@journey' },
	async ({ page }) => {
		await enterMockedOrganization(page);
		await expect(page).toHaveURL(/\/teams\/members$/);
		await visit(page, '/offices?offices=empty&mock=unreachable');
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
	}
);

test('OFC-11: a stopped office says why, in its row and in its details, with Copy details', async ({
	page,
	context
}) => {
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

test('OFC-11: other reasons have their words; no reason reads "It isn’t running"', async ({
	page
}) => {
	await openOffices(page, 'stopped_by_owner');
	await expect(rows(page).first()).toContainText('You stopped it');
	const other = await page.context().newPage();
	await enterMockedOrganization(other, 'Beta', '/create?office=unknown');
	await expect(other).toHaveURL(/\/teams\/members$/);
	await visit(other, '/offices?offices=empty');
	await expect(other.getByTestId('office-row').first()).toContainText("It isn't running");
});

test('OFC-11: a restart that is pending counts down, and the office reads Stopped, never Offline', async ({
	page
}) => {
	await openOffices(page, 'restarting');
	const hq = rows(page).first();
	await expect(hq).toContainText('Stopped');
	await expect(hq).not.toContainText('Offline');
	await expect(hq).toContainText(/It stopped unexpectedly\. Restarting in \d+ s/);
	// The row name contains its visible countdown; the separate live-status announcement stays stable.
	const label = (await hq.getByRole('button').first().getAttribute('aria-label')) ?? '';
	expect(label).toContain('This Mac');
	expect(label).toContain(await hq.getByTestId('office-location').innerText());
});

test(
	'OFC-11: Restart office brings a crashed office back, and the row reads Online',
	{ tag: '@journey' },
	async ({ page }) => {
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
	}
);

test('OFC-11: an office the owner stopped says Start office', async ({ page }) => {
	await openOffices(page, 'stopped_by_owner');
	await rows(page).first().getByRole('button').first().click();
	await expect(page.getByRole('dialog').getByTestId('restart-office')).toHaveText('Start office');
});

test('OFC-11: a host that is still starting says so and the button comes back', async ({
	page
}) => {
	await openOffices(page, 'crashed');
	await visit(page, '/offices?offices=empty&restart=not_ready');
	await rows(page).first().getByRole('button').first().click();
	const sheet = page.getByRole('dialog');
	await sheet.getByTestId('restart-office').click();
	await expect(sheet.getByTestId('restart-error')).toHaveText(
		'The office host is still starting. Try again in a moment.'
	);
	await expect(sheet.getByTestId('restart-office')).toHaveText('Restart office');
	await expect(sheet.getByTestId('restart-office')).toBeEnabled();
});

test('OFC-11: an accepted restart keeps the spinner while the office is still stopped', async ({
	page
}) => {
	await openOffices(page, 'crashed');
	await visit(page, '/offices?offices=empty&restart=stuck');
	await rows(page).first().getByRole('button').first().click();
	const sheet = page.getByRole('dialog');
	await sheet.getByTestId('restart-office').click();
	await expect(sheet.getByTestId('restart-office')).toHaveText('Restarting…');
	await page.waitForTimeout(5000); // several status re-reads later
	await expect(sheet.getByTestId('restart-office')).toHaveText('Restarting…');
	await expect(rows(page).first()).toContainText('Stopped');
});

test('OFC-11: a status change the host pushes shows at once, without waiting for the next check', async ({
	page
}) => {
	await openOffices(page);
	const hq = rows(page).first();
	await expect(hq).toContainText('Online');
	const officeId = (await hq.getAttribute('data-office')) ?? '';
	const office = {
		id: officeId,
		organization_id: officeId.replace('office-', ''),
		name: 'HQ',
		role: 'hq',
		host: 'This Mac'
	};
	const push = (update: object) =>
		page.evaluate(
			(u) =>
				(window as unknown as { __mockOfficeStatus: (u: object) => void }).__mockOfficeStatus(u),
			update
		);
	await push({
		kind: 'office',
		office: { ...office, status: 'unavailable', status_reason: 'crashed' }
	});
	await expect(hq).toContainText('Stopped');
	await expect(hq).toContainText('It stopped unexpectedly');
	await push({
		kind: 'office',
		office: { ...office, organization_id: 'another', status: 'active' }
	}); // another organization: nothing changes
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
		const { __mockOfficeStatus: push } = window as unknown as {
			__mockOfficeStatus: (update: StatusUpdate) => void;
		};
		push({ kind: 'office', office: { ...connection.office, ...changes } });
	}, changes);
}

test('OFC-13 / BUG-23: keyboard focus stays on Restart while it runs and after a refusal', async ({
	page
}) => {
	await openOffices(page, 'crashed');
	await visit(page, '/offices?offices=empty&restart=not_ready');
	await page.evaluate(async () => {
		const path = '/e2e/fakes/office-api.ts';
		const { officeApi } = await import(path);
		const restart = officeApi.restartOffice.bind(officeApi);
		let release!: () => void;
		const gate = new Promise<void>((resolve) => {
			release = resolve;
		});
		const held = { release, calls: 0 };
		Object.assign(window, { __heldRestart: held });
		officeApi.restartOffice = async (id: string) => {
			held.calls++;
			await gate;
			return restart(id);
		};
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
		const held = (window as unknown as { __heldRestart: { release: () => void; calls: number } })
			.__heldRestart;
		held.release();
		return held.calls;
	});
	expect(calls).toBe(1);
	await expect(sheet.getByTestId('restart-error')).toContainText(
		'The office host is still starting'
	);
	await expect(restart).toHaveText('Restart office');
	await shot(page, 'office-restart-keyboard-refused');
	await expect(restart).toBeFocused();
});

test('OFC-11 / BUG-24: an unsupported release gives local repair steps for the selected office', async ({
	page,
	context
}) => {
	await context.grantPermissions(['clipboard-read', 'clipboard-write']);
	await openOffices(page);
	const row = rows(page).first();
	await expect(row).toContainText('Online');
	await page.setViewportSize({ width: 320, height: 700 });
	const host = 'northwindregionalagentservicesproductionheadquarterscluster.example';
	await pushOffice(page, {
		name: host,
		host,
		status: 'unavailable',
		status_reason: 'unsupported_version'
	});
	const location = host; // No local endpoint: retain the reported remote repair target.
	const reason = `The release components on ${location} don't match. Run diagnostics and repair the selected instance there.`;
	await expect(row).toContainText(reason);
	await expect(row.locator('code')).toHaveText(host);
	await expect(row.locator('code')).toHaveCSS('word-break', 'break-all');
	await expect(row.locator('code')).toHaveCSS('font-family', /monospace/);
	const opener = row.getByRole('button');
	await expect
		.poll(async () => (await opener.boundingBox())?.width ?? 0)
		.toBeGreaterThanOrEqual(200);
	await shot(page, 'office-incompatible-row-320');
	await opener.click();
	const sheet = page.getByRole('dialog');
	await expect(sheet).toHaveCSS('width', '320px');
	await expect(sheet.getByRole('heading', { name: host, exact: true })).toBeVisible();
	expect(
		await sheet.evaluate((el) => {
			const title = el.querySelector('[data-slot="sheet-title"]')!.getBoundingClientRect();
			const close = el.querySelector('[data-slot="sheet-close"]')!.getBoundingClientRect();
			return title.right <= close.left;
		})
	).toBe(true);
	await shot(page, 'office-incompatible-release');
	await expect(sheet.getByTestId('office-why')).toHaveText(reason);
	await expect(sheet.getByTestId('restart-office')).toHaveCount(0);
	await expect(sheet.getByRole('button', { name: 'Copy details', exact: true })).toBeVisible();
	const show = sheet.getByRole('button', {
		name: 'Show local CLI repair instructions',
		exact: true
	});
	await expect(show).toHaveAttribute('aria-expanded', 'false');
	const id = await show.getAttribute('aria-controls');
	expect(id).toBeTruthy();
	await show.press('Enter');
	const hide = sheet.getByRole('button', { name: 'Hide repair instructions', exact: true });
	await expect(hide).toBeFocused();
	await expect(hide).toHaveAttribute('aria-expanded', 'true');
	const instructions = sheet.locator(`[id="${id}"]`);
	await expect(instructions.getByText(`On ${location}, run:`, { exact: true })).toBeVisible();
	await expect(instructions.locator('pre code')).toHaveText('agentic-org doctor');
	await expect(instructions).toContainText(
		'It checks the Agentic Org instance on that machine and says what to repair. Follow its steps there. This page updates on its own when the office is back.'
	);
	await expect(instructions).toContainText(
		`If ${location} runs more than one instance, add --name <instance>.`
	);
	await expect(instructions.locator('p code').filter({ hasText: '--name <instance>' })).toHaveText(
		'--name <instance>'
	);
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

test(
	'OFC-11 / BUG-24: pointer disclosure focuses the repair toggle',
	{ tag: ['@journey', '@webkit'] },
	async ({ page, browserName }) => {
		await openOffices(page);
		const row = rows(page).first();
		await expect(row).toContainText('Online');
		await pushOffice(page, {
			host: 'This Mac',
			status: 'unavailable',
			status_reason: 'unsupported_version'
		});
		await row.getByRole('button').click();
		const sheet = page.getByRole('dialog');
		await expect(sheet.getByTestId('office-why')).toHaveText(
			"The release components on This Mac don't match. Run diagnostics and repair the selected instance there."
		);
		await expect(sheet.getByTestId('office-why').locator('code')).toHaveCount(0);
		const show = sheet.getByRole('button', {
			name: 'Show local CLI repair instructions',
			exact: true
		});
		await sheet.getByRole('button', { name: 'Close', exact: true }).focus();
		if (browserName === 'webkit') await show.click();
		else await show.dispatchEvent('click'); // Model WebKit's click without native focus in the default suite.
		await expect(
			sheet.getByRole('button', { name: 'Hide repair instructions', exact: true })
		).toBeFocused();
		await expect(sheet.getByText('On This Mac, run:', { exact: true })).toBeVisible();
		await shot(page, 'office-repair-pointer-focus');
	}
);

test(
	'BUG-25: the Offices footer text meets 4.5:1 contrast',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOffices(page);
		const footer = page.getByText('Shown from HQ. Statuses include last-seen information.', {
			exact: true
		});
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
			const luminance = () =>
				[...ctx.getImageData(0, 0, 1, 1).data]
					.slice(0, 3)
					.map((v) => v / 255)
					.map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
					.reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0);
			const bg = luminance();
			ctx.fillStyle = getComputedStyle(el).color;
			ctx.fillRect(0, 0, 1, 1);
			const fg = luminance();
			return (Math.max(fg, bg) + 0.05) / (Math.min(fg, bg) + 0.05);
		});
		expect(contrast).toBeGreaterThanOrEqual(4.5);
	}
);

test(
	'OFC-11 / BUG-26: a new restart countdown uses the time of the status push',
	{ tag: '@webkit' },
	async ({ page }) => {
		await page.clock.install();
		await openOffices(page);
		const row = rows(page).first();
		await expect(row).toContainText('Online');
		await page.clock.pauseAt(new Date((await page.evaluate(() => Date.now())) + 1_000));
		await page.clock.runFor(10_000); // Establish the existing countdown clock's last sample.
		const current = (await page.evaluate(() => Date.now())) + 4_000;
		await page.clock.setSystemTime(new Date(current)); // A push arrives before the next timer tick.
		await row.evaluate((el) => {
			const seconds: number[] = [];
			const record = (text: string | null) => {
				const countdown = text?.match(/Restarting in (\d+) s/);
				if (countdown) seconds.push(Number(countdown[1]));
			};
			const observer = new MutationObserver((changes) => {
				for (const change of changes) {
					record(change.oldValue);
					record(change.target.textContent);
				}
			});
			observer.observe(el, {
				childList: true,
				characterData: true,
				characterDataOldValue: true,
				subtree: true
			});
			Object.assign(window, {
				__officeCountdownWrites: { seconds, stop: () => observer.disconnect() }
			});
		});
		const pending = {
			status: 'offline',
			status_reason: 'crashed',
			next_restart_at: new Date(current + 45_000).toISOString()
		} as const;
		await pushOffice(page, pending);
		await expect(row).toContainText('Stopped');
		await shot(page, 'office-countdown-new-push', { settle: false });
		await expect(row).toContainText('Restarting in 40 s');
		const written = await page.evaluate(() => {
			const log = (
				window as unknown as { __officeCountdownWrites: { seconds: number[]; stop: () => void } }
			).__officeCountdownWrites;
			log.stop();
			return [...new Set(log.seconds)];
		});
		expect(written).toEqual([40]);
		await expect(row.getByRole('button')).toHaveAccessibleName(
			literal(await row.getByTestId('office-location').innerText())
		);
		await page.clock.setSystemTime(new Date(current + 2_000));
		await pushOffice(page, pending); // The same deadline is reported again before a tick.
		await expect(row).toContainText('Restarting in 40 s');
		await page.clock.runFor(10_000);
		await expect(row).toContainText('Restarting in 30 s');
		await expect(row.getByRole('button')).toHaveAccessibleName(
			literal(await row.getByTestId('office-location').innerText())
		);

		const observed = await page.evaluate(() => Date.now());
		const later = { ...pending, next_restart_at: new Date(observed + 60_000).toISOString() };
		await pushOffice(page, later);
		await expect(row).toContainText('Restarting in 60 s');
		const opener = row.getByRole('button');
		const label = (await opener.getAttribute('aria-label')) ?? '';
		expect(label).toContain('Restarting in 60 s');
		await page.clock.setSystemTime(new Date(observed + 2_000));
		await pushOffice(page, later);
		await expect(row).toContainText('Restarting in 50 s');
		await expect(opener).toHaveAccessibleName(
			literal(await opener.getByTestId('office-location').innerText())
		);
	}
);

test(
	'OFC-13 / BUG-27: the office sheet has one Close and returns focus to the opener',
	{ tag: '@webkit' },
	async ({ page }) => {
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
	}
);

test(
	'OFC-13 / BUG-27: pointer activation without focus still returns to the office row',
	{ tag: ['@journey', '@webkit'] },
	async ({ page, browserName }) => {
		await openOffices(page);
		const opener = rows(page).first().getByRole('button');
		await page
			.getByRole('navigation', { name: 'Main', exact: true })
			.getByRole('link', { name: 'Offices', exact: true })
			.focus();
		// WebKit doesn't focus a button on pointer activation. Exercise the same path in Chromium.
		if (browserName === 'webkit') await opener.click();
		else await opener.dispatchEvent('click');
		const sheet = page.getByRole('dialog');
		await expect(sheet).toBeVisible();
		await sheet.getByRole('button', { name: 'Close', exact: true }).click();
		await expect(sheet).not.toBeVisible();
		await expect(opener).toBeFocused();
		await shot(page, 'office-details-pointer-return-focus');
	}
);

test(
	'BUG-25: HQ source copy is accurate in a remote browser',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOffices(page);
		const footer = page.getByText('Shown from HQ. Statuses include last-seen information.', {
			exact: true
		});
		await expect(rows(page).first()).toContainText('7 agents');
		await expect(rows(page).first()).toContainText('Online');
		await expect(footer).toBeVisible();
		await page.setViewportSize({ width: 320, height: 640 });
		await footer.scrollIntoViewIfNeeded();
		await expect(footer).toBeInViewport({ ratio: 1 });
		await shot(page, 'offices-hq-source-320', { context: footer });
	}
);

for (const host of ['studio-mac.local', 'This Mac']) {
	test(
		`BUG-25: the local endpoint uses the shared machine label with reported host ${host}`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await openOffices(page);
			await expect(rows(page).first()).toContainText('Online');
			await pushOffice(page, { host, endpoint: '/tmp/test-owned-office.sock' });
			const location = "This Mac (you're here)";
			const row = rows(page).first();
			await expect(row).toContainText('Online');
			await expect(row.getByRole('button')).toHaveAccessibleName(
				`HQ, HQ, ${location}, Online, 7 agents, opens details`
			);
			await row.getByRole('button').press('Enter');
			await expect(page.getByRole('dialog')).toBeVisible();
			await page.keyboard.press('Escape');
			await expect(row.getByRole('button')).toBeFocused();
		}
	);
}

for (const reason of ['crashed', 'in_use'] as const) {
	test(
		`BUG-26: step boundaries and shortly are correct for ${reason}`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.clock.install();
			await openOffices(page);
			const row = rows(page).first();
			await expect(row).toContainText('Online');
			await page.clock.pauseAt(new Date((await page.evaluate(() => Date.now())) + 1_000));
			const now = await page.evaluate(() => Date.now());
			await pushOffice(page, {
				status: 'offline',
				status_reason: reason,
				next_restart_at: new Date(now + 45_000).toISOString()
			});
			const verb = reason === 'in_use' ? 'Trying again' : 'Restarting';
			await expect(row).toContainText(`${verb} in 40 s`);
			await row.getByRole('button').press('Enter');
			const why = page.getByTestId('office-why');
			await expect(why).toContainText(`${verb} in 40 s`);
			await page.clock.runFor(5_001);
			await expect(why).toContainText(`${verb} in 30 s`);
			await page.keyboard.press('Escape');
			const observed = await page.evaluate(() => Date.now());
			await pushOffice(page, {
				status: 'offline',
				status_reason: reason,
				next_restart_at: new Date(observed + 11_000).toISOString()
			});
			await expect(row).toContainText(`${verb} in 10 s`);
			await expect(row.getByRole('button')).toHaveAccessibleName(literal(`${verb} in 10 s`)); // 11 seconds remain.
			await page.clock.runFor(1_000);
			await expect(row).toContainText(`${verb} in 10 s`);
			await expect(row.getByRole('button')).toHaveAccessibleName(literal(`${verb} in 10 s`)); // 10 seconds remain.
			await page.clock.runFor(1_000);
			await expect(row).toContainText(`${verb} shortly`);
			await expect(row.getByRole('button')).toHaveAccessibleName(literal(`${verb} shortly`)); // 9 seconds remain.
			await page.clock.runFor(10_000);
			await expect(row).toContainText(`${verb} shortly`);
			await row.getByRole('button').press('Enter');
			await expect(why).toContainText(`${verb} shortly`);
			await shot(page, `office-countdown-shortly-${reason}`);
		}
	);
}

for (const change of ['reason', 'status'] as const) {
	test(
		`BUG-64: a refused restart note clears after its ${change} changes`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await openOffices(page, 'crashed');
			await visit(page, '/offices?offices=empty&restart=not_ready');
			await rows(page).first().getByRole('button').press('Enter');
			const sheet = page.getByRole('dialog');
			const restart = sheet.getByTestId('restart-office');
			await restart.press('Enter');
			await expect(sheet.getByTestId('restart-error')).toContainText(
				'The office host is still starting'
			);
			await expect(restart).toBeFocused();
			await pushOffice(page, {});
			await expect(sheet.getByTestId('restart-error')).toContainText(
				'The office host is still starting'
			);
			await pushOffice(
				page,
				change === 'reason'
					? { status: 'unavailable', status_reason: 'unsupported_version' }
					: { status: 'active', status_reason: 'crashed' }
			);
			await expect(sheet.getByTestId('restart-error')).toHaveCount(0);
			if (change === 'reason')
				await expect(
					sheet.getByRole('button', { name: 'Show local CLI repair instructions' })
				).toBeVisible();
			else await expect(sheet.getByText('Online', { exact: true })).toBeVisible();
			await shot(page, `office-refusal-cleared-${change}`);
		}
	);
}

test(
	'BUG-64: unavailable restart shows the reason row without raw host text',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOffices(page, 'crashed');
		await page.evaluate(async () => {
			const path = '/e2e/fakes/office-api.ts';
			const { officeApi } = await import(path);
			officeApi.restartOffice = async () => {
				throw { source: 'api', code: 'unavailable', message: 'RAW-HOST-REFUSAL' };
			};
		});
		await rows(page).first().getByRole('button').press('Enter');
		const sheet = page.getByRole('dialog');
		const restart = sheet.getByTestId('restart-office');
		await restart.press('Enter');
		await expect(restart).toHaveAttribute('aria-busy', 'false');
		await expect(sheet.getByTestId('office-why')).toHaveText('It stopped unexpectedly');
		await expect(sheet.getByTestId('restart-error')).toHaveCount(0);
		await expect(sheet).not.toContainText('RAW-HOST-REFUSAL');
		await expect(restart).toBeFocused();
		await shot(page, 'office-unavailable-restart-reason');
	}
);

test(
	'BUG-64: a delayed refusal cannot restore a note for an obsolete reason',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOffices(page, 'crashed');
		await page.evaluate(async () => {
			const path = '/e2e/fakes/office-api.ts';
			const { officeApi } = await import(path);
			officeApi.restartOffice = async () => {
				await new Promise<void>((resolve) =>
					window.addEventListener('release-office-refusal', () => resolve(), { once: true })
				);
				document.documentElement.dataset.refusalReturned = 'true';
				throw { source: 'api', code: 'not_ready', message: 'The office host is still starting.' };
			};
		});
		await rows(page).first().getByRole('button').press('Enter');
		const sheet = page.getByRole('dialog');
		await sheet.getByTestId('restart-office').press('Enter');
		await expect(sheet.getByTestId('restart-office')).toHaveAttribute('aria-busy', 'true');
		await pushOffice(page, { status: 'active', status_reason: undefined });
		await expect(sheet.getByText('Online', { exact: true })).toBeVisible();
		await page.evaluate(() => window.dispatchEvent(new Event('release-office-refusal')));
		await expect(page.locator('html')).toHaveAttribute('data-refusal-returned', 'true');
		await expect(sheet.getByTestId('restart-error')).toHaveCount(0);
		await shot(page, 'office-obsolete-delayed-refusal');
		await expect(sheet.getByTestId('restart-error')).toHaveCount(0);
	}
);

async function expectOfficePill(badge: Locator, scale: number) {
	const style = await badge.evaluate((element) => {
		const css = getComputedStyle(element),
			box = element.getBoundingClientRect();
		return {
			radius: parseFloat(css.borderRadius),
			x: parseFloat(css.paddingLeft),
			y: parseFloat(css.paddingTop),
			shadow: css.boxShadow,
			inViewport: box.left >= 0 && box.right <= window.innerWidth,
			fits: [...element.children]
				.map((child) => child.getBoundingClientRect())
				.every(
					(r) =>
						r.left >= box.left - 1 &&
						r.right <= box.right + 1 &&
						r.top >= box.top - 1 &&
						r.bottom <= box.bottom + 1
				)
		};
	});
	expect.soft(style.radius).toBeGreaterThanOrEqual(12 * scale);
	expect.soft(style.x).toBe(8 * scale);
	expect.soft(style.y).toBe(4 * scale);
	expect.soft(style.shadow).not.toBe('none');
	expect(style.fits).toBe(true);
	expect(style.inViewport).toBe(true);
	// Labels may ellipsize at 200%; preserve full text/name/title and the icon inside the pill.
	const label = badge.locator(':scope > span').last(),
		full = (await badge.textContent())!.trim();
	await expect(label).toHaveAttribute('title', full);
	await expect(label).toHaveCSS('white-space', 'nowrap');
	await expect(label).toHaveCSS('overflow-x', 'hidden');
	await expect(label).toHaveCSS('text-overflow', 'ellipsis');
	expect(
		await label.evaluate(
			(element) =>
				Math.abs(
					element.getBoundingClientRect().height - parseFloat(getComputedStyle(element).lineHeight)
				) < 1
		)
	).toBe(true);
	await expect(badge).toHaveCSS('white-space', 'nowrap');
	await expect(badge.locator('svg')).toHaveCount(1);
	await expect(badge.locator('svg')).toHaveAttribute('aria-hidden', 'true');
	expect(await badge.getAttribute('role')).toBeNull();
	expect(await badge.getAttribute('aria-live')).toBeNull();
}

for (const width of [320, 640, 960, 1280]) {
	for (const scale of [1, 2]) {
		test(
			`OFC-13 BUG-70 Offices pill: list and details at ${width}px and ${scale * 100}% text`,
			{ tag: '@webkit' },
			async ({ page }) => {
				await page.setViewportSize({ width, height: 900 });
				await openOffices(page);
				await expect(rows(page).first()).toContainText('Online');
				await page.addStyleTag({ content: `html { font-size: ${scale * 100}% !important; }` });
				await page.evaluate(() => document.fonts.ready.then(() => undefined));
				const row = rows(page).first(),
					trigger = row.getByRole('button');
				await expect(trigger).toHaveAccessibleName(
					"HQ, HQ, This Mac (you're here), Online, 7 agents, opens details"
				);
				const badge = row.locator('[data-status-icon]');
				await expect(badge).toHaveText('Online');
				await expect(badge).toHaveAttribute('data-status-icon', 'circle');

				await shot(page, `office-pill-list-${width}-${scale * 100}`, { context: row });
				expect(
					await trigger
						.locator('span')
						.first()
						.evaluate((element) => element.scrollWidth <= element.clientWidth)
				).toBe(true);
				expect(
					await badge.getByText('Online', { exact: true }).evaluate((element) => {
						const range = document.createRange();
						range.selectNodeContents(element);
						return range.getClientRects().length;
					})
				).toBe(1); // A normal short word must remain readable at 320px/200%.
				await expectOfficePill(badge, scale);
				const countFits = await row.evaluate((element) => {
					const box = element.getBoundingClientRect(),
						count = element.querySelector('[data-testid=agent-count]')!.getBoundingClientRect();
					return count.left >= box.left && count.right <= box.right;
				});
				expect(countFits).toBe(true);

				await trigger.press('Enter');
				const sheet = page.getByRole('dialog'),
					status = sheet.locator('[data-status-icon]');
				await expect(status).toHaveText('Online');
				expect(await sheet.ariaSnapshot()).toContain('Online');
				await expectOfficePill(status, scale);
				await shot(page, `office-pill-details-${width}-${scale * 100}`, { context: sheet });
				await sheet.getByRole('button', { name: 'Close', exact: true }).press('Enter');
				await expect(trigger).toBeFocused();
			}
		);
	}
}

for (const scale of [1, 2]) {
	test(
		`OFC-13 BUG-70 Offices pill: every status retains its word and icon at ${scale * 100}% text`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width: 320, height: 900 });
			await openOffices(page);
			await page.addStyleTag({ content: `html { font-size: ${scale * 100}% !important; }` });
			await page.evaluate(() => document.fonts.ready.then(() => undefined));
			await page.evaluate(async () => {
				const path = '/src/lib/modules/offices/testing/office-status.ts';
				const { mountOfficeStatuses } = await import(path);
				const target = document.createElement('div');
				document.body.append(target);
				mountOfficeStatuses(target);
			});
			const fixture = page.getByRole('region', { name: 'Office status contracts', exact: true });
			const cases = [
				['active', 'Online', 'circle', 'text-success', undefined],
				['starting', 'Starting', 'loader', 'text-info', 'M12 2v4'],
				['pending', 'Invited', 'mail', 'text-fg-muted', 'm22 7-8.991 5.727a2 2 0 0 1-2.009 0L2 7'],
				['offline', 'Offline', 'circle-outline', 'text-fg-muted', undefined],
				['unavailable', 'Stopped', 'circle-x', 'text-danger', 'm15 9-6 6'],
				['update_needed', 'Update needed', 'circle-arrow-up', 'text-warning', 'm16 12-4-4-4 4'],
				['unreachable', 'Unreachable', 'wifi-off', 'text-danger', 'M12 20h.01'],
				['checking', 'Checking…', 'loader', 'text-info', 'M12 2v4'],
				[
					'status_unknown',
					'Status unknown',
					'circle-help',
					'text-fg-muted',
					'M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3'
				],
				[
					'undefined',
					'Status unknown',
					'circle-help',
					'text-fg-muted',
					'M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3'
				],
				[
					'future',
					'Status unknown',
					'circle-help',
					'text-fg-muted',
					'M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3'
				]
			] as const;
			for (const [state, label, icon, tone, path] of cases) {
				const badge = fixture.locator(`[data-office-status="${state}"] [data-status-icon]`);
				await expect(badge).toBeVisible();
				await badge.scrollIntoViewIfNeeded();
				await expect(badge).toHaveText(label);
				expect(await badge.ariaSnapshot()).toContain(label);
				await expect(badge).toHaveAttribute('data-status-icon', icon);
				await expect(badge.locator('svg')).toHaveClass(new RegExp(`\\b${tone}\\b`));
				if (path) await expect(badge.locator('svg path').first()).toHaveAttribute('d', path);
				if (icon === 'circle' || icon === 'circle-outline') {
					await expect(badge.locator('svg circle')).toHaveAttribute('r', '10');
					await expect(badge.locator('svg path')).toHaveCount(0);
					expect((await badge.locator('svg').getAttribute('class'))?.includes('fill-current')).toBe(
						icon === 'circle'
					);
				}
				if (icon === 'loader') {
					await expect(badge.locator('svg')).toHaveCSS('animation-name', 'spin');
					await page.emulateMedia({ reducedMotion: 'reduce' });
					await expect(badge.locator('svg')).toHaveCSS('animation-name', 'none');
					await page.emulateMedia({ reducedMotion: 'no-preference' });
				}
				await expectOfficePill(badge, scale);
			}
			await shot(page, `office-pill-vocabulary-${scale * 100}`, { context: fixture });
		}
	);
}

const literal = (text: string) => new RegExp(text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'));

for (const status of ['Status unknown', 'Unreachable']) {
	test(
		`OFC-13 BUG-70 BUG-91: ${status} truncates inside its real Offices row at 320px and 200% text`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width: 320, height: 900 });
			await openOffices(page);
			if (status === 'Unreachable') await visit(page, '/offices?offices=empty&mock=unreachable');
			else
				await page.evaluate(async () => {
					const path = '/e2e/fakes/office-api.ts';
					const { officeApi } = await import(path);
					const connection = (await officeApi.connections()).at(-1)!;
					(
						window as unknown as { __mockOfficeStatus: (update: StatusUpdate) => void }
					).__mockOfficeStatus({
						kind: 'offices',
						offices: [
							connection.office,
							{
								...connection.office,
								id: 'remote-status',
								name: 'Studio',
								host: 'studio.local',
								endpoint: undefined,
								role: 'satellite'
							}
						]
					});
				});
			await page.addStyleTag({ content: 'html { font-size: 200% !important; }' });
			const row =
				status === 'Status unknown' ? rows(page).filter({ hasText: 'Studio' }) : rows(page).first();
			const badge = row.locator('[data-status-icon]');
			await expect(badge).toHaveText(status);
			await shot(page, `of1-pill-bounds-${status.replaceAll(' ', '-')}`, { context: row });
			const bounds = await badge.evaluate((element) => {
				const pill = element.getBoundingClientRect(),
					row = element.closest('[data-testid=office-row]')!,
					box = row.getBoundingClientRect(),
					css = getComputedStyle(row);
				return {
					left: pill.left - box.left - parseFloat(css.paddingLeft),
					right: box.right - parseFloat(css.paddingRight) - pill.right
				};
			});
			expect(bounds.left).toBeGreaterThanOrEqual(-1);
			expect(bounds.right).toBeGreaterThanOrEqual(-1);
			const label = badge.locator(':scope > span').last();
			await expect(label).toHaveAttribute('title', status);
			await expect(label).toHaveCSS('text-overflow', 'ellipsis');
			await expect(label).toHaveCSS('overflow-x', 'hidden');
			await expect(row.locator('button[aria-haspopup]')).toHaveAccessibleName(literal(status));
		}
	);
}

for (const width of [390, 960]) {
	test(
		`OFC-13 BUG-70 BUG-92: long status icon and label share one line at ${width}px and 200% text`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width, height: 900 });
			await openOffices(page);
			await expect(rows(page).first()).toContainText('Online');
			await pushOffice(page, { status: 'update_needed' });
			await page.addStyleTag({ content: 'html { font-size: 200% !important; }' });
			const badge = rows(page).first().locator('[data-status-icon]');
			await expect(badge).toHaveText('Update needed');
			await shot(page, `of1-pill-one-line-${width}`, { context: rows(page).first() });
			const geometry = await badge.evaluate((element) => {
				const icon = element.querySelector('svg')!.getBoundingClientRect(),
					label = element.lastElementChild!,
					text = label.getBoundingClientRect(),
					css = getComputedStyle(element);
				return {
					centers: Math.abs((icon.top + icon.bottom) / 2 - (text.top + text.bottom) / 2),
					height: element.getBoundingClientRect().height,
					line:
						parseFloat(getComputedStyle(label).lineHeight) +
						parseFloat(css.paddingTop) +
						parseFloat(css.paddingBottom)
				};
			});
			expect(geometry.centers).toBeLessThanOrEqual(1);
			expect(geometry.height).toBeCloseTo(geometry.line, 0);
		}
	);
}

test(
	'OFC-13 BUG-70 BUG-81: the row accessible name contains its visible location',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOffices(page);
		await expect(rows(page).first()).toContainText('Online');
		const opener = rows(page).first().locator('button[aria-haspopup]');
		const visible = await opener.getByTestId('office-location').innerText();
		await shot(page, 'of1-visible-row-name', { context: rows(page).first() });
		await expect(opener).toHaveAccessibleName(literal(visible));
	}
);

for (const seconds of [45, 60]) {
	test(
		`OFC-11 OFC-13 BUG-70 BUG-82: the spoken row countdown matches the visible ${seconds === 45 ? 40 : 60} second countdown`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.clock.install();
			await openOffices(page);
			await expect(rows(page).first()).toContainText('Online');
			await page.clock.pauseAt(new Date((await page.evaluate(() => Date.now())) + 1_000));
			const now = await page.evaluate(() => Date.now());
			await pushOffice(page, {
				status: 'offline',
				status_reason: 'crashed',
				next_restart_at: new Date(now + seconds * 1_000).toISOString()
			});
			const row = rows(page).first(),
				opener = row.locator('button[aria-haspopup]');
			await expect(row).toContainText(`Restarting in ${seconds === 45 ? 40 : 60} s`);
			await shot(page, `of1-visible-countdown-${seconds}`, { settle: false, context: row });
			const visible = await opener.getByTestId('office-location').innerText();
			await expect(opener).toHaveAccessibleName(literal(visible));
			const announcement = await page.getByTestId('live-status').textContent();
			await page.clock.runFor(10_000);
			await expect(opener).toHaveAccessibleName(
				literal(await opener.getByTestId('office-location').innerText())
			);
			await expect(page.getByTestId('live-status')).toHaveText(announcement!);
		}
	);
}

test(
	'OFC-1 OFC-13 BUG-70 OF-2 OF-3 OF-4 OF-5: Offices and Unplaced use flat quiet rows and secondary disabled actions',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOffices(page);
		await expect(rows(page).first()).toContainText('Online');
		const list = page.getByRole('list', { name: 'Offices', exact: true });
		await shot(page, 'of1-flat-list-and-unplaced');
		await expect(list).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
		await expect(list).toHaveCSS('border-radius', '0px');
		await expect(rows(page).first()).toHaveCSS('padding-top', '16px');
		await expect(rows(page).first()).toHaveCSS('padding-left', '32px');
		const normalRow = rows(page).first();
		expect(
			await normalRow.evaluate((element) => {
				const pill = element.querySelector('[data-status-icon]')!.getBoundingClientRect(),
					chevron = element
						.querySelector(':scope > svg, [data-testid=office-actions] > svg')!
						.getBoundingClientRect();
				return Math.abs((pill.top + pill.bottom) / 2 - (chevron.top + chevron.bottom) / 2);
			})
		).toBeLessThanOrEqual(1);
		await expect(
			rows(page).first().locator(':scope > span[aria-hidden=true]').filter({ hasText: /^HQ$/ })
		).toHaveCount(0);
		const unplaced = page.getByTestId('unplaced-note');
		await expect(unplaced).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
		const explanation = page.getByTestId('unplaced-explanation');
		await expect(explanation).toContainText(
			"These agents aren't placed in an office, so they don't run anywhere."
		);
		await expect(explanation).not.toHaveAttribute('role');
		await expect(explanation).not.toHaveAttribute('aria-live');
		await expect(unplaced.getByRole('listitem')).toHaveCount(1);
		await expect(unplaced.getByRole('listitem')).toHaveCSS('padding-top', '16px');
		// Add office is the one filled action; the row actions are quieter outline buttons.
		const add = page.getByRole('button', { name: 'Add office', exact: true });
		await expect(add).toBeEnabled();
		const place = unplaced.getByRole('button', { name: 'Place Inês Duarte…', exact: true });
		await expect(place).toBeEnabled();
		expect(await add.evaluate((element) => getComputedStyle(element).backgroundColor)).not.toBe(
			await place.evaluate((element) => getComputedStyle(element).backgroundColor)
		);
	}
);

test(
	'OFC-1 OFC-13 BUG-70 OF-2 OF-3: office title keeps HQ only for a named HQ and the row has a visible focus ring',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOffices(page);
		const row = rows(page).first(),
			opener = row.locator('button[aria-haspopup]');
		await expect(row).toContainText('Online');
		await expect(opener.locator(':scope > span').first()).toHaveText('HQ');
		await pushOffice(page, { name: 'Main office' });
		await expect(opener.locator(':scope > span').first()).toHaveText('Main office · HQ');
		const words = (name: string) => name.replace(/[·,]/g, ' ').replace(/\s+/g, ' ').trim();
		const visibleTitle = words(await opener.locator(':scope > span').first().innerText());
		expect(words((await opener.getAttribute('aria-label'))!).startsWith(visibleTitle)).toBe(true);
		await pushOffice(page, { name: 'Studio', role: 'satellite' });
		await expect(opener.locator(':scope > span').first()).toHaveText('Studio');
		await page.keyboard.press('Tab');
		await opener.focus();
		const ring = opener.locator(':scope > .absolute');
		await expect(ring).toHaveCSS('outline-style', 'solid');
		await expect(ring).toHaveCSS('outline-width', '2px');
		await expect(opener).toHaveCSS('outline-style', 'none');
		await opener.press('Enter');
		await expect(page.getByRole('dialog')).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(opener).toBeFocused();
		await shot(page, 'of1-flat-row-keyboard-focus', { context: row });
	}
);

test(
	'OFC-12 BUG-70 OF-2: the stretched row target does not intercept pointer Retry',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOffices(page);
		await visit(page, '/offices?offices=empty&mock=unreachable');
		const row = rows(page).first(),
			retry = row.getByTestId('retry-office');
		await expect(row).toContainText('Unreachable');
		await retry.click();
		await expect(retry).toHaveText('Retrying…');
		await expect(page.getByRole('dialog')).toHaveCount(0);
		await expect(retry).toHaveText('Retry');
		await shot(page, 'of1-row-pointer-retry', { context: row });
	}
);

// A truncated status needs enough room to paint its ellipsis, not just a positive box width.
async function expectVisibleStatusText(row: Locator) {
	const label = row.locator('[data-status-icon] > span').last();
	await expect(label).toHaveCSS('text-overflow', 'ellipsis');
	const geometry = await label.evaluate((element) => {
		const css = getComputedStyle(element),
			canvas = document.createElement('canvas');
		const context = canvas.getContext('2d')!;
		context.font = `${css.fontStyle} ${css.fontWeight} ${css.fontSize} ${css.fontFamily}`;
		return {
			width: element.getBoundingClientRect().width,
			natural: element.scrollWidth,
			ellipsis: context.measureText('…').width
		};
	});
	expect(geometry.width, JSON.stringify(geometry)).toBeGreaterThan(0);
	expect(geometry.width, JSON.stringify(geometry)).toBeGreaterThanOrEqual(
		Math.min(geometry.natural, geometry.ellipsis)
	);
}

async function expectReadableOfficeName(row: Locator) {
	const name = row.locator('button[aria-haspopup] > span').first();
	const geometry = await name.evaluate((element) => {
		const css = getComputedStyle(element),
			context = document.createElement('canvas').getContext('2d')!;
		context.font = `${css.fontStyle} ${css.fontWeight} ${css.fontSize} ${css.fontFamily}`;
		const box = element.getBoundingClientRect(),
			parent = element.parentElement!.getBoundingClientRect();
		return {
			visible: Math.min(box.right, parent.right) - Math.max(box.left, parent.left),
			minimum: Math.min(
				6 * context.measureText('0').width,
				context.measureText(element.getAttribute('title')!).width
			)
		};
	});
	expect(geometry.visible, JSON.stringify(geometry)).toBeGreaterThanOrEqual(geometry.minimum);
}

async function expectFullStatusWord(row: Locator, chevronOnNextLine = false) {
	const geometry = await row.evaluate((element) => {
		const pill = element.querySelector('[data-status-icon]')!,
			label = pill.lastElementChild!;
		const css = getComputedStyle(element),
			chevron = element.querySelector(':scope > svg, [data-testid=office-actions] > svg')!;
		const box = element.getBoundingClientRect(),
			arrow = chevron.getBoundingClientRect();
		return {
			rootFont: getComputedStyle(document.documentElement).fontSize,
			rowWidth: box.width,
			paddingLeft: css.paddingLeft,
			paddingRight: css.paddingRight,
			labelWidth: label.clientWidth,
			labelScroll: label.scrollWidth,
			pillBottom: pill.getBoundingClientRect().bottom,
			chevronTop: arrow.top,
			chevronEndGap: box.right - parseFloat(css.paddingRight) - arrow.right,
			chevronWidth: arrow.width,
			chevronFits: arrow.left >= box.left && arrow.right <= box.right,
			rowFits: element.scrollWidth <= element.clientWidth
		};
	});
	expect(geometry.labelWidth, JSON.stringify(geometry)).toBeGreaterThan(0);
	expect(geometry.labelWidth, JSON.stringify(geometry)).toBe(geometry.labelScroll);
	expect(geometry.chevronWidth).toBeGreaterThan(0);
	expect(geometry.chevronFits).toBe(true);
	expect(geometry.chevronEndGap).toBeCloseTo(0, 0);
	if (chevronOnNextLine) expect(geometry.chevronTop).toBeGreaterThanOrEqual(geometry.pillBottom);
	expect(geometry.rowFits).toBe(true);
}

for (const width of [320, 640, 960, 1280])
	for (const text of [1, 2]) {
		test(
			`OFC-1 OFC-13 BUG-70 A-OF1 preflight: rows and details at ${width}px and ${text * 100}% text`,
			{ tag: '@webkit' },
			async ({ page }) => {
				await page.setViewportSize({ width, height: 900 });
				await openOffices(page);
				await expect(rows(page).first()).toContainText('Online');
				await page.addStyleTag({ content: `html { font-size: ${text * 100}% !important; }` });
				await expect(page.locator('html')).toHaveCSS(
					'font-size',
					`${text * (test.info().project.name.endsWith('-20px') ? 20 : 16)}px`
				);
				const row = rows(page).first(),
					opener = row.locator('button[aria-haspopup]');
				await shot(page, `of1-preflight-${width}-${text}-online`, { context: row });
				expect(await row.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
				expect(
					await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)
				).toBe(true);
				await expect(row.getByTestId('agent-count')).toHaveText('7 agents');
				const pill = row.locator('[data-status-icon]');
				await expect(pill).toHaveText('Online');
				await expectReadableOfficeName(row);
				await expectVisibleStatusText(row);
				if (test.info().project.name.endsWith('-20px') && width === 960 && text === 2)
					await expectFullStatusWord(row);
				await opener.press('Enter');
				await expect(page.getByRole('dialog')).toBeVisible();
				await expect(page.getByTestId('office-details')).toContainText("This Mac (you're here)");
				await shot(page, `of1-preflight-${width}-${text}-details`, {
					context: page.getByRole('dialog')
				});
				await page.keyboard.press('Escape');
				await expect(opener).toBeFocused();
				await pushOffice(page, {
					name: 'Engineering office with a long readable name',
					status: 'unavailable',
					status_reason: 'crashed'
				});
				await expect(row).toContainText('Stopped');
				await expectReadableOfficeName(row);
				await expectVisibleStatusText(row);
				if (test.info().project.name.endsWith('-20px') && width === 960 && text === 2)
					await expectFullStatusWord(row);
				await expect(opener).toHaveAccessibleName(
					/Engineering office with a long readable name, HQ/
				);
				await shot(page, `of1-preflight-${width}-${text}-stopped`, { context: row });
				expect(await row.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
			}
		);
	}

for (const host of [
	'different-reported-machine.local',
	'northwindregionalagentservicesproductionheadquarterscluster.example'
]) {
	test(
		`OFC-11 OFC-13 BUG-81 BUG-25: a local endpoint keeps its shared machine label and reported repair host ${host}`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width: 320, height: 900 });
			await openOffices(page);
			await expect(rows(page).first()).toContainText('Online');
			await page.addStyleTag({ content: 'html { font-size: 200% !important; }' });
			const location = `This Mac (reported as ${host})`;
			await pushOffice(page, {
				host,
				endpoint: '/tmp/test-owned-office.sock',
				status: 'unavailable',
				status_reason: 'unsupported_version'
			});
			const row = rows(page).first(),
				opener = row.locator('button[aria-haspopup]');
			await expect(opener).toHaveAccessibleName(literal("This Mac (you're here)"));
			await expect(row).toContainText(`The release components on ${location} don't match.`);
			await expect(row.locator('code')).toHaveText(host);
			await expect(row.locator('code')).toHaveCSS('word-break', 'break-all');
			await expect(row.locator('code')).toHaveCSS('font-family', /monospace/);
			await shot(page, 'of1-shared-machine-reported-host-row', { context: row });
			await opener.press('Enter');
			const sheet = page.getByRole('dialog');
			await expect(sheet.getByTestId('office-details')).toContainText("This Mac (you're here)");
			await expect(sheet.getByTestId('office-why')).toContainText(location);
			await sheet
				.getByRole('button', { name: 'Show local CLI repair instructions', exact: true })
				.press('Enter');
			await expect(sheet.getByText(`On ${location}, run:`, { exact: true })).toBeVisible();
			await expect(sheet).toContainText(
				`If ${location} runs more than one instance, add --name <instance>.`
			);
			await expect(sheet.locator('pre code')).toHaveText('agentic-org doctor');
			await expect(sheet.locator('p code').filter({ hasText: host })).toHaveCount(3);
			for (const code of await sheet.locator('p code').filter({ hasText: host }).all()) {
				await expect(code).toHaveText(host);
				await expect(code).toHaveCSS('word-break', 'break-all');
				await expect(code).toHaveCSS('font-family', /monospace/);
			}
			expect(await sheet.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(
				true
			);
			await sheet.getByText(`On ${location}, run:`, { exact: true }).scrollIntoViewIfNeeded();
			await shot(page, 'of1-shared-machine-reported-host', { context: sheet });
		}
	);
}

test(
	'OFC-1 OFC-13 BUG-70 A-OF1 preflight: empty office list and empty agent counts remain readable',
	{ tag: '@webkit' },
	async ({ page }) => {
		await page.setViewportSize({ width: 320, height: 900 });
		await openOffices(page);
		await visit(page, '/offices?offices=empty&org=empty');
		await expect(rows(page).first().getByTestId('agent-count')).toHaveText('0 agents');
		await expect(page.getByTestId('unplaced-note')).toHaveCount(0);
		await page.addStyleTag({ content: 'html { font-size: 200% !important; }' });
		await page.evaluate(() =>
			(
				window as unknown as { __mockOfficeStatus: (update: StatusUpdate) => void }
			).__mockOfficeStatus({ kind: 'offices', offices: [] })
		);
		await expect(rows(page)).toHaveCount(0);
		await expect(page.getByTestId('no-offices')).toHaveText('No offices to show.');
		await shot(page, 'of1-no-offices-or-agents-320-200', {
			context: page.getByTestId('no-offices')
		});
		expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
			true
		);
	}
);

test(
	'OFC-11 OFC-13 BUG-70 OF-2: repair prose requires both an incompatible reason and a stopped office',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOffices(page);
		await expect(rows(page).first()).toContainText('Online');
		const row = rows(page).first();
		await pushOffice(page, { status: 'active', status_reason: 'unsupported_version' });
		await expect(row).toContainText("This Mac (you're here)");
		await expect(row).not.toContainText('The release components');
		await expect(row.locator('code')).toHaveCount(0);
		await pushOffice(page, { status: 'unavailable', status_reason: 'crashed' });
		await expect(row).toContainText('It stopped unexpectedly');
		await expect(row).not.toContainText('The release components');
		await row.locator('button[aria-haspopup]').press('Enter');
		await expect(page.getByTestId('office-why')).toHaveText('It stopped unexpectedly');
		await expect(page.getByTestId('office-why').locator('code')).toHaveCount(0);
		await shot(page, 'of1-repair-reason-guard', { context: page.getByRole('dialog') });
	}
);

test(
	'OFC-13 BUG-70 BUG-91: the full status word and chevron stay visible in a narrow 960px content column at 40px root text',
	{ tag: '@webkit' },
	async ({ page }) => {
		await page.setViewportSize({ width: 960, height: 900 });
		await openOffices(page);
		const row = rows(page).first();
		await expect(row).toContainText('Online');
		await page.addStyleTag({ content: 'html {font-size:250% !important}' });
		await expect(page.locator('html')).toHaveCSS('font-size', '40px');
		await pushOffice(page, { status: 'unavailable', status_reason: 'crashed' });
		await expect(row).toContainText('Stopped');
		await shot(page, 'of1-narrow-content-full-status-40px', { context: row });
		await expectFullStatusWord(row);
		await expectReadableOfficeName(row);
		const opener = row.locator('button[aria-haspopup]');
		await expect(row.getByRole('button')).toHaveCount(1);
		const arrow = await row
			.locator(':scope > svg, [data-testid=office-actions] > svg')
			.boundingBox();
		expect(arrow).not.toBeNull();
		await page.mouse.click(arrow!.x + arrow!.width / 2, arrow!.y + arrow!.height / 2);
		await expect(page.getByRole('dialog')).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(opener).toBeFocused();
	}
);

// Exercise the old failing pixels, including the viewport lg gutter transition at 304px.
// Now that the shell and settings layouts reflow instead of starving a 1579px viewport to 299px, pin the Offices
// container to `column` px under a >= 1024px (`lg:`) viewport so compact-row geometry stays covered at these pixels.
for (const column of [299, 300, 301, 304]) {
	test(
		`OFC-13 BUG-70 BUG-91: readable status and name at the ${column}px content column`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width: 1280, height: 900 });
			await openOffices(page);
			const row = rows(page).first();
			await expect(row).toContainText('Online');
			await page.addStyleTag({ content: 'html { font-size: 250% !important; }' });
			await expect(page.locator('html')).toHaveCSS('font-size', '40px');
			await row.evaluate((el, width) => {
				const container = el.closest('[class~="@container"]') as HTMLElement | null;
				if (container) container.style.width = `${width}px`;
			}, column);
			await pushOffice(page, {
				name: 'Engineering office with a long readable name',
				status: 'unavailable',
				status_reason: 'crashed'
			});
			await expect(row).toContainText('Stopped');
			expect((await row.boundingBox())!.width).toBeCloseTo(column, 0);
			await shot(page, `of1-content-${column}-40px`, { context: row });
			await expectVisibleStatusText(row);
			await expectReadableOfficeName(row);
			await expectFullStatusWord(row);
			await expect(row).toHaveCSS('padding-left', `${Number((column * 0.05).toFixed(2))}px`);
			const unplaced = page.getByTestId('unplaced-note').getByRole('listitem');
			await expect(unplaced).toHaveCSS('padding-left', `${Number((column * 0.05).toFixed(2))}px`);
			expect(await unplaced.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(
				true
			);
			expect(
				await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)
			).toBe(true);
		}
	);
}

for (const rootFont of [16, 32, 40])
	for (const offset of [-1, 1]) {
		test(
			`OFC-13 BUG-70 BUG-91: compact row threshold ${offset < 0 ? 'inside' : 'outside'} at ${rootFont}px root`,
			{ tag: '@webkit' },
			async ({ page }) => {
				const column = 21 * rootFont + offset;
				// The fixed 264px ExecutiveSidebar plus 14rem settings column take `settingsChrome(rootFont)` at desktop widths; mobile has neither.
				const sidebar = rootFont === 16 ? 0 : settingsChrome(rootFont);
				await page.setViewportSize({ width: sidebar + column, height: 1100 });
				await openOffices(page);
				const row = rows(page).first();
				await expect(row).toContainText('Online');
				await page.addStyleTag({
					content: `html { font-size: ${(rootFont / 16) * 100}% !important; }`
				});
				await expect(page.locator('html')).toHaveCSS('font-size', `${rootFont}px`);
				await pushOffice(page, {
					name: 'Engineering office with a long readable name',
					status: 'update_needed',
					status_reason: 'unsupported_version'
				});
				await expect(row).toContainText('Update needed');
				await shot(page, `of1-rem-boundary-${rootFont}-${offset}`, { context: row });
				expect((await row.boundingBox())!.width).toBeCloseTo(column, 0);
				await expectReadableOfficeName(row);
				await expectVisibleStatusText(row);
				const group = row.getByTestId('office-actions');
				await expect(group).toHaveCSS('flex-wrap', 'wrap');
				await expect(row).toHaveCSS('column-gap', `${offset < 0 ? rootFont / 2 : rootFont}px`);
				const gutter =
					offset < 0 ? Math.min(rootFont, column * 0.05) : rootFont * (rootFont === 16 ? 1 : 2);
				await expect(row).toHaveCSS('padding-left', `${gutter}px`);
				await expect(page.getByTestId('unplaced-note').getByRole('listitem')).toHaveCSS(
					'padding-left',
					`${gutter}px`
				);
				if (offset < 0) {
					await expectFullStatusWord(row);
					const layout = await row.evaluate((element) => {
						const button = element.querySelector('button[aria-haspopup]')!.getBoundingClientRect();
						const group = element
							.querySelector('[data-testid=office-actions]')!
							.getBoundingClientRect();
						return {
							buttonBottom: button.bottom,
							groupTop: group.top,
							groupWidth: group.width,
							buttonWidth: button.width
						};
					});
					expect(layout.groupTop).toBeGreaterThanOrEqual(layout.buttonBottom);
					expect(layout.groupWidth).toBeGreaterThan(0);
					expect(layout.groupWidth).toBeLessThanOrEqual(layout.buttonWidth);
				}
				expect(await row.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(
					true
				);
			}
		);
	}

// WebKit has no Playwright default-font preference: these are explicit computed roots,
// paired with the separate real Chromium default-20px run (Ada's approved proof).
for (const rootFont of [20, 40])
	for (const width of [300, 304, 960]) {
		test(
			`OFC-13 BUG-70 BUG-91: readable row at ${width}px viewport with computed ${rootFont}px root`,
			{ tag: '@webkit' },
			async ({ page }) => {
				await page.setViewportSize({ width, height: 1100 });
				await openOffices(page);
				const row = rows(page).first();
				await expect(row).toContainText('Online');
				await page.addStyleTag({ content: `html { font-size: ${rootFont}px !important; }` });
				await expect(page.locator('html')).toHaveCSS('font-size', `${rootFont}px`);
				// The shared badge animates font-size/padding; measure the settled rendered row.
				await shot(page, `of1-computed-root-${width}-${rootFont}-online`, { context: row });
				await expectReadableOfficeName(row);
				await expectVisibleStatusText(row);
				await pushOffice(page, {
					name: 'Engineering office with a long readable name',
					status: 'unavailable',
					status_reason: 'crashed'
				});
				await expect(row).toContainText('Stopped');
				await shot(page, `of1-computed-root-${width}-${rootFont}`, { context: row });
				await expectReadableOfficeName(row);
				await expectVisibleStatusText(row);
				await expectFullStatusWord(row);
				expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
					true
				);
			}
		);
	}

async function expectTrailingOfficeChevron(row: Locator, retryExpected: boolean, mayWrap = false) {
	expect(
		await row.locator('[data-status-icon]').evaluate((e) => e.closest('[aria-hidden="true"]'))
	).not.toBeNull();
	await expect(row.locator('[data-testid=office-actions] > svg')).toHaveAttribute(
		'aria-hidden',
		'true'
	);
	await expect(row.getByTestId('retry-office')).toHaveCount(retryExpected ? 1 : 0);
	if (retryExpected) {
		await expect(row.getByRole('button', { name: /Retry/ })).toHaveCount(1);
		expect(
			await row.getByTestId('retry-office').evaluate((e) => e.closest('[aria-hidden="true"]'))
		).toBeNull();
	}
	const layout = await row.evaluate((element) => {
		const pill = element.querySelector('[data-status-icon]')!,
			group = element.querySelector('[data-testid=office-actions]')!,
			arrow = group.querySelector(':scope > svg')!;
		const retry = element.querySelector('[data-testid=retry-office]');
		const box = (item: Element) => {
			const b = item.getBoundingClientRect();
			return {
				left: b.left,
				right: b.right,
				top: b.top,
				bottom: b.bottom,
				width: b.width,
				height: b.height,
				mid: (b.top + b.bottom) / 2
			};
		};
		const row = element.getBoundingClientRect(),
			css = getComputedStyle(element);
		return {
			rowStart: row.left + parseFloat(css.paddingLeft),
			rowEnd: row.right - parseFloat(css.paddingRight),
			rowBottom: row.bottom,
			gap: parseFloat(css.columnGap),
			pill: box(pill),
			arrow: box(arrow),
			last: element.lastElementChild === group && group.lastElementChild === arrow,
			retry: retry ? box(retry) : null
		};
	});
	expect(layout.last).toBe(true);
	expect(layout.arrow.right, JSON.stringify(layout)).toBeCloseTo(layout.rowEnd, 0);
	expect(layout.arrow.bottom).toBeLessThanOrEqual(layout.rowBottom);
	const previous = layout.retry ?? layout.pill;
	const fits =
		previous.width + layout.gap + layout.arrow.width <= layout.rowEnd - layout.rowStart + 0.5;
	// BUG-150: the chevron joins Retry's line whenever that pair fits, even if the pill wrapped.
	if (!mayWrap || fits) {
		expect(Math.abs(layout.arrow.mid - previous.mid), JSON.stringify(layout)).toBeLessThanOrEqual(
			1
		);
		expect(layout.arrow.left - previous.right).toBeCloseTo(layout.gap, 0);
	} else expect(layout.arrow.top, JSON.stringify(layout)).toBeGreaterThanOrEqual(previous.bottom);
	if (layout.retry) {
		expect(layout.retry.width).toBeGreaterThanOrEqual(24);
		expect(layout.retry.height).toBeGreaterThanOrEqual(24);
		expect(layout.retry.left).toBeGreaterThanOrEqual(layout.rowStart);
		expect(layout.retry.right).toBeLessThanOrEqual(layout.rowEnd);
		expect(layout.retry.top).toBeGreaterThanOrEqual(layout.pill.top);
		const overlap =
			Math.min(layout.retry.bottom, layout.pill.bottom) -
			Math.max(layout.retry.top, layout.pill.top);
		if (overlap > 1) expect(layout.retry.left).toBeGreaterThanOrEqual(layout.pill.right);
		else expect(layout.retry.top).toBeGreaterThanOrEqual(layout.pill.bottom);
	}
	await expectReadableOfficeName(row);
	await expectVisibleStatusText(row);
}

for (const width of [390, 1280]) {
	test(
		`OFC-12 BUG-70 BUG-127 BUG-128: pill then Retry then chevron end-aligned at ${width}px`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width, height: 900 });
			await openOffices(page);
			const row = rows(page).first();
			await expect(row).toContainText('Online');
			await shot(page, `of2-online-order-${width}`, { context: row });
			await expectTrailingOfficeChevron(row, false);
			await visit(page, '/offices?offices=empty&mock=unreachable');
			await expect(row).toContainText('Unreachable');
			await shot(page, `of2-unreachable-order-${width}`, { context: row });
			await expectTrailingOfficeChevron(row, true);
			const retry = row.getByTestId('retry-office');
			await retry.focus();
			await retry.press('Enter');
			await expect(retry).toHaveText('Retrying…');
			await expect(retry).toBeFocused();
			await expect(page.getByRole('dialog')).toHaveCount(0);
			await shot(page, `of2-retrying-order-${width}`, { context: row });
			await expectTrailingOfficeChevron(row, true);
			await expect(retry).toHaveText('Retry');
		}
	);
}

for (const [width, rootFont] of [
	[1280, 16],
	[320, 32]
] as const) {
	test(
		`OFC-13 BUG-70 BUG-128: meta facts have no orphan separator at ${width}px root${rootFont}`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width, height: 1100 });
			await openOffices(page);
			const row = rows(page).first(),
				opener = row.locator('button[aria-haspopup]');
			await expect(row).toContainText('Online');
			await page.addStyleTag({ content: `html { font-size: ${rootFont}px !important; }` });
			await pushOffice(page, {
				host: 'different-reported-machine.local',
				endpoint: '/tmp/test-owned-office.sock',
				status: 'unavailable',
				status_reason: 'unsupported_version'
			});
			await expect(row).toContainText('The release components');
			await shot(page, `of2-meta-${width}-${rootFont}`, { context: row });
			const lines = await opener.evaluate((element) => {
				const title = element.firstElementChild!,
					walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
				const lines: { top: number; text: string }[] = [];
				for (let node = walker.nextNode(); node; node = walker.nextNode()) {
					if (title.contains(node)) continue;
					for (let i = 0; i < (node.textContent?.length ?? 0); i++) {
						const char = node.textContent![i];
						if (!char.trim()) continue;
						const range = document.createRange();
						range.setStart(node, i);
						range.setEnd(node, i + 1);
						const rect = range.getBoundingClientRect();
						if (rect.width <= 0) continue;
						const line = lines.find((line) => Math.abs(line.top - rect.top) < 2);
						if (line) line.text += char;
						else lines.push({ top: rect.top, text: char });
					}
				}
				return lines.map((line) => line.text);
			});
			expect(lines.length).toBeGreaterThan(0);
			for (const line of lines) expect(line, JSON.stringify(lines)).not.toMatch(/^·|·$/);
			const count = row.getByTestId('agent-count');
			await expect(count).toHaveText('7 agents');
			expect(
				await count.evaluate((element) => {
					const range = document.createRange();
					range.selectNodeContents(element);
					return new Set(Array.from(range.getClientRects(), (r) => Math.round(r.top))).size;
				})
			).toBe(1);
		}
	);
}

for (const rootFont of [16, 32, 40])
	for (const offset of [-1, 1]) {
		test(
			`OFC-13 BUG-70 BUG-128: header and row share gutter ${offset < 0 ? 'inside' : 'outside'} compact at root${rootFont}`,
			{ tag: '@webkit' },
			async ({ page }) => {
				const column = 21 * rootFont + offset,
					sidebar = rootFont === 16 ? 0 : settingsChrome(rootFont);
				await page.setViewportSize({ width: column + sidebar, height: 1100 });
				await openOffices(page);
				const row = rows(page).first();
				await expect(row).toContainText('Online');
				await page.addStyleTag({ content: `html { font-size: ${rootFont}px !important; }` });
				await shot(page, `of2-gutter-${rootFont}-${offset}`, { context: row });
				const intro = page.getByText(
					"Where Acme's agents run. HQ keeps the records and approves new offices; every other office keeps a copy.",
					{ exact: true }
				);
				const header = (await intro.boundingBox())!,
					button = (await row.locator('button[aria-haspopup]').boundingBox())!;
				expect((await row.boundingBox())!.width).toBeCloseTo(column, 0);
				expect((await row.boundingBox())!.x).toBeCloseTo(sidebar, 0);
				expect(header.x).toBeCloseTo(button.x, 0);
				const expected =
					offset < 0 ? Math.min(rootFont, column * 0.05) : rootFont * (rootFont === 16 ? 1 : 2);
				const unplaced = page.getByTestId('unplaced-note').getByRole('listitem');
				expect((await unplaced.boundingBox())!.width).toBeCloseTo(column, 0);
				expect((await unplaced.boundingBox())!.x).toBeCloseTo(sidebar, 0);
				expect(button.x - (await row.boundingBox())!.x).toBeCloseTo(expected, 1);
				expect(header.x - (await row.boundingBox())!.x).toBeCloseTo(expected, 1);
			}
		);
	}

for (const status of ['Unreachable', 'Status unknown', 'Update needed']) {
	test(
		`OFC-13 BUG-70 BUG-127 BUG-128: compact ${status} preserves its full word before the decorative chevron fallback at 320px and 200%`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width: 320, height: 1100 });
			await openOffices(page);
			await expect(rows(page).first()).toContainText('Online');
			if (status === 'Unreachable') await visit(page, '/offices?offices=empty&mock=unreachable');
			else if (status === 'Update needed') await pushOffice(page, { status: 'update_needed' });
			else
				await page.evaluate(async () => {
					const path = '/e2e/fakes/office-api.ts';
					const { officeApi } = await import(path);
					const connection = (await officeApi.connections()).at(-1)!;
					(
						window as unknown as { __mockOfficeStatus: (update: StatusUpdate) => void }
					).__mockOfficeStatus({
						kind: 'offices',
						offices: [
							connection.office,
							{
								...connection.office,
								id: 'remote-status',
								name: 'Studio',
								host: 'studio.local',
								endpoint: undefined,
								role: 'satellite'
							}
						]
					});
				});
			const row =
				status === 'Status unknown' ? rows(page).filter({ hasText: 'Studio' }) : rows(page).first();
			await expect(row.locator('[data-status-icon]')).toHaveText(status);
			await page.addStyleTag({ content: 'html { font-size: 200% !important; }' });
			await shot(page, `of2-compact-${status.replaceAll(' ', '-')}`, { context: row });
			expect((await row.boundingBox())!.width).toBeCloseTo(320, 0);
			expect((await row.boundingBox())!.x).toBeCloseTo(0, 0);
			await expectTrailingOfficeChevron(row, status === 'Unreachable', true);
			await expectFullStatusWord(row);
			await expect(row.locator('button[aria-haspopup]')).toHaveAccessibleName(literal(status));
			expect(await row.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
		}
	);
}

for (const status of ['Unreachable', 'Status unknown', 'Update needed']) {
	test(
		`OFC-13 BUG-70 BUG-119: full ${status} at 960px and 40px root`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width: 960, height: 1100 });
			await openOffices(page);
			await expect(rows(page).first()).toContainText('Online');
			if (status === 'Unreachable') await visit(page, '/offices?offices=empty&mock=unreachable');
			else if (status === 'Update needed') await pushOffice(page, { status: 'update_needed' });
			else
				await page.evaluate(async () => {
					const path = '/e2e/fakes/office-api.ts';
					const { officeApi } = await import(path);
					const connection = (await officeApi.connections()).at(-1)!;
					(
						window as unknown as { __mockOfficeStatus: (update: StatusUpdate) => void }
					).__mockOfficeStatus({
						kind: 'offices',
						offices: [
							connection.office,
							{
								...connection.office,
								id: 'remote-status',
								name: 'Studio',
								host: 'studio.local',
								endpoint: undefined,
								role: 'satellite'
							}
						]
					});
				});
			const row =
				status === 'Status unknown' ? rows(page).filter({ hasText: 'Studio' }) : rows(page).first();
			const badge = row.locator('[data-status-icon]'),
				label = badge.locator(':scope > span').last();
			await expect(badge).toHaveText(status);
			await page.addStyleTag({ content: 'html { font-size: 40px !important; }' });
			await expect(row.locator('button[aria-haspopup]')).toHaveAccessibleName(literal(status));
			await expect(label).toHaveAttribute('title', status);
			await shot(page, `of2-bug119-${status.replaceAll(' ', '-')}`, { context: row });
			const text = await label.evaluate((e) => ({ width: e.clientWidth, natural: e.scrollWidth }));
			expect(text.width).toBeGreaterThan(0);
			expect(text.width, JSON.stringify(text)).toBe(text.natural);
		}
	);
}

for (const [width, rootFont] of [
	[320, 16],
	[390, 16],
	[1280, 16],
	[320, 32],
	[320, 40],
	[390, 40],
	[1100, 40]
] as const) {
	test(
		`OFC-12 OFC-13 BUG-70 BUG-150: status flow ends every line at ${width}px root${rootFont}`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await page.setViewportSize({ width, height: 1100 });
			await openOffices(page);
			await visit(page, '/offices?offices=empty&mock=unreachable');
			const row = rows(page).first(),
				retry = row.getByTestId('retry-office');
			await expect(row.locator('[data-status-icon]')).toHaveText('Unreachable');
			await expect(retry).toHaveText('Retry');
			await page.addStyleTag({
				content: `html { font-size: ${rootFont === 32 ? '200%' : `${rootFont}px`} !important; }`
			});
			await expect(page.locator('html')).toHaveCSS('font-size', `${rootFont}px`);
			await shot(page, `bug150-flow-${width}-${rootFont}`, { context: row });
			await expectFullStatusWord(row);
			expect(await retry.evaluate((element) => element.closest('[aria-hidden="true"]'))).toBeNull();
			const layout = await row.evaluate((element) => {
				const box = (item: Element) => {
					const b = item.getBoundingClientRect();
					return {
						left: b.left,
						right: b.right,
						top: b.top,
						bottom: b.bottom,
						width: b.width,
						height: b.height
					};
				};
				const pill = element.querySelector('[data-status-icon]')!,
					retry = element.querySelector('[data-testid=retry-office]')!,
					arrow = element.querySelector(':scope > svg, [data-testid=office-actions] > svg')!;
				const bounds = element.getBoundingClientRect(),
					css = getComputedStyle(element);
				const group = element.querySelector('[data-testid=office-actions]')!,
					groupCss = getComputedStyle(group);
				return {
					start: bounds.left + parseFloat(css.paddingLeft),
					end: bounds.right - parseFloat(css.paddingRight),
					gap: parseFloat(groupCss.columnGap),
					rowGap: parseFloat(css.columnGap),
					pill: box(pill),
					retry: box(retry),
					arrow: box(arrow),
					retryLabel: { width: retry.clientWidth, natural: retry.scrollWidth },
					ordered:
						!!(pill.compareDocumentPosition(retry) & Node.DOCUMENT_POSITION_FOLLOWING) &&
						!!(retry.compareDocumentPosition(arrow) & Node.DOCUMENT_POSITION_FOLLOWING)
				};
			});
			expect(layout.ordered).toBe(true);
			expect(layout.retryLabel.width).toBeGreaterThan(0);
			expect(layout.retryLabel.width).toBeGreaterThanOrEqual(layout.retryLabel.natural);
			const lines: { bottom: number; top: number; end: number }[] = [];
			for (const item of [layout.pill, layout.retry, layout.arrow]) {
				expect(item.left, JSON.stringify(layout)).toBeGreaterThanOrEqual(layout.start - 1);
				expect(item.right, JSON.stringify(layout)).toBeLessThanOrEqual(layout.end + 1);
				const line = lines.find(
					(line) => Math.min(line.bottom, item.bottom) - Math.max(line.top, item.top) > 1
				);
				if (line) line.end = item.right;
				else lines.push({ top: item.top, bottom: item.bottom, end: item.right });
			}
			for (const line of lines)
				expect(Math.abs(line.end - layout.end), JSON.stringify(layout)).toBeLessThanOrEqual(1);
			expect(Math.abs(layout.arrow.right - layout.end), JSON.stringify(layout)).toBeLessThanOrEqual(
				1
			);
			if (layout.retry.width + layout.gap + layout.arrow.width <= layout.end - layout.start) {
				expect(
					Math.min(layout.retry.bottom, layout.arrow.bottom) -
						Math.max(layout.retry.top, layout.arrow.top),
					JSON.stringify(layout)
				).toBeGreaterThan(1);
				expect(
					Math.abs(
						(layout.retry.top + layout.retry.bottom) / 2 -
							(layout.arrow.top + layout.arrow.bottom) / 2
					),
					JSON.stringify(layout)
				).toBeLessThanOrEqual(1);
				expect(
					Math.abs(layout.arrow.left - layout.retry.right - layout.rowGap),
					'Preserve the original chevron gap: ' + JSON.stringify(layout)
				).toBeLessThanOrEqual(1);
			}
		}
	);
}

// ---- The office flows beyond the connected office (§6.1–§6.5), against the offices mock's seeded organization ----

async function openSeeded(page: Page, query = '') {
	await enterMockedOrganization(page, 'Lumen Labs');
	await expect(page).toHaveURL(/\/teams\/members$/);
	await visit(page, `/offices${query}`);
	await expect(page.getByRole('list', { name: 'Offices', exact: true })).toBeVisible();
	await expect(rows(page)).toHaveCount(4);
}
const addOffice = (page: Page) =>
	page.getByRole('main').getByRole('button', { name: 'Add office', exact: true });

test('OFC-1 OFC-5: join requests come first, then HQ and the other offices A–Z, invitations, unplaced and the outbox', async ({
	page
}) => {
	await openSeeded(page);
	await expect(page).toHaveTitle('Offices · Organization settings · Agentic Org');
	await expect(page.getByRole('heading', { level: 2, name: /^Offices/ })).toBeVisible();
	const join = page.getByTestId('join-request');
	await expect(join).toContainText('Laptop wants to join');
	await expect(join).toContainText('from 192.168.1.44');
	await expect(rows(page).nth(0)).toContainText('HQ');
	await expect(rows(page).nth(1)).toContainText('Build box');
	await expect(rows(page).nth(1)).toContainText('last seen 2 h ago');
	await expect(rows(page).nth(1).locator('[data-status-icon]')).toHaveText('Offline');
	await expect(rows(page).nth(2)).toContainText('Lab mini');
	await expect(rows(page).nth(3)).toContainText('Studio Mac');
	await expect(rows(page).nth(3)).toContainText('2 agents');
	const order = await page.evaluate(() =>
		[
			'[data-testid=join-request]',
			'[data-testid=office-row]',
			'[data-testid=invitation-row]',
			'[data-testid=outbox]'
		].map((s) => document.querySelector(s)!.getBoundingClientRect().top)
	);
	expect([...order].sort((a, b) => a - b)).toEqual(order);
	const invitations = page.getByTestId('invitation-row');
	await expect(invitations).toHaveCount(3);
	await expect(invitations.nth(0)).toContainText('Field Mac');
	await expect(invitations.nth(0)).toContainText('Invited');
	await expect(invitations.nth(0)).toContainText(/expires in/);
	await expect(invitations.nth(1)).toContainText('Invitation expired');
	await expect(invitations.nth(2)).toContainText('Join declined');
	await expect(page.getByTestId('outbox')).toContainText('Waiting on offices · 2');
	await expect(page.getByTestId('hq-only-hint')).toHaveCount(0);
	await expect(
		page.getByText('Shown from HQ. Statuses include last-seen information.')
	).toBeVisible();
	await shot(page, 'offices-seeded');
});

test('§6.1: an office that holds features back has a quiet note that opens its details', async ({
	page
}) => {
	await openSeeded(page);
	const note = page.getByTestId('version-note');
	await expect(note).toHaveText(
		'Lab mini runs an older version of Agentic Org, so new features wait until it updates.'
	);
	const opener = note.getByRole('button', { name: 'Lab mini' });
	await opener.click();
	const details = page.getByTestId('office-details');
	await expect(details).toContainText('Update needed');
	await expect(details).toContainText('Agentic Org 0.0.9');
	await page.keyboard.press('Escape');
	await expect(opener).toBeFocused();
});

test('OFC-5 OFC-6: approving a join checks the code and the office starts copying records', async ({
	page
}) => {
	await openSeeded(page);
	await page.getByRole('button', { name: 'Review and approve Laptop' }).click();
	const dialog = page.getByRole('dialog');
	await expect(dialog.getByRole('heading', { name: 'Approve Laptop?' })).toBeVisible();
	await expect(dialog).toContainText('A computer at 192.168.1.44 used the invitation for Laptop');
	const code = dialog.getByLabel('Type the check code shown on Laptop');
	const approve = dialog.getByRole('button', { name: 'Approve Laptop' });
	await code.fill('K7QP4MZ');
	await expect(approve).toBeDisabled();
	await code.fill('K7QP4MZU');
	await expect(dialog).toContainText('Codes use digits and letters, without U');
	await code.fill('AAAA-BBBB');
	await approve.click();
	await expect(dialog.getByRole('alert')).toContainText(
		"That code doesn't match Laptop's. Check its terminal or your trusted session. 2 tries left."
	);
	await code.fill('k7qp-4mzt'); // case and dashes are ignored
	await approve.click();
	await expect(dialog).toBeHidden();
	await expect(page.getByText('Laptop joined Lumen Labs.')).toBeVisible();
	await expect(page.getByTestId('join-request')).toHaveCount(0);
	const laptop = rows(page).filter({ hasText: 'Laptop' });
	await expect(laptop).toContainText('copying records');
	await expect(laptop.locator('[data-status-icon]')).toHaveText('Starting');
});

test('OFC-6: the third wrong code cancels the invitation; Decline asks once', async ({ page }) => {
	await openSeeded(page);
	await page.getByRole('button', { name: 'Review and approve Laptop' }).click();
	const dialog = page.getByRole('dialog');
	const code = dialog.getByLabel('Type the check code shown on Laptop');
	for (const tries of ['2 tries left', '1 try left']) {
		await code.fill('AAAABBBB');
		await dialog.getByRole('button', { name: 'Approve Laptop' }).click();
		await expect(dialog.getByRole('alert')).toContainText(tries);
	}
	await code.fill('AAAABBBB');
	await dialog.getByRole('button', { name: 'Approve Laptop' }).click();
	await expect(dialog).toContainText(
		'Join declined after 3 wrong codes. The invitation is cancelled; create a new invitation to try again.'
	);
	await dialog.getByRole('button', { name: 'Close' }).first().click();
	const cancelled = page.getByTestId('invitation-row').filter({ hasText: 'Laptop' });
	await expect(cancelled).toContainText('Invitation cancelled');
	await expect(cancelled.getByRole('button', { name: 'New invitation for Laptop' })).toBeVisible();
	await expect(
		cancelled.getByRole('button', { name: 'Remove the invitation for Laptop' })
	).toBeVisible();
});

test('§6.3: Decline asks once, then the request becomes a declined invitation', async ({
	page
}) => {
	await openSeeded(page);
	await page.getByRole('button', { name: 'Review and approve Laptop' }).click();
	const dialog = page.getByRole('dialog');
	await dialog.getByRole('button', { name: 'Decline' }).click();
	await expect(dialog.getByRole('heading', { name: 'Decline Laptop?' })).toBeVisible();
	await expect(dialog).toContainText('Its invitation is cancelled.');
	await dialog.getByRole('button', { name: 'Decline' }).click();
	await expect(dialog).toBeHidden();
	await expect(page.getByTestId('join-request')).toHaveCount(0);
	await expect(page.getByTestId('invitation-row').filter({ hasText: 'Laptop' })).toContainText(
		'Join declined'
	);
});

test('OFC-3: inviting another machine shows the invitation once, then an Invited row with Cancel and New invitation', async ({
	page
}) => {
	await openSeeded(page);
	const add = addOffice(page);
	await add.click();
	const dialog = page.getByRole('dialog');
	await dialog.getByLabel('Office name').fill('Design Mac');
	await dialog.getByRole('radio', { name: /On another machine/ }).check();
	await dialog.getByRole('button', { name: 'Create invitation' }).click();
	await expect(dialog.getByRole('heading', { name: 'Invitation for Design Mac' })).toBeVisible();
	await expect(dialog.getByLabel('Invitation')).toHaveValue(/^ao-invite-/);
	await expect(dialog.getByRole('button', { name: 'Copy invitation' })).toBeVisible();
	await expect(dialog).toContainText("It's shown only now.");
	await shot(page, 'offices-invitation', { context: dialog });
	await dialog.getByRole('button', { name: 'Done' }).click();
	await expect(add).toBeFocused();
	const invited = page.getByTestId('invitation-row').filter({ hasText: 'Design Mac' });
	await expect(invited).toContainText('Invited');
	await expect(
		invited.getByRole('button', { name: 'Cancel invitation for Design Mac' })
	).toBeVisible();
	await expect(
		invited.getByRole('button', { name: 'New invitation for Design Mac' })
	).toBeVisible();
});

test('OFC-4: New invitation on a working invitation asks first, then shows the new one once', async ({
	page
}) => {
	await openSeeded(page);
	const field = page.getByTestId('invitation-row').filter({ hasText: 'Field Mac' });
	await field.getByRole('button', { name: 'New invitation for Field Mac' }).click();
	const confirm = page.getByRole('dialog').last();
	await expect(confirm).toContainText('Make a new invitation for Field Mac?');
	await expect(confirm).toContainText('The previous invitation stops working.');
	await confirm.getByRole('button', { name: 'New invitation' }).click();
	const issued = page.getByRole('dialog').filter({ hasText: 'New invitation for Field Mac' });
	await expect(issued.getByLabel('Invitation')).toHaveValue(/^ao-invite-/);
	await issued.getByRole('button', { name: 'Done' }).click();
	await expect(page.getByRole('dialog')).toHaveCount(0);
});

test('§6.3: cancelling an invitation asks first and focuses the next row', async ({ page }) => {
	await openSeeded(page);
	const field = page.getByTestId('invitation-row').filter({ hasText: 'Field Mac' });
	await field.getByRole('button', { name: 'Cancel invitation for Field Mac' }).click();
	const confirm = page.getByRole('dialog').last();
	await expect(confirm).toContainText('Its link stops working. Nothing is added.');
	await confirm.getByRole('button', { name: 'Cancel invitation' }).click();
	await expect(page.getByTestId('invitation-row').filter({ hasText: 'Field Mac' })).toHaveCount(0);
	// The next row (Old mini) takes focus, never the body.
	await expect(
		page.getByTestId('invitation-row').filter({ hasText: 'Old mini' }).getByRole('button').first()
	).toBeFocused();
});

test('OFC-2: adding an office on this instance shows Creating, Synchronizing and Ready', async ({
	page
}) => {
	await openSeeded(page);
	await addOffice(page).click();
	const dialog = page.getByRole('dialog');
	await dialog.getByLabel('Office name').fill('Office 2');
	await dialog.getByRole('radio', { name: /On this instance/ }).check();
	await dialog.getByRole('button', { name: 'Add office' }).click();
	const progress = dialog.getByTestId('add-progress');
	await expect(progress).toContainText('Creating…');
	await expect(progress).toContainText('Synchronizing…', { timeout: 5_000 });
	await expect(page.getByText("Office 2 added. It's starting.")).toBeVisible({ timeout: 10_000 });
	await expect(rows(page).filter({ hasText: 'Office 2' })).toHaveCount(1);
});

test('OFC-2: a name already used is refused on the field and keeps the input', async ({ page }) => {
	await openSeeded(page);
	await addOffice(page).click();
	const dialog = page.getByRole('dialog');
	const name = dialog.getByLabel('Office name');
	await name.fill('Studio Mac');
	await dialog.getByRole('button', { name: 'Add office' }).click();
	await expect(dialog.getByRole('alert')).toHaveText(
		"There's already an office called Studio Mac. Choose another name."
	);
	await expect(name).toHaveValue('Studio Mac');
	await expect(name).toBeFocused();
	await expect(name).toHaveAttribute('aria-invalid', 'true');
});

test('OFC-7: an owner who is not the administrator gets the invitation straight away', async ({
	page
}) => {
	await openSeeded(page, '?admin=other');
	await addOffice(page).click();
	const dialog = page.getByRole('dialog');
	await expect(
		dialog.getByRole('heading', { name: 'Add an office on another machine' })
	).toBeVisible();
	await expect(dialog.getByRole('radio')).toHaveCount(0);
	await expect(dialog.getByRole('button', { name: 'Create invitation' })).toBeVisible();
});

test('OFC-29 OFC-35 OFC-37: a wait for running agents leaves a pending row whose Cancel asks first', async ({
	page
}) => {
	await openSeeded(page, '?add=waiting');
	await addOffice(page).click();
	const dialog = page.getByRole('dialog');
	await dialog.getByLabel('Office name').fill('Office 2');
	await dialog.getByRole('button', { name: 'Add office' }).click();
	await expect(dialog.getByTestId('add-waiting')).toContainText(
		'Waiting for 3 agents to stop: Kai Lindqvist, Juno Park and Noor Haddad.'
	);
	await expect(dialog.getByLabel('Office name')).toHaveAttribute('readonly', '');
	await dialog.getByRole('button', { name: 'Close' }).first().click();
	const pending = page.getByTestId('pending-add');
	await expect(pending).toContainText('Waiting for 3 agents to stop · on HQ instance');
	await expect(pending.locator('[data-status-icon]')).toHaveCount(0);
	await pending.getByRole('button', { name: 'Cancel adding Office 2' }).click();
	const confirm = page.getByRole('dialog').last();
	await expect(confirm).toContainText('Stop adding Office 2?');
	await expect(confirm).toContainText("It wasn't added yet.");
	await confirm.getByRole('button', { name: 'Stop adding' }).click();
	await expect(page.getByText("Office 2 wasn't added.")).toBeVisible();
	await expect(page.getByTestId('pending-add')).toHaveCount(0);
});

test('OFC-8 OFC-10: removing an office names its cost, then its agents are unplaced with Place… and Offboard…', async ({
	page
}) => {
	await openSeeded(page);
	const studio = rows(page).filter({ hasText: 'Studio Mac' });
	await studio.getByRole('button').first().click();
	await page.getByTestId('remove-office').click();
	const confirm = page.getByRole('dialog').last();
	await expect(
		confirm.getByRole('heading', { name: 'Remove Studio Mac from Lumen Labs?' })
	).toBeVisible();
	await expect(confirm).toContainText('Its 2 agents stop running there and become unplaced');
	await expect(confirm).toContainText('HQ takes back their 5 tasks.');
	await shot(page, 'offices-remove', { context: confirm });
	await confirm.getByRole('button', { name: 'Remove Studio Mac' }).click();
	await expect(page.getByText('Studio Mac was removed.')).toBeVisible();
	await expect(rows(page).filter({ hasText: 'Studio Mac' })).toHaveCount(0);
	const unplaced = page.getByTestId('unplaced-note');
	await expect(unplaced.getByRole('heading', { name: /^Unplaced · \d+ agents$/ })).toBeFocused();
	for (const name of ['Noor Haddad', 'Kai Lindqvist']) {
		await expect(unplaced.getByRole('button', { name: `Place ${name}…` })).toBeVisible();
		await expect(unplaced.getByRole('button', { name: `Offboard ${name}…` })).toBeVisible();
	}
});

test('§6.1: an office is renamed from its details', async ({ page }) => {
	await openSeeded(page);
	await rows(page).filter({ hasText: 'Build box' }).getByRole('button').first().click();
	const details = page.getByTestId('office-details');
	await details.getByRole('button', { name: 'Rename Build box' }).click();
	const field = details.getByLabel('Office name');
	await field.fill('Studio Mac');
	await details.getByRole('button', { name: 'Save' }).click();
	await expect(details.getByRole('alert')).toContainText(
		"There's already an office called Studio Mac."
	);
	await field.fill('Build server');
	await details.getByRole('button', { name: 'Save' }).click();
	await expect(details.getByRole('button', { name: 'Rename Build server' })).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(rows(page).filter({ hasText: 'Build server' })).toHaveCount(1);
});

test('OFC-9: a blocked command says why; Retry says it is still offline, Discard asks first', async ({
	page
}) => {
	await openSeeded(page);
	const outbox = page.getByTestId('outbox');
	await expect(outbox).toContainText('Assign T-A12 to Pax Moreau');
	await expect(outbox).toContainText('Blocked');
	await expect(outbox).toContainText("Build box is offline, so HQ can't confirm it.");
	await expect(outbox).toContainText('Pending');
	await outbox.getByRole('button', { name: 'Retry: Assign T-A12 to Pax Moreau' }).click();
	await expect(
		page.getByText("Build box is still offline. Try again when it's back.")
	).toBeVisible();
	await outbox.getByRole('button', { name: 'Discard: Assign T-A12 to Pax Moreau' }).click();
	const confirm = page.getByRole('dialog').last();
	await expect(confirm).toContainText("Assign T-A12 to Pax Moreau won't be sent to Build box.");
	await confirm.getByRole('button', { name: 'Discard' }).click();
	await expect(outbox).toContainText('Waiting on offices · 1');
});

test('§6a: while the other offices load, a skeleton row says so', async ({ page }) => {
	await enterMockedOrganization(page, 'Lumen Labs');
	await visit(page, '/offices?mock=slow');
	await expect(page.getByTestId('offices-loading')).toContainText('Loading the other offices…');
	await expect(addOffice(page)).toBeDisabled();
	await shot(page, 'offices-loading', { settle: false });
	await expect(page.getByTestId('offices-loading')).toHaveCount(0, { timeout: 10_000 });
	await expect(rows(page)).toHaveCount(4);
});

test('§6a: when the other offices can’t be read, the list stays and an alert offers Retry', async ({
	page
}) => {
	await enterMockedOrganization(page, 'Lumen Labs');
	await visit(page, '/offices?offices=fails');
	const alert = page.getByTestId('overview-error');
	await expect(alert).toContainText(
		"Can't read join requests, invitations and offices that joined"
	);
	await expect(rows(page)).toHaveCount(1); // HQ, from the connection
	await expect(addOffice(page)).toBeDisabled();
	await expect(addOffice(page)).toHaveAccessibleDescription(/Can't read join requests/);
	await shot(page, 'offices-error');
	await alert.getByRole('button', { name: 'Retry' }).click();
	await expect(alert.getByRole('button')).toHaveText(/Retry/);
});

test('§0.1: where the web API does not serve the office flows, the page says so and offers no dead control', async ({
	page
}) => {
	await enterMockedOrganization(page, 'Lumen Labs');
	await visit(page, '/offices?offices=unavailable');
	await expect(page.getByTestId('offices-unavailable')).toHaveText(
		"Adding, approving and removing offices isn't built yet. It's coming in this release."
	);
	await expect(addOffice(page)).toHaveCount(0);
	await expect(rows(page)).toHaveCount(1);
	await expect(
		page.getByTestId('unplaced-note').getByRole('button', { name: /^Place / })
	).toHaveCount(0);
	await rows(page).first().getByRole('button').first().click();
	await expect(
		page.getByTestId('office-details').getByRole('button', { name: /^Rename/ })
	).toHaveCount(0);
	await shot(page, 'offices-unavailable');
});

for (const width of [320, 390, 640, 1024, 1440]) {
	test(`§2: the seeded page reflows without sideways scroll at ${width}px`, async ({ page }) => {
		await page.setViewportSize({ width, height: 900 });
		await openSeeded(page);
		expect(
			await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)
		).toBe(true);
		if (width < 640) {
			for (const button of await page.getByRole('main').getByRole('button').all()) {
				if (!(await button.isVisible())) continue;
				const box = (await button.boundingBox())!;
				expect(box.height, await button.innerText()).toBeGreaterThanOrEqual(44);
			}
		}
		await shot(page, `offices-seeded-${width}`);
	});
}

// ---- Review fixes: focus never falls to the page when the row that opened a dialog is gone (OF-14, §6a) ----

const focusedIsNotBody = (page: Page) =>
	page.evaluate(() => document.activeElement !== document.body && document.activeElement !== null);

test('OF-14: after approving a join, focus moves to the new office’s row', async ({ page }) => {
	await openSeeded(page);
	await page.getByRole('button', { name: 'Review and approve Laptop' }).click();
	const dialog = page.getByRole('dialog');
	await dialog.getByLabel('Type the check code shown on Laptop').fill('K7QP4MZT');
	await dialog.getByRole('button', { name: 'Approve Laptop' }).click();
	await expect(dialog).toBeHidden();
	await expect(
		rows(page).filter({ hasText: 'Laptop' }).locator('button[aria-haspopup]')
	).toBeFocused();
});

test('OF-14: after declining a join, focus moves to its declined invitation', async ({ page }) => {
	await openSeeded(page);
	await page.getByRole('button', { name: 'Review and approve Laptop' }).click();
	const dialog = page.getByRole('dialog');
	await dialog.getByRole('button', { name: 'Decline' }).click();
	await dialog.getByRole('button', { name: 'Decline' }).click();
	await expect(dialog).toBeHidden();
	await expect(
		page
			.getByTestId('invitation-row')
			.filter({ hasText: 'Laptop' })
			.getByRole('button', { name: 'New invitation for Laptop' })
	).toBeFocused();
});

test('OF-14 OF-13: renewing an ended invitation is busy once, then focus lands on its new row', async ({
	page
}) => {
	await openSeeded(page);
	const renew = page
		.getByTestId('invitation-row')
		.filter({ hasText: 'Old mini' })
		.getByRole('button', { name: 'New invitation for Old mini' });
	await renew.dblclick(); // a double click makes one invitation
	const issued = page.getByRole('dialog').filter({ hasText: 'New invitation for Old mini' });
	await expect(issued.getByLabel('Invitation')).toHaveValue(/^ao-invite-/);
	await issued.getByRole('button', { name: 'Done' }).click();
	await expect(page.getByRole('dialog')).toHaveCount(0);
	expect(await focusedIsNotBody(page)).toBe(true);
	await expect(
		page
			.getByTestId('invitation-row')
			.filter({ hasText: 'Old mini' })
			.getByRole('button', { name: 'Cancel invitation for Old mini' })
	).toBeFocused();
	await expect(page.getByText("That invitation isn't here any more.")).toHaveCount(0);
});

test('OF-15: the Approve dialog fits at 320 px with 200% text', async ({ page }) => {
	await page.setViewportSize({ width: 320, height: 900 });
	await openSeeded(page);
	await page.addStyleTag({ content: 'html { font-size: 200% !important; }' });
	await page.getByRole('button', { name: 'Review and approve Laptop' }).click();
	const dialog = page.getByRole('dialog');
	await expect(dialog.getByRole('button', { name: 'Approve Laptop' })).toBeVisible();
	await shot(page, 'offices-approve-320-200', { context: dialog });
	const outside = await dialog.evaluate((root) =>
		[root, ...root.querySelectorAll('*')]
			.filter((e) => {
				const r = e.getBoundingClientRect();
				return r.width > 0 && (r.right > window.innerWidth + 0.5 || r.left < -0.5);
			})
			.map((e) => e.tagName + (e.textContent ?? '').slice(0, 20))
	);
	expect(outside).toEqual([]);
});

test('OF-9: while the agents can’t be counted, Remove says so instead of hiding the cost', async ({
	page
}) => {
	await enterMockedOrganization(page, 'Lumen Labs');
	await visit(page, '/offices?org=fails');
	await expect(rows(page)).toHaveCount(4);
	await rows(page).filter({ hasText: 'Studio Mac' }).getByRole('button').first().click();
	await page.getByTestId('remove-office').click();
	const confirm = page.getByRole('dialog').last();
	await expect(confirm).toContainText("Its agents couldn't be counted");
	await expect(confirm).toContainText('becomes unplaced');
});

test('OFC-2: a failed progress check offers Check status, never a second add', async ({ page }) => {
	await openSeeded(page, '?add=check-fails');
	await addOffice(page).click();
	const dialog = page.getByRole('dialog');
	await dialog.getByLabel('Office name').fill('Office 2');
	await dialog.getByRole('button', { name: 'Add office' }).click();
	await expect(dialog.getByTestId('add-unchecked')).toContainText(
		"Can't check how adding Office 2 is going"
	);
	await expect(dialog.getByRole('button', { name: 'Try again' })).toHaveCount(0);
	await dialog.getByRole('button', { name: 'Check status' }).click();
	await expect(page.getByText("Office 2 added. It's starting.")).toBeVisible({ timeout: 10_000 });
	await expect(rows(page).filter({ hasText: 'Office 2' })).toHaveCount(1);
});

test('OFC-13: a status change of an office that joined is announced in the spec’s words', async ({
	page
}) => {
	await openSeeded(page);
	await page.getByRole('button', { name: 'Review and approve Laptop' }).click();
	const dialog = page.getByRole('dialog');
	await dialog.getByLabel('Type the check code shown on Laptop').fill('K7QP4MZT');
	await dialog.getByRole('button', { name: 'Approve Laptop' }).click();
	await expect(rows(page).filter({ hasText: 'Laptop' }).locator('[data-status-icon]')).toHaveText(
		'Starting'
	);
	await page.waitForTimeout(6_500); // the mock finishes Laptop's first copy of records
	// Any action that reads the offices again shows the change.
	const outbox = page.getByTestId('outbox');
	await outbox.getByRole('button', { name: 'Discard: Assign T-A12 to Pax Moreau' }).click();
	await page.getByRole('dialog').last().getByRole('button', { name: 'Discard' }).click();
	await expect(page.getByTestId('office-changes')).toHaveText('Laptop is online');
});

for (const outcome of ['arrive', 'fail again'] as const) {
	test(
		`BUG-166 BUG-70: pointer count Retry keeps focus while working; when counts ${outcome} focus ${outcome === 'arrive' ? 'goes to Offices heading' : 'stays on Retry'}`,
		{ tag: '@webkit' },
		async ({ page }) => {
			await openOffices(page);
			await visit(page, '/offices?offices=empty&org=fails_once');
			await expect(page.getByTestId('count-error')).toBeVisible();
			await page.evaluate(async (fail) => {
				const modulePath = '/e2e/fakes/org-api.ts';
				const { orgApi } = (await import(modulePath)) as {
					orgApi: { view: (id: string) => Promise<unknown> };
				};
				const view = orgApi.view;
				let held = false;
				let release!: () => void;
				const gate = new Promise<void>((resolve) => {
					release = resolve;
				});
				(window as unknown as { __releaseCounts: () => void }).__releaseCounts = release;
				orgApi.view = async (id: string) => {
					if (!held) {
						held = true;
						await gate;
						if (fail) throw new Error('Still unreadable.');
					}
					return view(id);
				};
			}, outcome === 'fail again');
			const retry = page.getByTestId('count-error').getByRole('button', { name: /Retry|Retrying/ });
			await retry.click();
			await expect(retry).toHaveAttribute('aria-busy', 'true');
			await expect(retry).toBeFocused();
			await page.evaluate(() =>
				(window as unknown as { __releaseCounts: () => void }).__releaseCounts()
			);
			if (outcome === 'arrive') {
				await expect(page.getByTestId('count-error')).toHaveCount(0);
				await expect(page.locator('#offices-heading')).toBeFocused();
			} else {
				await expect(retry).toHaveAttribute('aria-busy', 'false');
				await expect(page.getByTestId('count-error')).toContainText('Still unreadable.');
				await expect(retry).toBeFocused();
				await expect(page.locator('#offices-heading')).not.toBeFocused();
			}
		}
	);
}

test(
	'BUG-166 BUG-70: pointer Retry on an unreachable office stays focused while it works',
	{ tag: '@webkit' },
	async ({ page }) => {
		await openOffices(page);
		await visit(page, '/offices?offices=empty&mock=unreachable');
		const retry = rows(page).first().getByTestId('retry-office');
		await expect(retry).toHaveAccessibleName('Retry reaching HQ');
		await retry.click();
		await expect(retry).toHaveText('Retrying…');
		await expect(retry).toBeFocused();
		await expect(retry).toHaveText('Retry', { timeout: 15_000 });
		await expect(retry).toBeFocused();
	}
);
