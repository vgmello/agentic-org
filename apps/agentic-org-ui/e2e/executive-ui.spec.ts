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
	await expect(page.locator('html')).not.toHaveAttribute('data-interior-page');
	await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
	await expect(page.getByRole('region', { name: /Executive overview/ })).toBeVisible();
	await expect(page.locator('#hb-pil-fin')).toHaveText('Financials');
	await expect(page.locator('#hb-pil-team')).toHaveText('Team');
	await expect(page.locator('#hb-pil-proj')).toHaveText('Delivery');
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

	// Executive Brief Show full brief / Show less disclosure at stacked viewport (<1280px)
	await page.setViewportSize({ width: 1024, height: 900 });
	const expandBtn = page.getByRole('button', { name: 'Show full brief' });
	await expect(expandBtn).toBeVisible();
	await expect(expandBtn).toHaveAttribute('aria-expanded', 'false');
	await expandBtn.click();
	const collapseBtn = page.getByRole('button', { name: 'Show less' });
	await expect(collapseBtn).toBeVisible();
	await expect(collapseBtn).toHaveAttribute('aria-expanded', 'true');
	await collapseBtn.click();
	await expect(page.getByRole('button', { name: 'Show full brief' })).toHaveAttribute(
		'aria-expanded',
		'false'
	);
	await page.setViewportSize({ width: 1440, height: 900 });

	// Inline Home search combobox: Cmd/Ctrl+K focuses input, live filters, Escape preserves query & blocks hidden Enter, Tab closes
	const homeCombo = page.getByRole('combobox', { name: 'Search, run a command, or ask Org AI' });
	await expect(homeCombo).toBeVisible();
	await page.keyboard.press('ControlOrMeta+k');
	await expect(homeCombo).toBeFocused();
	await homeCombo.fill('Theme: Classic');
	const inlineListbox = page.locator('#exec-cmdk-home-listbox');
	await expect(inlineListbox).toBeVisible();
	await expect(homeCombo).toHaveAttribute('aria-controls', 'exec-cmdk-home-listbox');
	await expect(inlineListbox.getByRole('option', { name: /^Theme: Classic/ })).toBeVisible();
	await expect(
		inlineListbox.getByRole('option', { name: /Ask Org AI about 'Theme: Classic'/ })
	).toBeVisible();

	// First Escape closes popup without clearing query; pressing Enter while closed does NOT execute hidden option
	await homeCombo.press('Escape');
	await expect(inlineListbox).toBeHidden();
	await expect(homeCombo).not.toHaveAttribute('aria-controls');
	await expect(homeCombo).toHaveValue('Theme: Classic');
	await homeCombo.press('Enter');
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'ocean');

	// Re-open with ArrowDown selects index 0 (not index 1), and Tab closes cleanly
	await homeCombo.press('ArrowDown');
	await expect(inlineListbox).toBeVisible();
	await expect(homeCombo).toHaveAttribute('aria-activedescendant', 'exec-cmdk-home-opt-0');
	await homeCombo.press('Tab');
	await expect(inlineListbox).toBeHidden();

	// Escape -> ArrowDown -> Enter selects index 0 (Theme: Classic), not index 1
	await homeCombo.focus();
	await expect(inlineListbox).toBeVisible();
	await homeCombo.press('Escape');
	await expect(inlineListbox).toBeHidden();
	await homeCombo.press('ArrowDown');
	await expect(inlineListbox).toBeVisible();
	await expect(homeCombo).toHaveAttribute('aria-activedescendant', 'exec-cmdk-home-opt-0');
	await homeCombo.press('Enter');
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'classic');
	await expect(page).toHaveURL(/\/home$/);
	await expect(inlineListbox).toBeHidden();
	await page
		.getByRole('group', { name: 'Theme' })
		.getByRole('button', { name: 'Ocean', exact: true })
		.click();
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'ocean');

	// Capture Ocean Home canvas backgroundImage (3-orb radial gradients) before visiting interior pages
	const oceanHomeCanvasBg = await page.locator('.exec-canvas').evaluate((el) => {
		return getComputedStyle(el, '::before').backgroundImage;
	});
	expect(oceanHomeCanvasBg).toContain('radial-gradient');

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
	await expect(page.locator('.exec-cta-glass').first()).toHaveCSS(
		'background-color',
		'rgb(83, 83, 83)'
	);
	await expect(page.locator('.exec-cta-glass').first()).toHaveCSS('border-radius', '5px');
	if (captureDir) {
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
	await expect(page.locator('html')).toHaveAttribute('data-interior-page');
	const oceanInteriorCanvasBg = await page.locator('.exec-canvas').evaluate((el) => {
		return getComputedStyle(el, '::before').backgroundImage;
	});
	expect(oceanInteriorCanvasBg).not.toBe(oceanHomeCanvasBg);
	expect(oceanInteriorCanvasBg).toContain('linear-gradient');
	expect(oceanInteriorCanvasBg).not.toContain('radial-gradient');
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

	// Settings shows 6-theme Appearance selector + WAI-ARIA radio-group keyboard + Cozy High Contrast
	await page
		.getByRole('navigation', { name: 'Main' })
		.getByRole('link', { name: 'Settings' })
		.click();
	await expect(page).toHaveURL(/\/settings$/);
	await expect(page.getByTestId('executive-ui-toggle')).toHaveCount(0);
	const oceanRadio = page.getByTestId('theme-option-ocean');
	const classicRadio = page.getByTestId('theme-option-classic');
	const graphiteRadio = page.getByTestId('theme-option-graphite');
	const cozyRadio = page.getByTestId('theme-option-cozy');
	await expect(graphiteRadio).toBeVisible();
	await expect(cozyRadio).toBeVisible();
	await expect(page.getByTestId('theme-option-report')).toBeVisible();

	// WAI-ARIA radio group roving tabindex + ArrowRight / ArrowLeft keyboard switching
	await expect(oceanRadio).toHaveAttribute('tabindex', '0');
	await expect(classicRadio).toHaveAttribute('tabindex', '-1');
	await oceanRadio.focus();
	await page.keyboard.press('ArrowRight');
	await expect(classicRadio).toBeFocused();
	await expect(classicRadio).toHaveAttribute('tabindex', '0');
	await expect(oceanRadio).toHaveAttribute('tabindex', '-1');
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'classic');
	await page.keyboard.press('ArrowLeft');
	await expect(oceanRadio).toBeFocused();
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'ocean');

	await cozyRadio.click();
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'cozy');
	const cozyContrastToggle = page.getByTestId('cozy-high-contrast-toggle');
	await expect(cozyContrastToggle).toHaveAttribute('aria-checked', 'false');
	const beforeInk = await page
		.locator('html')
		.evaluate((el) => getComputedStyle(el).getPropertyValue('--exec-ink').trim());
	const beforeSurface = await page
		.locator('html')
		.evaluate((el) => getComputedStyle(el).getPropertyValue('--exec-surface').trim());

	const measureContrastCardHelp = () =>
		cozyContrastToggle.evaluate((el) => {
			const card = el.closest('.contrast-card');
			const help = document.getElementById('cozy-contrast-help');
			if (!card || !help) throw new Error('Expected .contrast-card and #cozy-contrast-help');
			const parseRgba = (s: string) => {
				const parts = (s.match(/[\d.]+/g) ?? []).map(Number);
				return [parts[0] ?? 0, parts[1] ?? 0, parts[2] ?? 0, parts[3] ?? 1] as const;
			};
			const composite = (
				fg: readonly [number, number, number, number],
				bg: readonly [number, number, number]
			) => {
				const a = fg[3];
				return [
					fg[0] * a + bg[0] * (1 - a),
					fg[1] * a + bg[1] * (1 - a),
					fg[2] * a + bg[2] * (1 - a)
				] as const;
			};
			const lum = (rgb: readonly [number, number, number]) => {
				const c = rgb.map((v) => {
					const n = v / 255;
					return n <= 0.03928 ? n / 12.92 : Math.pow((n + 0.055) / 1.055, 2.4);
				});
				return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
			};
			// Worst-case Cozy peak overhead shell backdrop (#493d32 = rgb(73, 61, 50))
			const shellPeak = [73, 61, 50] as const;
			const bgRgba = parseRgba(getComputedStyle(card).backgroundColor);
			const bgRgb = composite(bgRgba, shellPeak);
			const fgRgba = parseRgba(getComputedStyle(help).color);
			const fgRgb = composite(fgRgba, bgRgb);
			const l1 = lum(fgRgb);
			const l2 = lum(bgRgb);
			return {
				alpha: bgRgba[3],
				ratio: (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05)
			};
		});

	const beforeCard = await measureContrastCardHelp();

	await cozyContrastToggle.click();
	await expect(cozyContrastToggle).toHaveAttribute('aria-checked', 'true');
	await expect(page.locator('html')).toHaveAttribute('data-cozy-contrast', 'high');
	const afterCard = await measureContrastCardHelp();
	expect(afterCard.alpha).toBeGreaterThan(beforeCard.alpha);
	expect(afterCard.ratio).toBeGreaterThan(beforeCard.ratio);
	expect(afterCard.ratio).toBeGreaterThanOrEqual(8);
	const afterInk = await page
		.locator('html')
		.evaluate((el) => getComputedStyle(el).getPropertyValue('--exec-ink').trim());
	const afterSurface = await page
		.locator('html')
		.evaluate((el) => getComputedStyle(el).getPropertyValue('--exec-surface').trim());
	expect(afterInk).not.toBe(beforeInk);
	expect(afterSurface).not.toBe(beforeSurface);

	// Reload persists Cozy Glass High Contrast preference
	await page.reload();
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'cozy');
	await expect(page.locator('html')).toHaveAttribute('data-cozy-contrast', 'high');
	await expect(page.getByTestId('cozy-high-contrast-toggle')).toHaveAttribute(
		'aria-checked',
		'true'
	);

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

