import { expect, test } from '@playwright/test';
import { enterMockedOrganization } from './mock-organization';
import { visit } from './ready';

const captureDir = process.env.CAPTURE_EXEC_SHOTS_DIR;

test('Executive UI v6 (always-on): 4 themes, Home KPIs, 3-lens Executive Brief, Inbox, Activity, and Ask', async ({
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

	// Switch across all 4 themes via sidebar theme switcher
	await page
		.getByRole('group', { name: 'Theme' })
		.getByRole('button', { name: 'Report', exact: true })
		.click();
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'report');
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

	// Inbox renders real pending decisions from inboxApi (never empty by default)
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: /Inbox/ }).click();
	await expect(page).toHaveURL(/\/inbox$/);
	await expect(page.getByRole('heading', { level: 1, name: 'Inbox' })).toBeVisible();
	await expect(page.getByText('waiting on you')).toBeVisible();

	// Activity renders canonical activity feed
	await page
		.getByRole('navigation', { name: 'Main' })
		.getByRole('link', { name: 'Activity' })
		.click();
	await expect(page).toHaveURL(/\/activity$/);
	await expect(page.getByRole('heading', { level: 1, name: 'Activity' })).toBeVisible();

	// Settings shows 4-theme Appearance selector (no legacy toggle)
	await page
		.getByRole('navigation', { name: 'Main' })
		.getByRole('link', { name: 'Settings' })
		.click();
	await expect(page).toHaveURL(/\/settings$/);
	await expect(page.getByTestId('executive-ui-toggle')).toHaveCount(0);
	await expect(page.getByTestId('theme-option-report')).toBeVisible();
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
	await expect(page.getByText("Approve publishing Kai's sync update to v1.0")).toBeVisible();
	await expect(page.getByText("Reroute Jules's 3 queued tasks to Maya & Rowan")).toBeVisible();

	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/poc-mirror-inbox-report.png` });
	}
});
