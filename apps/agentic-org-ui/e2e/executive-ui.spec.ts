import { expect, test } from '@playwright/test';
import { enterMockedOrganization } from './mock-organization';
import { visit } from './ready';

const captureDir = process.env.CAPTURE_EXEC_SHOTS_DIR;

test('Executive UI v2 feature flag, 3 themes (Ocean/Light/Classic), Home, Inbox, Activity, and Org AI', async ({
	page
}) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await enterMockedOrganization(page, 'Acme Executive');

	// Default: legacy mode is active
	await expect(page.locator('html')).toHaveAttribute('data-ui-mode', 'legacy');

	// Visit Settings and flip the Executive UI (v2) flag ON
	await visit(page, '/settings');
	const flagToggle = page.getByTestId('executive-ui-toggle');
	await expect(flagToggle).toHaveAttribute('aria-checked', 'false');
	await flagToggle.click();
	await expect(flagToggle).toHaveAttribute('aria-checked', 'true');
	await expect(page.locator('html')).toHaveAttribute('data-ui-mode', 'executive');
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'ocean');

	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/prod-settings-ocean.png` });
	}

	// Open Executive Home and wait for live KPIs to load
	await page.getByRole('button', { name: 'Open Executive Home' }).click();
	await expect(page).toHaveURL(/\/home$/);
	await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
	await expect(page.getByText('Work & Flow')).toBeVisible();
	await expect(page.getByText('Catch-up')).toBeVisible();

	// Verify Projects is the last item under Workspace in the Executive Sidebar
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

	// Capture Ocean Home
	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/prod-home-ocean.png` });
	}

	// Switch to Light theme via sidebar theme switcher
	await page.getByRole('group', { name: 'Theme' }).getByRole('button', { name: 'Light' }).click();
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'light');
	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/prod-home-light.png` });
	}

	// Switch to Classic (v0 Flat) theme via sidebar theme switcher
	await page.getByRole('group', { name: 'Theme' }).getByRole('button', { name: 'Classic' }).click();
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'classic');
	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/prod-home-classic.png` });
	}

	// Switch back to Ocean and test Ask Org AI modal
	await page.getByRole('group', { name: 'Theme' }).getByRole('button', { name: 'Ocean' }).click();
	await page.getByRole('button', { name: 'Ask AI' }).click();
	const dialog = page.getByRole('dialog', { name: 'Command palette and Org AI' });
	await expect(dialog).toBeVisible();
	await dialog.getByRole('button', { name: 'Team & office status' }).click();
	await expect(dialog).toContainText('agents active');
	await dialog.getByRole('button', { name: 'ESC' }).click();
	await expect(dialog).toBeHidden();

	// Navigate to Inbox & Activity
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Inbox' }).click();
	await expect(page).toHaveURL(/\/inbox$/);
	await expect(page.getByRole('heading', { level: 1, name: 'Inbox' })).toBeVisible();

	await page
		.getByRole('navigation', { name: 'Main' })
		.getByRole('link', { name: 'Activity' })
		.click();
	await expect(page).toHaveURL(/\/activity$/);
	await expect(page.getByRole('heading', { level: 1, name: 'Activity' })).toBeVisible();

	// Turn Executive UI flag back OFF in Settings and confirm legacy shell returns
	await page
		.getByRole('navigation', { name: 'Main' })
		.getByRole('link', { name: 'Settings' })
		.click();
	await page.getByTestId('executive-ui-toggle').click();
	await expect(page.locator('html')).toHaveAttribute('data-ui-mode', 'legacy');
});
