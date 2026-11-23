import { expect, test } from '@playwright/test';
import { enterMockedOrganization } from './mock-organization';
import { visit } from './ready';

const captureDir = process.env.CAPTURE_EXEC_SHOTS_DIR;

test('Executive UI v6 (always-on): 6 themes, Home KPIs, 3-lens Executive Brief, Inbox, Activity, and Ask', async ({
	page
}) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await enterMockedOrganization(page, 'Acme Executive');

	// Always-on Executive UI: lands directly in executive mode with Ocean default theme
	await expect(page.locator('html')).toHaveAttribute('data-ui-mode', 'executive');
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'ocean');

	// Home loads live domain KPIs, Project Scorecard, and 3-lens Executive Brief
	await visit(page, '/home');
	await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
	await expect(page.getByText('Financials')).toBeVisible();
	await expect(page.getByText('Workforce & Offices')).toBeVisible();
	await expect(page.getByText('Work & Flow')).toBeVisible();
	await expect(page.getByRole('heading', { name: 'Executive Brief' })).toBeVisible();

	// 3-lens Executive Brief switching (Today / 7-Day / Board) + Copy brief -> Copied
	await page.getByRole('tab', { name: '7-Day', exact: true }).click();
	await expect(page.getByText('Velocity & Delivery')).toBeVisible();
	await page.getByRole('tab', { name: 'Board', exact: true }).click();
	await expect(page.getByText('Executive Summary')).toBeVisible();
	await page.getByRole('tab', { name: 'Today', exact: true }).click();
	await expect(page.getByText('Live Status & Highlights')).toBeVisible();

	const copyBtn = page.getByRole('button', { name: 'Copy brief' });
	await copyBtn.click();
	await expect(page.getByRole('button', { name: 'Copied' })).toBeVisible();

	// Verify Projects is last under Workspace and Teams is reachable in Organization
	const navLinks = await page
		.getByRole('navigation', { name: 'Main' })
		.getByRole('link')
		.allInnerTexts();
	const cleanLinks = navLinks.map((t) => t.trim().split('\n')[0]);
	const workIdx = cleanLinks.indexOf('Work');
	const chatIdx = cleanLinks.indexOf('Chat');
	const actIdx = cleanLinks.indexOf('Activity');
	const projIdx = cleanLinks.indexOf('Projects');
	expect(workIdx).toBeGreaterThan(-1);
	expect(chatIdx).toBeGreaterThan(workIdx);
	expect(actIdx).toBeGreaterThan(chatIdx);
	expect(projIdx).toBeGreaterThan(actIdx);
	expect(cleanLinks).toContain('Teams');

	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/prod-home-ocean.png` });
	}

	// Switch across all 6 themes via sidebar theme switcher
	await page
		.getByRole('group', { name: 'Theme' })
		.getByRole('button', { name: 'Graphite', exact: true })
		.click();
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'graphite');
	await expect(page.locator('.cozy-room')).toBeHidden();
	if (captureDir) {
		await expect(page.locator('.exec-cta-glass').first()).toHaveCSS(
			'background-color',
			'rgb(94, 82, 161)'
		);
		await page.screenshot({ path: `${captureDir}/prod-home-graphite.png` });
	}

	await page
		.getByRole('group', { name: 'Theme' })
		.getByRole('button', { name: 'Cozy', exact: true })
		.click();
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'cozy');
	await expect(page.locator('.cozy-room')).toBeVisible();
	const askRadius = await page
		.getByRole('button', { name: 'Ask', exact: true })
		.evaluate((el) => getComputedStyle(el).borderRadius);
	expect(askRadius).toBe('9999px');
	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/prod-home-cozy.png` });
	}

	await page
		.getByRole('group', { name: 'Theme' })
		.getByRole('button', { name: 'Report', exact: true })
		.click();
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'report');
	await expect(page.locator('.cozy-room')).toBeHidden();
	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/prod-home-report.png` });
	}

	await page
		.getByRole('group', { name: 'Theme' })
		.getByRole('button', { name: 'Night', exact: true })
		.click();
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'report-dark');
	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/prod-home-report-dark.png` });
	}

	await page
		.getByRole('group', { name: 'Theme' })
		.getByRole('button', { name: 'Classic', exact: true })
		.click();
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'classic');
	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/prod-home-classic.png` });
	}

	await page
		.getByRole('group', { name: 'Theme' })
		.getByRole('button', { name: 'Ocean', exact: true })
		.click();
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'ocean');

	// Command bar Ask Org AI modal
	await page.getByRole('button', { name: 'Ask', exact: true }).click();
	const dialog = page.getByRole('dialog', { name: 'Command palette and Org AI' });
	await expect(dialog).toBeVisible();
	await dialog.getByRole('button', { name: 'Team & office status' }).click();
	await expect(dialog).toContainText('agents active');
	await dialog.getByRole('button', { name: 'ESC' }).click();
	await expect(dialog).toBeHidden();

	// Inbox renders split master-detail approval & triage workspace from inboxApi
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: /Inbox/ }).click();
	await expect(page).toHaveURL(/\/inbox$/);
	await expect(page.getByRole('heading', { level: 1, name: 'Inbox' })).toBeVisible();
	await expect(page.getByRole('main').getByText('waiting on you')).toBeVisible();
	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/prod-inbox-ocean.png` });
	}

	// Activity lands on Assembly Line Matrix FIRST by default, with toggle to Activity stream
	await page
		.getByRole('navigation', { name: 'Main' })
		.getByRole('link', { name: 'Activity' })
		.click();
	await expect(page).toHaveURL(/\/activity$/);
	await expect(page.getByRole('heading', { level: 1, name: 'Activity' })).toBeVisible();
	await expect(
		page.getByRole('main').getByRole('button', { name: 'Assembly line', exact: true })
	).toHaveAttribute('aria-pressed', 'true');
	await expect(page.getByRole('region', { name: 'Assembly line matrix' })).toBeVisible();
	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/prod-activity-assembly-ocean.png` });
	}
	await page.getByRole('main').getByRole('button', { name: 'Activity', exact: true }).click();
	await expect(
		page.getByRole('main').getByRole('button', { name: 'Activity', exact: true })
	).toHaveAttribute('aria-pressed', 'true');

	// Settings shows 6-theme Appearance selector + Cozy Glass High Contrast toggle (no legacy toggle)
	await page
		.getByRole('navigation', { name: 'Main' })
		.getByRole('link', { name: 'Settings' })
		.click();
	await expect(page).toHaveURL(/\/settings$/);
	await expect(page.getByTestId('executive-ui-toggle')).toHaveCount(0);
	await expect(page.getByTestId('theme-option-graphite')).toBeVisible();
	await expect(page.getByTestId('theme-option-cozy')).toBeVisible();
	await expect(page.getByTestId('theme-option-report')).toBeVisible();

	await page.getByTestId('theme-option-cozy').click();
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'cozy');
	const cozyContrastToggle = page.getByTestId('cozy-high-contrast-toggle');
	await expect(cozyContrastToggle).toHaveAttribute('aria-checked', 'false');
	await cozyContrastToggle.click();
	await expect(cozyContrastToggle).toHaveAttribute('aria-checked', 'true');
	await expect(page.locator('html')).toHaveAttribute('data-cozy-contrast', 'high');

	if (captureDir) {
		await page
			.getByRole('navigation', { name: 'Main' })
			.getByRole('link', { name: 'Home' })
			.click();
		await expect(page).toHaveURL(/\/home$/);
		await expect(page.getByRole('heading', { name: 'Executive Brief' })).toBeVisible();
		await page.screenshot({ path: `${captureDir}/prod-home-cozy-hc.png` });
		await page
			.getByRole('navigation', { name: 'Main' })
			.getByRole('link', { name: 'Settings' })
			.click();
		await expect(page).toHaveURL(/\/settings$/);
	}

	await page.getByTestId('cozy-high-contrast-toggle').click();
	await expect(page.getByTestId('cozy-high-contrast-toggle')).toHaveAttribute(
		'aria-checked',
		'false'
	);
	await expect(page.locator('html')).not.toHaveAttribute('data-cozy-contrast', 'high');
});

test('Home scroll cue: appears when below-fold content exists, scrolls smoothly, and hides on scroll', async ({
	page
}) => {
	await page.setViewportSize({ width: 1440, height: 700 });
	await visit(page, '/home?theme=graphite&mock=poc');

	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'graphite');
	await expect(page.getByRole('heading', { name: 'Executive Brief' })).toBeVisible();

	const hint = page.getByTestId('home-scroll-hint');
	await expect(hint).toBeVisible();
	await expect(hint).toContainText('Scroll for more');

	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/poc-home-scroll-cue-graphite.png` });
	}

	await hint.click();
	await expect.poll(() => page.locator('main').evaluate((el) => el.scrollTop)).toBeGreaterThan(16);
	await expect(hint).toBeHidden();
});

