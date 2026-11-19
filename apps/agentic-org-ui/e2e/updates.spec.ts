import { expect, test, type Page } from '@playwright/test';
import { visit } from './ready';
import { signInAsAdmin } from './web-session';

// Organization settings › Updates against the mock updates API (app-shell-ux §0.1; server-installation-and-access-ux
// §5; .agent-kit/tasks/v1-ui-updates-contract.md UPD-1 to UPD-7).
async function openOrganization(page: Page) {
	await signInAsAdmin(page, [
		{
			id: 'org-updates',
			name: 'Lumen Labs',
			created_at: '2026-11-18T00:00:00Z',
			recovery: 'acknowledged'
		}
	]);
	await visit(page, '/create');
	await page.getByLabel('Organization name').fill('Lumen Labs');
	await page.getByRole('button', { name: 'Create organization' }).click();
	await expect(page).not.toHaveURL(/\/create$/);
}

const offices = (page: Page) => page.getByTestId('office-versions').locator('> li');
const checkDetail = (page: Page) => page.getByTestId('check-detail');

async function noSidewaysScroll(page: Page) {
	const overflow = await page.evaluate(
		() => document.documentElement.scrollWidth - document.documentElement.clientWidth
	);
	expect(overflow).toBeLessThanOrEqual(0);
}

test('UPD-1 to UPD-5: the instance, its update check, every office and how to update, with no restart control', async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/updates');
	await expect(page).toHaveTitle('Updates · Organization settings · Agentic Org');
	await expect(page.getByRole('heading', { level: 2, name: 'Updates' })).toBeVisible();
	await expect(
		page.getByRole('navigation', { name: 'Organization settings' }).getByRole('link', {
			name: 'Updates'
		})
	).toHaveAttribute('aria-current', 'page');

	const instance = page.getByTestId('instance');
	await expect(instance).toContainText('default');
	await expect(instance).toContainText('0.2.0');
	await expect(instance).toContainText('Update available');
	await expect(checkDetail(page)).toContainText(
		'Version 0.3.0 is available, downloaded and verified. Found 1 hour ago. Nothing changes until you upgrade.'
	);

	await expect(page.getByTestId('upgrade-command')).toHaveText(
		'agentic-org upgrade --name default'
	);
	await expect(page.getByRole('heading', { level: 3, name: 'How to update' })).toBeVisible();
	await expect(page.getByText('On This Mac, run:')).toBeVisible();
	await expect(page.getByTestId('upgrade-effects')).toContainText('You stay signed in.');

	await expect(offices(page)).toHaveCount(3);
	await expect(offices(page).nth(0)).toContainText('HQ');
	await expect(offices(page).nth(0)).toContainText('This Mac · Version 0.2.0');
	await expect(offices(page).nth(1)).toContainText('Build box');
	await expect(offices(page).nth(1)).toContainText('Version 0.1.0');
	await expect(offices(page).nth(1)).toContainText('Needs update');
	await expect(offices(page).nth(1)).toContainText(
		'To update it, run agentic-org upgrade on build-box.local.'
	);
	await expect(offices(page).nth(2)).toContainText('Studio Mac');
	await expect(offices(page).nth(2)).toContainText('Same as HQ');
	await expect(page.getByTestId('hold-back-note')).toHaveText(
		'Build box runs an older version of Agentic Org, so new features wait until it updates.'
	);

	// §0.1: no restart-to-update, install or check control before a supported update flow exists.
	await expect(page.locator('main section').getByRole('button')).toHaveText(['Copy command']);
	await expect(page.getByRole('button', { name: /restart|install|update now|check/i })).toHaveCount(
		0
	);
});

test('UPD-4: Copy command copies the exact command and says so', async ({ page, context }) => {
	await context.grantPermissions(['clipboard-read', 'clipboard-write']);
	await openOrganization(page);
	await visit(page, '/settings/updates');
	await page.getByRole('button', { name: 'Copy command' }).click();
	await expect(page.getByRole('button', { name: 'Copied' })).toBeVisible();
	expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
		'agentic-org upgrade --name default'
	);
});

