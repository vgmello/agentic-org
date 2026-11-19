import { expect, test, type Locator, type Page } from '@playwright/test';
import { visit } from './ready';
import { signInAsAdmin } from './web-session';
import { enterMockedOrganization } from './mock-organization';

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
	await enterMockedOrganization(page, 'Lumen Labs');
}

const offices = (page: Page) => page.getByTestId('office-versions').locator('> li');
const checkDetail = (page: Page) => page.getByTestId('check-detail');

/** How many lines a pill's text takes, and which words were broken across lines (§6a: whole words only). */
async function pillWords(pill: Locator) {
	return pill.evaluate((el) => {
		const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
		let node: Text | null = null;
		while (walker.nextNode()) {
			const t = walker.currentNode as Text;
			if (t.data.trim()) node = t;
		}
		if (!node) return { lines: 0, broken: ['(no text)'] };
		const tops = (range: Range) =>
			new Set([...range.getClientRects()].filter((r) => r.width > 0).map((r) => Math.round(r.top)));
		const all = document.createRange();
		all.selectNodeContents(node);
		const broken: string[] = [];
		for (const m of node.data.matchAll(/\S+/g)) {
			const word = document.createRange();
			word.setStart(node, m.index!);
			word.setEnd(node, m.index! + m[0].length);
			if (tops(word).size > 1) broken.push(m[0]);
		}
		return { lines: tops(all).size, broken };
	});
}

/** Tab forward until `target` has focus: the keyboard path, not a pointer click. */
async function tabTo(page: Page, target: Locator) {
	for (let i = 0; i < 60; i++) {
		await page.keyboard.press('Tab');
		if (await target.evaluate((e) => e === document.activeElement)) return;
	}
	throw new Error('Tab never reached the control');
}

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
	await expect(page.getByText('On the HQ machine (This Mac), run:')).toBeVisible();
	await expect(page.getByTestId('upgrade-effects')).toContainText('You stay signed in.');

	await expect(offices(page)).toHaveCount(4);
	await expect(offices(page).nth(0)).toContainText('HQ');
	await expect(offices(page).nth(0)).toContainText('This Mac · Version 0.2.0');
	await expect(offices(page).nth(1)).toContainText('Build box');
	await expect(offices(page).nth(1)).toContainText('Version 0.1.0');
	await expect(offices(page).nth(1)).toContainText('Needs update');
	// P2-2: the office's own instance, so the command can't upgrade a peer instance on that machine (SRV-16).
	await expect(offices(page).nth(1).getByTestId('office-hint')).toHaveText(
		'To update it, run agentic-org upgrade --name build on build-box.local.'
	);
	// Lab mini (the Offices mock's third satellite) is older but does not hold features back.
	await expect(offices(page).nth(2)).toContainText('Lab mini');
	await expect(offices(page).nth(2)).toContainText('Version 0.0.9');
	await expect(offices(page).nth(3)).toContainText('Studio Mac');
	await expect(offices(page).nth(3)).toContainText('Same as HQ');
	const note = page.getByTestId('hold-back-note');
	await expect(note).toHaveText(
		'Build box runs an older version of Agentic Org, so new features wait until it updates.'
	);
	// offices-and-teams-ux §6.1: the note links to the row.
	await note.getByRole('link', { name: 'Build box' }).click();
	await expect(page).toHaveURL(/#office-version-/);
	await expect(offices(page).nth(1)).toBeFocused();

	// §0.1: no restart-to-update, install or check control before a supported update flow exists.
	await expect(page.locator('main section').getByRole('button')).toHaveText(['Copy command']);
	await expect(page.getByRole('button', { name: /restart|install|update now|check/i })).toHaveCount(
		0
	);
});

test("UPD-3: when HQ doesn't know an office's instance, the hint asks for it rather than defaulting", async ({
	page
}) => {
	await openOrganization(page);
	await visit(page, '/settings/updates?mock=no-instance');
	const hint = offices(page).nth(1).getByTestId('office-hint');
	await expect(hint).toContainText('agentic-org upgrade --name <instance>');
	await expect(hint).toContainText(
		"with the name of the instance this office runs in. HQ doesn't know it"
	);
	await expect(hint.locator('code').first()).toHaveText('agentic-org upgrade --name <instance>');
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
	// Retry from the keyboard while the read still fails: the busy label, then the same error with focus back on
	// Retry, never on the body.
	const retry = alert.getByRole('button', { name: 'Retry' });
	await tabTo(page, retry);
	await page.keyboard.press('Enter');
	await expect(alert.getByRole('button', { name: 'Retrying…' })).toHaveAttribute(
		'aria-busy',
		'true'
	);
	await expect(retry).toBeEnabled();
	await expect(retry).toBeFocused();

	// P2-1: a keyboard Retry that succeeds focuses the page heading, which shows no control ring (§6a).
	await visit(page, '/settings/updates?mock=fails-once');
	await tabTo(page, retry);
	await page.keyboard.press('Enter');
	await expect(page.getByTestId('instance')).toBeVisible();
	const heading = page.locator('#updates-heading');
	await expect(heading).toBeFocused();
	expect(await heading.evaluate((e) => e.matches(':focus-visible'))).toBe(true);
	expect(await heading.evaluate((e) => getComputedStyle(e).outlineStyle)).toBe('none');

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
		await expect(offices(page)).toHaveCount(4);
		await noSidewaysScroll(page);
		const pill = offices(page).nth(1).getByTestId('version-pill');
		await expect(pill).toHaveText('Needs update');
		// Room for it: one line, no word broken.
		expect(await pillWords(pill)).toEqual({ lines: 1, broken: [] });
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
	await expect(offices(page)).toHaveCount(4);
	await noSidewaysScroll(page);
	// Wherever the pills wrap, they wrap between words: no word is split across lines.
	for (const pill of await page.getByTestId('version-pill').all())
		expect((await pillWords(pill)).broken).toEqual([]);
});