test('POC-mirror dataset (?mock=poc): auto-seeds Agentic Org and mirrors poc/v6.html data', async ({
	page
}) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await visit(page, '/home?theme=report&mock=poc');

	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'report');
	await expect(page.getByText('Agentic Org').first()).toBeVisible();
	await expect(page.getByText('$142')).toBeVisible();
	await expect(page.getByText('/ $180 monthly')).toBeVisible();
	await expect(page.getByText('Office federation').first()).toBeVisible();
	await expect(page.getByText('Agentic Org v1.0').first()).toBeVisible();
	await expect(page.getByText('Look & feel refresh').first()).toBeVisible();
	await expect(page.getByText("Approve Kai's sync")).toBeVisible();

	// Persists across route navigation without re-specifying ?mock=poc
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: /Inbox/ }).click();
	await expect(page).toHaveURL(/\/inbox$/);
	await expect(
		page.getByRole('heading', { level: 2, name: "Approve publishing Kai's sync update to v1.0" })
	).toBeVisible();
	await expect(page.getByText("Reroute Jules's 3 queued tasks to Maya & Rowan")).toBeVisible();
	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/poc-mirror-inbox-report.png` });
	}

	// Activity Assembly Line in Report Day shows POC projects first by default
	await page
		.getByRole('navigation', { name: 'Main' })
		.getByRole('link', { name: 'Activity' })
		.click();
	await expect(page).toHaveURL(/\/activity$/);
	await expect(page.getByRole('region', { name: 'Assembly line matrix' })).toBeVisible();
	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/poc-mirror-activity-assembly-report.png` });
	}

	// Opening a POC work item exercises workApi.get() & workApi.activity() without error
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Work' }).click();
	await expect(page).toHaveURL(/\/work$/);
	await page.getByRole('button', { name: 'Custody lock fix' }).click();
	await expect(page.getByRole('dialog')).toContainText('Custody lock fix');
	await expect(page.getByRole('dialog')).toContainText('Implementation and unit checks complete');

	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/poc-mirror-work-sheet-report.png` });
	}
});