test('UPD-2: each update check state is said in words', async ({ page }) => {
	await openOrganization(page);
	await visit(page, '/settings/updates?mock=current');
	await expect(page.getByTestId('instance')).toContainText('Up to date');
	await expect(checkDetail(page)).toHaveText('No newer release was found. Checked 12 minutes ago.');

	await visit(page, '/settings/updates?mock=failed');
	await expect(page.getByTestId('instance')).toContainText('Check failed');
	await expect(checkDetail(page)).toContainText(
		"HQ couldn't reach the release server. This instance keeps running its current release."
	);

	await visit(page, '/settings/updates?mock=offline');
	await expect(checkDetail(page)).toHaveText(
		"This instance runs offline, so it doesn't check for newer releases."
	);

	await visit(page, '/settings/updates?mock=empty');
	await expect(page.getByTestId('instance')).toContainText('Not reported');
	await expect(offices(page)).toHaveCount(1);
	await expect(page.getByTestId('hold-back-note')).toHaveCount(0);
});

test('UPD-6: loading, a failed read with Retry, and unavailable', async ({ page }) => {
	await openOrganization(page);
	await visit(page, '/settings/updates?mock=slow');
	const loading = page.getByRole('status').filter({ hasText: /loading updates/i });
	await expect(loading).toHaveText(/^Loading updates/);
	await expect(loading).toContainText('Still loading updates');
	await expect(page.getByTestId('instance')).toBeVisible({ timeout: 10_000 });

	await visit(page, '/settings/updates?mock=fails');
	const alert = page
		.getByRole('alert')
		.filter({ hasText: "Couldn't load the release information" });
	await expect(alert).toContainText("HQ didn't answer. Check that it's running, then try again.");
	await expect(alert.getByRole('heading', { level: 2 })).toBeVisible();
	// Retry while the read still fails: the busy label, then the same error with focus kept on Retry.
	await alert.getByRole('button', { name: 'Retry' }).click();
	await expect(alert.getByRole('button', { name: 'Retrying…' })).toHaveAttribute(
		'aria-busy',
		'true'
	);
	await expect(alert.getByRole('button', { name: 'Retry' })).toBeEnabled();
	await expect(alert.getByRole('button', { name: 'Retry' })).toBeFocused();

	// A Retry that succeeds focuses the page heading, never the body (§6a).
	await page.evaluate(() => history.replaceState(null, '', '/settings/updates'));
	await alert.getByRole('button', { name: 'Retry' }).click();
	await expect(page.getByTestId('instance')).toBeVisible();
	await expect(page.locator('#updates-heading')).toBeFocused();

	await visit(page, '/settings/updates?mock=unavailable');
	await expect(page.getByText("Updates isn't built yet")).toBeVisible();
	await expect(page.getByText("It's coming in this release.")).toBeVisible();
	await expect(page.getByTestId('instance')).toHaveCount(0);
});

for (const width of [1440, 1024, 640, 390, 320]) {
	test(`UPD-7: no sideways scroll and whole pill words at ${width} px`, async ({ page }) => {
		await page.setViewportSize({ width, height: 900 });
		await openOrganization(page);
		await visit(page, '/settings/updates');
		await expect(offices(page)).toHaveCount(3);
		await noSidewaysScroll(page);
		const pill = offices(page).nth(1).getByText('Needs update', { exact: true });
		// Whole words: the pill's text is not clipped.
		expect(await pill.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
		if (width < 640) {
			const box = await page.getByRole('button', { name: 'Copy command' }).boundingBox();
			expect(box!.height).toBeGreaterThanOrEqual(44);
		}
	});
}

test('UPD-7: at 320 px with 200% text nothing scrolls sideways', async ({ page }) => {
	await page.setViewportSize({ width: 320, height: 800 });
	await openOrganization(page);
	await visit(page, '/settings/updates');
	await page.evaluate(() => (document.documentElement.style.fontSize = '200%'));
	await expect(offices(page)).toHaveCount(3);
	await noSidewaysScroll(page);
	const pill = offices(page).nth(1).getByText('Needs update', { exact: true });
	expect(await pill.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
});