test('Home scroll cue: keyboard-reachable before below-fold content, moves focus on activation, and resets on return to /home', async ({
	page
}) => {
	await page.setViewportSize({ width: 1440, height: 700 });
	await visit(page, '/home?theme=graphite&mock=poc');

	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'graphite');
	await expect(page.getByRole('heading', { name: 'Executive Brief' })).toBeVisible();

	const hint = page.getByTestId('home-scroll-hint');
	await expect(hint).toBeVisible();
	await expect(hint).toContainText('Scroll for more');

	// Verify 640-720px fine-pointer targets meet 44px minimum (sidebar theme pill, Ask pill, Home cue)
	await page.setViewportSize({ width: 680, height: 700 });
	await expect(hint).toBeVisible();
	const pillBox = await page
		.getByRole('group', { name: 'Theme' })
		.getByRole('button', { name: 'Graphite' })
		.boundingBox();
	const askBox = await page.getByRole('button', { name: 'Ask', exact: true }).boundingBox();
	const hintBox = await hint.boundingBox();
	expect(pillBox?.height ?? 0).toBeGreaterThanOrEqual(44);
	expect(askBox?.height ?? 0).toBeGreaterThanOrEqual(44);
	expect(hintBox?.height ?? 0).toBeGreaterThanOrEqual(44);
	await page.setViewportSize({ width: 1440, height: 700 });
	await expect(hint).toBeVisible();

	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/poc-home-scroll-cue-graphite.png` });
	}

	// Keyboard Tab from Ask button in command bar reaches Home scroll cue before below-fold content
	const askButton = page.getByRole('button', { name: 'Ask', exact: true });
	await askButton.focus();
	await page.keyboard.press('Tab');
	await expect(hint).toBeFocused();

	// Activating via Enter scrolls scorecard into view, moves focus off body, and hides cue
	await page.keyboard.press('Enter');
	await expect.poll(() => page.locator('main').evaluate((el) => el.scrollTop)).toBeGreaterThan(16);
	await expect(hint).toBeHidden();
	await expect(page.locator('#exec-projects-heading')).toBeInViewport();
	const activeId = await page.evaluate(() => ({
		tag: document.activeElement?.tagName,
		id: document.activeElement?.id
	}));
	expect(activeId.tag).not.toBe('BODY');
	expect(activeId.id).toBe('exec-projects-heading');

	// Navigating away while scrolled down and returning via popstate (Back) OR link resets main.scrollTop to 0 and shows cue again
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Work' }).click();
	await expect(page).toHaveURL(/\/work/);
	await expect(page.getByRole('heading', { name: 'Work' })).toBeVisible();
	// Use a shorter viewport and wait for Work content to overflow <main> so popstate Back genuinely resets a non-zero offset
	await page.setViewportSize({ width: 1440, height: 480 });
	await expect
		.poll(() => page.locator('main').evaluate((el) => el.scrollHeight - el.clientHeight))
		.toBeGreaterThan(30);
	await page.locator('main').evaluate((el) => {
		el.scrollTop = 48;
	});
	await expect.poll(() => page.locator('main').evaluate((el) => el.scrollTop)).toBeGreaterThan(16);
	await page.goBack();
	await expect(page).toHaveURL(/\/home/);
	await expect(page.getByRole('heading', { name: 'Executive Brief' })).toBeVisible();
	await expect.poll(() => page.locator('main').evaluate((el) => el.scrollTop)).toBe(0);
	await expect(hint).toBeVisible();
	await page.setViewportSize({ width: 1440, height: 700 });

	await hint.click();
	await expect.poll(() => page.locator('main').evaluate((el) => el.scrollTop)).toBeGreaterThan(16);
	await expect(hint).toBeHidden();
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: /Inbox/ }).click();
	await expect(page).toHaveURL(/\/inbox/);
	await expect(page.getByRole('heading', { level: 1, name: 'Inbox' })).toBeVisible();
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Home' }).click();
	await expect(page).toHaveURL(/\/home$/);
	await expect(page.getByRole('heading', { name: 'Executive Brief' })).toBeVisible();
	await expect.poll(() => page.locator('main').evaluate((el) => el.scrollTop)).toBe(0);
	await expect(hint).toBeVisible();
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

	// Upcoming milestones · Next 14 days renders on /home?mock=poc with milestone items
	await expect(
		page.getByRole('heading', { name: 'Upcoming milestones · Next 14 days' })
	).toBeVisible();
	await expect(page.locator('.hor-item').first()).toBeVisible();

	// Home inline search finds real POC work items and navigates to a real working project route (never NotBuilt)
	const pocCombo = page.getByRole('combobox', { name: 'Search, run a command, or ask Org AI' });
	await pocCombo.click();
	await pocCombo.fill('Custody lock');
	const custodyOption = page
		.locator('#exec-cmdk-home-listbox')
		.getByRole('option', { name: /^Custody lock fix/ });
	await expect(custodyOption).toBeVisible();
	await pocCombo.press('Escape');
	await pocCombo.press('ArrowDown');
	await expect(pocCombo).toHaveAttribute('aria-activedescendant', 'exec-cmdk-home-opt-0');
	await pocCombo.press('Enter');
	await expect(page).toHaveURL(/\/projects\/office-federation\/work$/);
	await expect(page.getByText('This area is not built yet')).toHaveCount(0);
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Home' }).click();
	await expect(page).toHaveURL(/\/home$/);

	// Decision Spotlight "Ask about this" opens Org AI with that decision's answer
	await page.getByRole('button', { name: 'Ask about this' }).click();
	const aiDialog = page.getByRole('dialog', { name: 'Command palette and Org AI' });
	await expect(aiDialog).toBeVisible();
	await expect(aiDialog).toContainText("Approve publishing Kai's sync update to v1.0");
	await expect(aiDialog).toContainText('Ada recommends: approve.');
	await aiDialog.getByRole('button', { name: 'ESC' }).click();
	await expect(aiDialog).toBeHidden();

	// Clicking "New task" on Home opens the task creation dialog on /work and strips ?new=1
	await page.getByRole('link', { name: 'New task' }).click();
	await expect(page).toHaveURL(/\/work$/);
	const newTaskDialog = page.getByRole('dialog', { name: 'New task' });
	await expect(newTaskDialog).toBeVisible();
	await newTaskDialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(newTaskDialog).toBeHidden();
	await page.reload();
	await page.locator('html[data-hydrated="true"]').waitFor();
	await expect(page.getByRole('heading', { name: 'Work' })).toBeVisible();
	await expect(page.getByRole('dialog', { name: 'New task' })).toHaveCount(0);
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Home' }).click();
	await expect(page).toHaveURL(/\/home$/);

	// Project card spot-jump jumps directly to the matching Spotlight decision
	const spotJumpBtn = page.locator('button[data-spot-jump="in2"]').first();
	await expect(spotJumpBtn).toBeVisible();
	await spotJumpBtn.click();
	await expect(page.locator('#hb-spotlight')).toContainText(
		'Choose how offices coordinate shared files: timed access or fixed lock'
	);

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

test('Phase 2 Inbox parity: 3 primary tabs, More/Filter/Display menus, Graphite surfaces, member card Escape, comments persistence, and dark CmdK contrast', async ({
	page
}) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await visit(page, '/inbox?theme=graphite&mock=poc');

	// Graphite Inbox matte surfaces & interior sidebar text hierarchy (13abb1f1, 1f5f7149..e75d920a)
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'graphite');
	await expect(page.locator('html')).toHaveAttribute('data-interior-page');
	await expect(page.locator('aside.exec-sidebar')).toHaveCSS('background-color', 'rgb(19, 21, 22)');
	await expect(page.locator('.triage-queue')).toHaveCSS('background-color', 'rgb(23, 24, 25)');
	await expect(page.locator('.exec-nav-label').first()).toHaveCSS('color', 'rgb(133, 136, 142)');

	// 3 primary categories only (All, Approvals, Decisions) + More trigger keeps "More" label
	const catGroup = page.getByRole('group', { name: 'Inbox categories' });
	const primaryButtons = catGroup.locator(':scope > button.queue-tab');
	await expect(primaryButtons).toHaveCount(3);
	await expect(primaryButtons.nth(0)).toContainText('All');
	await expect(primaryButtons.nth(1)).toContainText('Approvals');
	await expect(primaryButtons.nth(2)).toContainText('Decisions');

	const moreTrigger = catGroup.getByRole('button', { name: 'More' });
	await moreTrigger.click();
	await expect(page.getByRole('menuitemradio', { name: /^Questions\s+\d+$/ })).toBeVisible();
	await expect(page.getByRole('menuitemradio', { name: /^Problems\s+\d+$/ })).toBeVisible();
	await expect(page.getByRole('menuitemradio', { name: /^Work\s+\d+$/ })).toBeVisible();
	const handledMenuItem = page.getByRole('menuitemradio', { name: /^Handled\s+[1-9]\d*$/ });
	await expect(handledMenuItem).toBeVisible();
	await handledMenuItem.click();

	await expect(moreTrigger).toHaveText('More');
	await expect(moreTrigger).toHaveAttribute('data-current-kind', '');
	await expect(moreTrigger).toHaveAttribute('title', 'Viewing Handled');
	await expect(page.locator('.triage-queue-summary')).toContainText('Handled items');

	// Switch back to All
	await primaryButtons.nth(0).click();
	await expect(page.locator('.triage-queue-summary')).toContainText('Priority first');

	// Filter inbox menu: Project scope URL round-trip + Urgent only + Reset filters
	const filterBtn = page.getByRole('button', { name: 'Filter inbox' });
	await filterBtn.click();
	await page.getByRole('menuitemradio', { name: 'Office federation' }).click();
	await expect(page).toHaveURL(/[?&]project=office-federation(?:&|$)/);
	await expect(filterBtn).toHaveAttribute('data-active', '');
	await page.reload();
	await page.locator('html[data-hydrated="true"]').waitFor();
	await expect(page).toHaveURL(/[?&]project=office-federation(?:&|$)/);
	await expect(page.getByRole('button', { name: 'Filter inbox' })).toHaveAttribute(
		'data-active',
		''
	);

	await page.getByRole('button', { name: 'Filter inbox' }).click();
	await page.getByRole('menuitem', { name: 'Reset filters' }).click();
	await expect(page).not.toHaveURL(/[?&]project=/);
	await expect(page.getByRole('button', { name: 'Filter inbox' })).not.toHaveAttribute(
		'data-active'
	);

	await page.getByRole('button', { name: 'Filter inbox' }).click();
	await page.getByRole('menuitemradio', { name: 'Urgent only' }).click();
	await expect(page.getByRole('button', { name: 'Filter inbox' })).toHaveAttribute(
		'data-active',
		''
	);
	await page.getByRole('button', { name: 'Filter inbox' }).click();
	await page.getByRole('menuitem', { name: 'Reset filters' }).click();
	await expect(page.getByRole('button', { name: 'Filter inbox' })).not.toHaveAttribute(
		'data-active'
	);

	// Inbox display menu: By date grouping + Row dividers checkbox persisted across reload
	const displayBtn = page.getByRole('button', { name: 'Inbox display' });
	await displayBtn.click();
	await page.getByRole('menuitemradio', { name: 'By date' }).click();
	await expect(page.locator('.triage-queue-summary')).toContainText('By date');
	await expect(displayBtn).toHaveAttribute('data-active', '');

	await displayBtn.click();
	await page.getByRole('menuitemcheckbox', { name: 'Row dividers' }).click();
	await expect(page.locator('.triage-workspace')).toHaveAttribute('data-inbox-dividers', '');
	await page.keyboard.press('Escape');

	// Select a specific item so ?item= persists across reload
	await page.locator('[data-triage-row="in1"]').click();
	await expect(page).toHaveURL(/[?&]item=in1(?:&|$)/);

	// Member hover/focus preview card: continuous hover bridge + Escape dismissal (WCAG 1.4.13)
	const memberLink = page.locator('.triage-from-link').first();
	const memberWrap = page.locator('.triage-from-member').first();
	const memberCard = memberWrap.locator('.triage-from-card');
	await memberLink.hover();
	await expect(memberCard).toBeVisible();
	const linkBox = (await memberLink.boundingBox())!;
	const cardBox = (await memberCard.boundingBox())!;
	// Move pointer slowly pixel-by-pixel across the 4px padding bridge from trigger to card without losing hover
	await page.mouse.move(linkBox.x + linkBox.width / 2, linkBox.y + linkBox.height - 1);
	await page.mouse.move(linkBox.x + linkBox.width / 2, linkBox.y + linkBox.height + 2, {
		steps: 4
	});
	await expect(memberCard).toBeVisible();
	await page.mouse.move(cardBox.x + cardBox.width / 2, cardBox.y + cardBox.height / 2, {
		steps: 8
	});
	await expect(memberCard).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(memberCard).toBeHidden();
	// Re-hover trigger and press Escape while still on the trigger: preview stays dismissed until pointer leaves
	await memberLink.hover();
	await expect(memberCard).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(memberWrap).toHaveAttribute('data-preview-dismissed', '');
	await expect(memberCard).toBeHidden();
	await page.mouse.move(10, 10);

	// Exact 720px viewport hit targets (>= 44px inclusive width <= 720px)
	await page.setViewportSize({ width: 720, height: 900 });
	await expect(page.locator('.triage-copy').first()).toBeVisible();
	expect(
		(await page.locator('.triage-copy').first().boundingBox())?.height ?? 0
	).toBeGreaterThanOrEqual(44);
	expect(
		(await page.locator('.triage-from-link').first().boundingBox())?.height ?? 0
	).toBeGreaterThanOrEqual(44);
	await page.getByRole('button', { name: 'Back to Inbox' }).click();
	await page.getByRole('button', { name: 'Filter inbox' }).click();
	const urgentMenuItem = page.getByRole('menuitemradio', { name: 'Urgent only' });
	await expect(urgentMenuItem).toBeVisible();
	await expect
		.poll(async () => (await urgentMenuItem.boundingBox())?.height ?? 0)
		.toBeGreaterThanOrEqual(44);
	await page.keyboard.press('Escape');
	await page.setViewportSize({ width: 1440, height: 900 });
	await page.locator('[data-triage-row="in1"]').click();

	// Comment validity reset, per-item unsent draft preservation, live status announcement, and reload persistence
	const commentBox = page.getByLabel('Comment on this request');
	const sendCommentBtn = page.getByRole('button', { name: 'Send comment' });
	const otherRow = page.locator('[data-triage-row]:not([data-triage-row="in1"])').first();

	// Save a valid unsent draft on another item first
	await otherRow.click();
	await commentBox.fill('Unsent draft for second item');

	// Trigger whitespace custom validity on in1, then switch back to otherRow and verify validity cleared
	await page.locator('[data-triage-row="in1"]').click();
	await commentBox.fill('   ');
	await sendCommentBtn.click();
	expect(await commentBox.evaluate((el: HTMLTextAreaElement) => el.validationMessage)).toContain(
		'Enter a comment'
	);

	await otherRow.click();
	await expect(commentBox).toHaveValue('Unsent draft for second item');
	await expect
		.poll(() =>
			commentBox.evaluate((el: HTMLTextAreaElement) => ({
				customError: el.validity.customError,
				validationMessage: el.validationMessage
			}))
		)
		.toEqual({ customError: false, validationMessage: '' });

	// Switch back to in1: whitespace draft preserved, then post a real comment & verify live status
	await page.locator('[data-triage-row="in1"]').click();
	await expect(commentBox).toHaveValue('   ');

	await commentBox.fill('Verified release notes and rollback plan.');
	await sendCommentBtn.click();
	await expect(page.locator('#triage-comment-status')).toHaveText('Comment posted (1 total).');
	await expect(page.locator('.triage-comment-list')).toContainText(
		'Verified release notes and rollback plan.'
	);

	await page.reload();
	await page.locator('html[data-hydrated="true"]').waitFor();
	await expect(page.locator('.triage-workspace')).toHaveAttribute('data-inbox-dividers', '');
	await expect(page.locator('.triage-comment-list')).toContainText(
		'Verified release notes and rollback plan.'
	);

	// Dark Command Dialog contrast in Classic & Report Night (ffe0e0ce) with computed WCAG checks
	for (const [themeLabel, themeId] of [
		['Classic', 'classic'],
		['Night', 'report-dark']
	] as const) {
		await page
			.getByRole('group', { name: 'Theme' })
			.getByRole('button', { name: themeLabel, exact: true })
			.click();
		await expect(page.locator('html')).toHaveAttribute('data-ui-theme', themeId);
		await page.getByRole('button', { name: 'Ask', exact: true }).click();
		const dialog = page.locator('.exec-cmdk-dialog');
		await expect(dialog).toBeVisible();
		await expect(dialog).toHaveCSS('border-top-color', 'rgb(131, 147, 171)');
		await expect(dialog.locator('.exec-cmdk-tabs')).toHaveCSS(
			'background-color',
			'rgb(52, 68, 90)'
		);
		const contrast = await dialog.evaluate((el) => {
			const parseRgb = (s: string) =>
				(s.match(/[\d.]+/g) ?? []).slice(0, 3).map(Number) as [number, number, number];
			const lum = (rgb: [number, number, number]) => {
				const c = rgb.map((v) => {
					const n = v / 255;
					return n <= 0.03928 ? n / 12.92 : Math.pow((n + 0.055) / 1.055, 2.4);
				});
				return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
			};
			const ratio = (a: [number, number, number], b: [number, number, number]) => {
				const l1 = lum(a);
				const l2 = lum(b);
				return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
			};
			const activeTab = el.querySelector<HTMLElement>('.exec-cmdk-tab[aria-selected="true"]')!;
			const inactiveTab = el.querySelector<HTMLElement>('.exec-cmdk-tab[aria-selected="false"]')!;
			const tabGrp = el.querySelector<HTMLElement>('.exec-cmdk-tab-grp')!;
			const escBtn = el.querySelector<HTMLElement>('.exec-cmdk-close')!;
			const borderRgb = parseRgb(getComputedStyle(el).borderTopColor);
			const pageRgb = parseRgb(
				getComputedStyle(document.querySelector('.exec-canvas')!).backgroundColor
			);
			return {
				borderVsPage: ratio(borderRgb, pageRgb),
				activeTabText: ratio(
					parseRgb(getComputedStyle(activeTab).color),
					parseRgb(getComputedStyle(activeTab).backgroundColor)
				),
				inactiveTabText: ratio(
					parseRgb(getComputedStyle(inactiveTab).color),
					parseRgb(getComputedStyle(tabGrp).backgroundColor)
				),
				escText: ratio(
					parseRgb(getComputedStyle(escBtn).color),
					parseRgb(getComputedStyle(escBtn).backgroundColor)
				)
			};
		});
		expect(contrast.borderVsPage).toBeGreaterThanOrEqual(3.0);
		expect(contrast.activeTabText).toBeGreaterThanOrEqual(4.5);
		expect(contrast.inactiveTabText).toBeGreaterThanOrEqual(4.5);
		expect(contrast.escText).toBeGreaterThanOrEqual(4.5);
		await page.keyboard.press('Escape');
		await expect(dialog).toBeHidden();
	}
});

test('Phase 3 Work parity: unified queue header, live search & / shortcut, Filter/Display menus, List/Board toggle, Graphite matte canvas, and 720px hit targets', async ({
	page
}) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await visit(page, '/work?theme=graphite&mock=poc');

	// Graphite matte Work canvas (#18191a) + unified .work-header.queue-header geometry
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'graphite');
	await expect(page.locator('html')).toHaveAttribute('data-interior-page');
	await expect(page.locator('main')).toHaveCSS('background-color', 'rgb(24, 25, 26)');
	await expect(page.locator('aside.exec-sidebar')).toHaveCSS('background-color', 'rgb(19, 21, 22)');
	await expect(page.locator('header.work-header.queue-header')).toBeVisible();
	await expect(page.locator('.work-header .queue-headline')).toBeVisible();
	await expect(page.locator('.work-toolbar.queue-toolbar')).toBeVisible();
	await expect(page.locator('#shell-page-title')).toHaveText('Work');

	// Switch to All tasks so we have multiple projects & statuses to filter/group/board
	await page.locator('[data-filter="all"]').click();
	const workRows = page.getByTestId('work-row');
	const initialCount = await workRows.count();
	expect(initialCount).toBeGreaterThan(2);

	// `/` shortcut focuses Work search input; live search filters rows; Escape clears then blurs
	await page.locator('#shell-page-title').focus();
	await page.keyboard.press('/');
	const searchInput = page.getByLabel('Search work');
	await expect(searchInput).toBeFocused();
	await searchInput.fill('Custody');
	await expect(workRows).toHaveCount(1);
	await expect(workRows.first()).toContainText('Custody lock fix');

	const clearBtn = page.getByRole('button', { name: 'Clear search' });
	await expect(clearBtn).toBeVisible();
	await clearBtn.click();
	await expect(searchInput).toHaveValue('');
	await expect(workRows).toHaveCount(initialCount);

	await searchInput.fill('Custody');
	await expect(workRows).toHaveCount(1);
	await searchInput.press('Escape');
	await expect(searchInput).toHaveValue('');
	await expect(searchInput).toBeFocused();
	await searchInput.press('Escape');
	await expect(searchInput).not.toBeFocused();

	// Filter work menu: Project + Assignee scope selection narrows count strictly & preserves both params across reload
	const filterBtn = page.getByRole('button', { name: 'Filter work' });
	await filterBtn.click();
	await page.getByRole('menuitemradio', { name: 'Office federation' }).click();
	await expect(page).toHaveURL(/[?&]project=office-federation(?:&|$)/);
	const projectOnlyCount = await workRows.count();
	expect(projectOnlyCount).toBe(3);

	await filterBtn.click();
	await page.getByRole('menuitemradio', { name: 'Sage' }).click();
	await expect(page).toHaveURL(/[?&]project=office-federation(?:&|$)/);
	await expect(page).toHaveURL(/[?&]assignee=sage(?:&|$)/);
	await expect(filterBtn).toHaveAttribute('data-active', '');
	await expect(workRows).toHaveCount(1);
	await expect(workRows.first()).toContainText('Custody lock fix');

	await page.reload();
	await page.locator('html[data-hydrated="true"]').waitFor();
	await expect(page).toHaveURL(/[?&]project=office-federation(?:&|$)/);
	await expect(page).toHaveURL(/[?&]assignee=sage(?:&|$)/);
	// Switch to All after reload (Blocked preset alone only has 1 Office federation task) to prove assignee=sage still narrows 3 -> 1
	await page.locator('[data-filter="all"]').click();
	await expect(workRows).toHaveCount(1);
	await expect(workRows.first()).toContainText('Custody lock fix');
	await expect(page.getByRole('button', { name: 'Filter work' })).toHaveAttribute(
		'data-active',
		''
	);

	await page.getByRole('button', { name: 'Filter work' }).click();
	await page.getByRole('menuitem', { name: 'Reset filters' }).click();
	await expect(page).not.toHaveURL(/[?&]project=/);
	await expect(page).not.toHaveURL(/[?&]assignee=/);
	await expect(page.getByRole('button', { name: 'Filter work' })).not.toHaveAttribute(
		'data-active'
	);

	// Work display menu: By project grouping switches headings from statuses to projects & Row dividers persists across reload
	await page.locator('[data-filter="all"]').click();
	const displayBtn = page.getByRole('button', { name: 'Work display' });
	await displayBtn.click();
	await page.getByRole('menuitemradio', { name: 'By project' }).click();
	await expect(displayBtn).toHaveAttribute('data-active', '');
	const groupHeadings = page.locator('.work-group-title');
	await expect(groupHeadings.filter({ hasText: 'Agentic Org v1.0' })).toBeVisible();
	await expect(groupHeadings.filter({ hasText: 'Blocked' })).toHaveCount(0);
	await expect(groupHeadings.filter({ hasText: 'In progress' })).toHaveCount(0);

	await displayBtn.click();
	await page.getByRole('menuitemcheckbox', { name: 'Row dividers' }).click();
	await expect(page.locator('.work-workspace')).toHaveAttribute('data-work-dividers', '');
	await page.keyboard.press('Escape');

	// Reload verifies Row dividers persistence
	await page.reload();
	await page.locator('html[data-hydrated="true"]').waitFor();
	await expect(page.locator('.work-workspace')).toHaveAttribute('data-work-dividers', '');

	// Requests tab: Work header search query is cleared by RequestsPanel "Clear filters"
	await page.getByRole('tab', { name: /^Requests/ }).click();
	await expect(page.getByTestId('requests-queue')).toBeVisible();
	await page.getByLabel('Search work').fill('zzznorequestmatch');
	const noRequestMatch = page.getByTestId('no-request-match');
	await expect(noRequestMatch).toBeVisible();
	await noRequestMatch.getByRole('button', { name: 'Clear filters' }).click();
	await expect(page.getByLabel('Search work')).toHaveValue('');
	await page.getByRole('tab', { name: /^Tasks/ }).click();

	// List / Board layout toggle renders clean status columns & clickable cards opening TaskSheet
	await page.locator('[data-filter="all"]').click();
	const layoutGroup = page.getByRole('group', { name: 'Work layout' });
	const listBtn = layoutGroup.getByRole('button', { name: 'List' });
	const boardBtn = layoutGroup.getByRole('button', { name: 'Board' });
	await expect(listBtn).toHaveAttribute('aria-pressed', 'true');
	await boardBtn.click();
	await expect(boardBtn).toHaveAttribute('aria-pressed', 'true');
	const board = page.getByTestId('work-board');
	await expect(board).toBeVisible();
	await expect(board.locator('.work-bcol')).toHaveCount(4);
	await board.getByRole('button', { name: 'Custody lock fix' }).click();
	await expect(page.getByRole('dialog')).toContainText('Custody lock fix');
	await page.keyboard.press('Escape');
	await expect(page.getByRole('dialog')).toBeHidden();
	await listBtn.click();
	await expect(listBtn).toHaveAttribute('aria-pressed', 'true');
	await expect(page.getByTestId('work-list')).toBeVisible();

	// Exact 720px viewport hit targets (>= 44px inclusive width <= 720px)
	await page.setViewportSize({ width: 720, height: 900 });
	await page.getByLabel('Search work').fill('Custody');
	for (const locator of [
		page.getByRole('tab', { name: /^Tasks/ }),
		listBtn,
		boardBtn,
		page.getByRole('button', { name: 'Filter work' }),
		page.getByRole('button', { name: 'Work display' }),
		page.getByRole('button', { name: 'New task' }),
		page.getByRole('button', { name: 'Clear search' })
	]) {
		await expect(locator).toBeVisible();
		expect((await locator.boundingBox())?.height ?? 0).toBeGreaterThanOrEqual(44);
	}
	await page.getByRole('button', { name: 'Filter work' }).click();
	const projMenuItem = page.getByRole('menuitemradio', { name: 'All projects' });
	await expect(projMenuItem).toBeVisible();
	await expect
		.poll(async () => (await projMenuItem.boundingBox())?.height ?? 0)
		.toBeGreaterThanOrEqual(44);
	await page.keyboard.press('Escape');
});

test('Phase 4B + 5B: Work priority bars on every ticket, full-width reading panel with search dismiss, Activity & Projects toolbars, CmdK # channel jump, and Report masthead simplification', async ({
	page
}) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await visit(page, '/work?theme=graphite&mock=poc');

	// Priority signal bars appear on EVERY Work row (Tasks list, Bugs list, AND Board cards), including normal/low/no-priority rows
	await page.locator('[data-filter="all"]').click();
	const taskRows = page.getByTestId('work-row');
	const taskRowCount = await taskRows.count();
	expect(taskRowCount).toBeGreaterThan(2);
	await expect(
		page.locator('[data-testid="work-row"] .work-priority-mark[data-priority-level]')
	).toHaveCount(taskRowCount);
	// Normal-priority rows render level 2 bars (never omitted)
	await expect(
		page.locator('[data-testid="work-row"] .work-priority-mark[data-priority-level="2"]').first()
	).toBeVisible();

	// Board view cards also render priority signal bars on every card
	const layoutGroup = page.getByRole('group', { name: 'Work layout' });
	await layoutGroup.getByRole('button', { name: 'Board' }).click();
	const boardCards = page.getByTestId('work-card');
	const boardCardCount = await boardCards.count();
	expect(boardCardCount).toBe(taskRowCount);
	await expect(
		page.locator('[data-testid="work-card"] .work-priority-mark[data-priority-level]')
	).toHaveCount(boardCardCount);
	await layoutGroup.getByRole('button', { name: 'List' }).click();

	// Bugs tab renders no-priority hollow bars (data-priority-level="0") on bugs without priority
	await page.getByRole('tab', { name: /^Bugs/ }).click();
	await page.locator('[data-filter="all"]').click();
	const bugRows = page.getByTestId('work-row');
	const bugRowCount = await bugRows.count();
	expect(bugRowCount).toBeGreaterThan(0);
	await expect(
		page.locator('[data-testid="work-row"] .work-priority-mark[data-priority-level]')
	).toHaveCount(bugRowCount);
	await expect(
		page.locator('[data-testid="work-row"] .work-priority-mark[data-priority-level="0"]').first()
	).toBeVisible();
	await page.getByRole('tab', { name: /^Tasks/ }).click();
	await page.locator('[data-filter="all"]').click();

	// Opening a task expands .work-reading full-width, hides .work-toolbar & #work-results, and shows Back to Work (>= 36px desktop)
	const custodyBtn = page.getByRole('button', { name: 'Custody lock fix' });
	await custodyBtn.click();

	const workspace = page.locator('.work-workspace');
	const readingPanel = page.getByTestId('work-reading-panel');
	await expect(workspace).toHaveClass(/has-work-detail/);
	await expect(readingPanel).toBeVisible();
	await expect(readingPanel).toHaveAttribute('role', 'dialog');
	await expect(readingPanel).toHaveAttribute('aria-modal', 'false');
	await expect(page.locator('.work-toolbar')).toBeHidden();
	await expect(page.locator('#work-results')).toBeHidden();
	await expect(readingPanel).toContainText('Custody lock fix');
	await expect(readingPanel).toContainText('Implementation and unit checks complete');
	await expect(readingPanel).toHaveCSS('background-color', 'rgb(24, 25, 26)');

	// Verify full-width expansion on desktop (matches workspace width, no cramped split column)
	const workspaceBox = (await workspace.boundingBox())!;
	const panelBox = (await readingPanel.boundingBox())!;
	expect(panelBox.width).toBeGreaterThanOrEqual(workspaceBox.width - 2);

	// Back to Work button is visible at desktop (>= 36px) and Copy ref button meets >= 36px hit target
	const backBtn = readingPanel.getByRole('button', { name: 'Back to Work' });
	await expect(backBtn).toBeVisible();
	expect((await backBtn.boundingBox())?.height ?? 0).toBeGreaterThanOrEqual(36);
	const copyRefBtn = readingPanel.getByRole('button', { name: /^Copy / });
	await expect(copyRefBtn).toBeVisible();
	expect((await copyRefBtn.boundingBox())?.height ?? 0).toBeGreaterThanOrEqual(36);

	// Escape while native <select> has focus does NOT dismiss the reading panel
	const statusSelect = readingPanel.locator('#task-status');
	await statusSelect.focus();
	await page.keyboard.press('Escape');
	await expect(readingPanel).toBeVisible();

	// Returning via Back to Work and opening Task B shows B's title & description with zero leaked A values
	await backBtn.click();
	await expect(readingPanel).toBeHidden();
	await expect(page.locator('.work-toolbar')).toBeVisible();
	await expect(page.locator('#work-results')).toBeVisible();
	await expect(custodyBtn).toBeFocused();

	const secondTaskBtn = page.getByRole('button', { name: 'CSP gates in the UI suite' });
	await secondTaskBtn.click();
	await expect(readingPanel.locator('#work-detail-title')).toHaveText('CSP gates in the UI suite');
	await expect(readingPanel.locator('#task-title')).toHaveValue('CSP gates in the UI suite');
	await expect(readingPanel.locator('#task-title')).toBeEnabled();
	await expect(readingPanel.locator('#task-description')).not.toHaveValue(/Custody lock fix/);

	// Typing into #work-search while .work-reading is open closes .work-reading and immediately shows filtered search matches
	const workSearch = page.locator('#work-search');
	await workSearch.fill('Custody');
	await expect(readingPanel).toBeHidden();
	await expect(workspace).not.toHaveClass(/has-work-detail/);
	await expect(page.locator('.work-toolbar')).toBeVisible();
	await expect(page.locator('#work-results')).toBeVisible();
	await expect(workSearch).toBeFocused();
	await expect(page.getByTestId('work-row')).toHaveCount(1);
	await expect(page.getByTestId('work-row').first()).toContainText('Custody lock fix');
	await page.getByRole('button', { name: 'Clear search' }).click();
	await expect(workSearch).toHaveValue('');

	// Escape closes inline reading panel and restores focus to triggering row button
	await custodyBtn.click();
	await expect(readingPanel).toBeVisible();
	await readingPanel.locator('#work-detail-title').focus();
	await page.keyboard.press('Escape');
	await expect(readingPanel).toBeHidden();
	await expect(workspace).not.toHaveClass(/has-work-detail/);
	await expect(custodyBtn).toBeFocused();

	// Below 960px (720px): Back to Work button >= 44px and hides toolbar & results while open
	await page.setViewportSize({ width: 720, height: 900 });
	await custodyBtn.click();
	await expect(readingPanel).toBeVisible();
	await expect(page.locator('.work-toolbar')).toBeHidden();
	await expect(page.locator('#work-results')).toBeHidden();
	await expect(backBtn).toBeVisible();
	expect((await backBtn.boundingBox())?.height ?? 0).toBeGreaterThanOrEqual(44);
	await backBtn.click();
	await expect(readingPanel).toBeHidden();
	await expect(page.locator('#work-results')).toBeVisible();
	await expect(custodyBtn).toBeFocused();

	// Activity .activity-header + .activity-toolbar render cleanly (with feed filter tabs in toolbar when Activity view is active)
	await page.setViewportSize({ width: 1440, height: 900 });
	await page
		.getByRole('navigation', { name: 'Main' })
		.getByRole('link', { name: 'Activity' })
		.click();
	await expect(page).toHaveURL(/\/activity$/);
	await expect(page.locator('.section-header.activity-header')).toBeVisible();
	await expect(page.locator('.section-toolbar.activity-toolbar')).toBeVisible();
	await page.getByRole('main').getByRole('button', { name: 'Activity', exact: true }).click();
	await expect(
		page.locator('.activity-toolbar').getByRole('tablist', { name: 'Filter activity stream' })
	).toBeVisible();

	// Projects .section-header + .project-toolbar render cleanly on directory & project detail pages
	await page
		.getByRole('navigation', { name: 'Main' })
		.getByRole('link', { name: 'Projects' })
		.click();
	await expect(page).toHaveURL(/\/projects$/);
	await expect(page.locator('.section-header.project-header')).toBeVisible();
	await expect(page.locator('.section-toolbar.project-toolbar')).toBeVisible();
	await page.getByRole('main').getByRole('link', { name: 'Office federation' }).click();
	await expect(page).toHaveURL(/\/projects\/office-federation$/);
	await expect(page.locator('.section-header.project-header')).toBeVisible();
	await expect(page.locator('.section-toolbar.project-toolbar')).toBeVisible();

	// CmdK `#` channel jump mode surfaces live Channels group cleanly and navigates to matching Chat conversation
	await page.keyboard.press('ControlOrMeta+k');
	const cmdDialog = page.getByRole('dialog', { name: 'Command palette and Org AI' });
	await expect(cmdDialog).toBeVisible();
	await cmdDialog.getByRole('combobox').fill('#office');
	await expect(
		cmdDialog.getByRole('option', { name: /#office-federation.*Channels/ })
	).toBeVisible();
	await page.keyboard.press('Enter');
	await expect(cmdDialog).toBeHidden();
	await expect(page).toHaveURL(/\/chat\?c=c-project-office-federation$/);
	await expect(page.getByTestId('conversation-title')).toHaveText('#office-federation');

	// Single-key `#` shortcut on /chat opens CmdK directly in `#` channel jump mode
	await page.locator('main').click({ position: { x: 12, y: 12 } });
	await page.keyboard.press('#');
	await expect(cmdDialog).toBeVisible();
	await expect(cmdDialog.getByRole('combobox')).toHaveValue('#');
	await expect(cmdDialog.getByRole('option', { name: /#general.*Channels/ })).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(cmdDialog).toBeHidden();

	// Report Day & Report Night masthead simplification: no obsolete EXECUTIVE OPERATIONS BRIEFING ::before banner
	for (const theme of ['report', 'report-dark'] as const) {
		await visit(page, `/home?theme=${theme}&mock=poc`);
		const mastheadBefore = await page
			.locator('.home-brief')
			.evaluate((el) => getComputedStyle(el, '::before').content);
		expect(mastheadBefore === 'none' || mastheadBefore === 'normal').toBe(true);
	}
});

test('Phase 4B: saving inline Task A while switching to Task B keeps B enabled, refreshes Work queue, and Ask -> Search # loads real Chat channels', async ({
	page
}) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await enterMockedOrganization(page);
	await visit(page, '/work?mock=default');
	const createTask = async (title: string) => {
		await page.getByRole('button', { name: 'New task' }).click();
		const dialog = page.getByRole('dialog', { name: 'New task' });
		await dialog.getByLabel('Title').fill(title);
		await dialog.getByRole('button', { name: 'Create task' }).click();
		await expect(dialog).toBeHidden();
	};
	await createTask('Alpha task');
	await createTask('Beta task');

	// Open Alpha, edit its title, close via Back to Work (flushing blur-save), and immediately open Beta
	await page.getByRole('button', { name: 'Alpha task' }).click();
	const readingPanel = page.locator('.work-reading');
	await expect(readingPanel.locator('#task-title')).toHaveValue('Alpha task');
	await readingPanel.locator('#task-title').fill('Alpha task — saved');
	await readingPanel.getByRole('button', { name: 'Back to Work' }).click();
	await expect(readingPanel).toBeHidden();
	await expect(page.getByRole('button', { name: 'Alpha task — saved' })).toBeVisible();
	await page.getByRole('button', { name: 'Beta task' }).click();

	// Beta stays clean and interactive with zero leaked Alpha values
	await expect(readingPanel.locator('#work-detail-title')).toHaveText('Beta task');
	await expect(readingPanel.locator('#task-title')).toHaveValue('Beta task');
	await expect(readingPanel.locator('#task-title')).toBeEnabled();

	// Editing Beta's title and pressing Escape blurs & saves while keeping the reading panel open
	// so errors/conflicts stay visible; pressing Escape again closes the reading panel
	await readingPanel.locator('#task-title').fill('Beta task — escape saved');
	await page.keyboard.press('Escape');
	await expect(readingPanel).toBeVisible();
	await expect(readingPanel.locator('#work-detail-title')).toBeFocused();
	await page.keyboard.press('Escape');
	await expect(readingPanel).toBeHidden();
	await expect(page.getByRole('button', { name: 'Beta task — escape saved' })).toBeVisible();

	// Deleting a task inside the inline reading panel never drops focus to document.body even after the row disappears
	await page.getByRole('button', { name: 'Beta task — escape saved' }).click();
	await readingPanel.getByRole('button', { name: 'Delete' }).click();
	await readingPanel.getByRole('button', { name: 'Delete task' }).click();
	await expect(readingPanel).toBeHidden();
	await expect(page.getByRole('button', { name: 'Beta task — escape saved' })).toBeHidden();
	await expect
		.poll(() => page.evaluate(() => document.activeElement?.tagName ?? 'BODY'))
		.not.toBe('BODY');

	// Ask -> Search & Commands tab switch loads real Chat conversations on first single-event `#release-v1` query
	// (#release-v1 exists only in the real Chat conversation snapshot, not the snapshot-derived fallback)
	await page.getByRole('button', { name: 'Ask', exact: true }).click();
	const cmdDialog = page.getByRole('dialog', { name: 'Command palette and Org AI' });
	await expect(cmdDialog).toBeVisible();
	await cmdDialog.getByRole('tab', { name: 'Search & Commands' }).click();
	await cmdDialog.getByRole('combobox').fill('#release-v1');
	await expect(cmdDialog.getByRole('option', { name: /#release-v1.*Channels/ })).toBeVisible();
	await page.keyboard.press('Enter');
	await expect(cmdDialog).toBeHidden();
	await expect(page).toHaveURL(/\/chat\?c=c-release-v1$/);
	await expect(page.getByTestId('conversation-title')).toHaveText('#release-v1');
});
