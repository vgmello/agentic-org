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
	const homeCombo = page.getByRole('combobox', { name: 'Search, run a command, or ask Tess' });
	await expect(homeCombo).toBeVisible();
	await page.keyboard.press('ControlOrMeta+k');
	await expect(homeCombo).toBeFocused();
	await homeCombo.fill('Theme: Classic');
	const inlineListbox = page.locator('#exec-cmdk-home-listbox');
	await expect(inlineListbox).toBeVisible();
	await expect(homeCombo).toHaveAttribute('aria-controls', 'exec-cmdk-home-listbox');
	await expect(inlineListbox.getByRole('option', { name: /^Theme: Classic/ })).toBeVisible();
	await expect(
		inlineListbox.getByRole('option', { name: /Ask Tess about 'Theme: Classic'/ })
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

	// Command bar Ask opens persistent right-hand Tess Chief of Staff Side Panel
	await page.getByRole('button', { name: 'Ask', exact: true }).click();
	const tessPanel = page.getByTestId('tess-panel');
	await expect(tessPanel).toBeVisible();
	await tessPanel.getByRole('button', { name: 'Budget, offices & active agents' }).click();
	await expect(tessPanel).toContainText('agents active');
	await expect(tessPanel).toContainText('Offline quick summary · Local data only');
	await tessPanel.getByRole('button', { name: 'Close Tess panel' }).click();
	await expect(tessPanel).toBeHidden();

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

	// Keyboard Tab from the last control in command bar (Ask button) reaches Home scroll cue before below-fold content
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
	const pocCombo = page.getByRole('combobox', { name: 'Search, run a command, or ask Tess' });
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

	// Decision Spotlight "Ask about this" opens Tess Side Panel with that decision's answer & context pill
	await page.getByRole('button', { name: 'Ask about this' }).click();
	const tessPanel = page.getByTestId('tess-panel');
	await expect(tessPanel).toBeVisible();
	await expect(tessPanel).toContainText("Approve publishing Kai's sync update to v1.0");
	await expect(tessPanel).toContainText('Ada recommends: approve.');
	await tessPanel.getByRole('button', { name: 'Close Tess panel' }).click();
	await expect(tessPanel).toBeHidden();

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

	// Opening a POC work item opens the inline WorkReadingPanel (with Back to Work + Open full page -> /work/T-118)
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Work' }).click();
	await expect(page).toHaveURL(/\/work$/);
	const custodyLink = page.getByRole('link', { name: /Custody lock fix/ }).first();
	await custodyLink.click();
	const readingPanel = page.getByTestId('work-reading-panel');
	await expect(readingPanel).toBeVisible();
	await expect(readingPanel).toContainText('Custody lock fix');
	await expect(
		readingPanel.getByText('Implementation and unit checks complete').first()
	).toBeVisible();

	if (captureDir) {
		await page.screenshot({ path: `${captureDir}/poc-mirror-work-sheet-report.png` });
	}

	await readingPanel.getByRole('button', { name: 'Back to Work' }).click();
	await expect(readingPanel).toBeHidden();
	await expect(custodyLink).toBeFocused();
	await custodyLink.click();
	await expect(readingPanel).toBeVisible();
	await readingPanel.getByRole('link', { name: 'Open full page for T-118' }).click();
	await expect(page).toHaveURL(/\/work\/T-118$/);
	await expect(page.getByRole('heading', { name: /Custody lock fix/ })).toBeVisible();
});

test('Phase 2 Inbox parity: 3 primary tabs, More/Filter/Display menus, Graphite surfaces, member card Escape, comments persistence, and dark CmdK contrast', async ({
	page
}) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await visit(page, '/inbox?theme=graphite&mock=poc');

	// Graphite Inbox matte surfaces, top-lit sidebar lighting & queue sheen (13abb1f1..2c58edcd)
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'graphite');
	await expect(page.locator('html')).toHaveAttribute('data-interior-page');
	await expect(page.locator('aside.exec-sidebar')).toHaveCSS('background-color', 'rgb(19, 21, 22)');
	const sidebarLightBg = await page
		.locator('aside.exec-sidebar')
		.evaluate((el) => getComputedStyle(el, '::before').backgroundImage);
	expect(sidebarLightBg).toContain('radial-gradient');
	await expect(page.locator('.triage-queue')).toHaveCSS('background-color', 'rgb(27, 28, 29)');
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
		await page.keyboard.press('ControlOrMeta+k');
		const dialog = page.locator('.exec-cmdk-dialog');
		await expect(dialog).toBeVisible();
		await expect(dialog).toHaveCSS('border-top-color', 'rgb(131, 147, 171)');
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
			const escBtn = el.querySelector<HTMLElement>('.exec-cmdk-close')!;
			const borderRgb = parseRgb(getComputedStyle(el).borderTopColor);
			const pageRgb = parseRgb(
				getComputedStyle(document.querySelector('.exec-canvas')!).backgroundColor
			);
			return {
				borderVsPage: ratio(borderRgb, pageRgb),
				escText: ratio(
					parseRgb(getComputedStyle(escBtn).color),
					parseRgb(getComputedStyle(escBtn).backgroundColor)
				)
			};
		});
		expect(contrast.borderVsPage).toBeGreaterThanOrEqual(3.0);
		expect(contrast.escText).toBeGreaterThanOrEqual(4.5);
		await page.keyboard.press('Escape');
		await expect(dialog).toBeHidden();
	}
});

test('Phase 3 Work parity: unified WorkQueueView header, live search & / shortcut, Board/List toggle, Graphite matte canvas, priority bars, auto-hide switcher & POC org switch, and 720px hit targets', async ({
	page
}) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await enterMockedOrganization(page, 'Lumen Labs');
	await visit(page, '/work?theme=graphite&mock=poc');

	// Graphite matte Work canvas (#18191a) + unified .work-header.queue-header geometry
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'graphite');
	await expect(page.locator('html')).toHaveAttribute('data-interior-page');
	await expect(page.locator('main')).toHaveCSS('background-color', 'rgb(24, 25, 26)');
	await expect(page.locator('aside.exec-sidebar')).toHaveCSS('background-color', 'rgb(19, 21, 22)');
	await expect(page.locator('header.work-header.queue-header')).toBeVisible();
	await expect(page.locator('#shell-page-title')).toContainText('Work');

	// `/` from explicit /work?view=needs_me awaits navigation and focuses Search work
	await visit(page, '/work?view=needs_me&theme=graphite');
	await expect(page.getByTestId('needs-me-list')).toBeVisible();
	await page.locator('#shell-page-title').focus();
	await page.keyboard.press('/');
	const searchInput = page.getByLabel('Search work');
	await expect(searchInput).toBeFocused();
	await searchInput.press('Escape');
	await expect(searchInput).not.toBeFocused();

	// Switch to Tasks tab and List layout so we can test search, filters, priority bars, and Board/List switching
	await page
		.getByRole('tablist', { name: 'Work', exact: true })
		.getByRole('tab', { name: /^Tasks/ })
		.click();
	await page.getByTestId('saved-filters').locator('[data-filter="all"]').click();

	// Board view renders 4 columns and priority signal bars on every card
	const layoutGroup = page.getByRole('group', { name: 'Layout' });
	const boardBtn = layoutGroup.getByRole('button', { name: 'Board' });
	const listBtn = layoutGroup.getByRole('button', { name: 'List' });
	await expect(boardBtn).toHaveAttribute('aria-pressed', 'true');
	const board = page.getByTestId('work-board');
	await expect(board).toBeVisible();
	await expect(board.getByTestId('board-column')).toHaveCount(4);
	const boardCardCount = await board.getByTestId('work-card').count();
	expect(boardCardCount).toBeGreaterThan(2);
	await expect(
		board.locator('[data-testid="work-card"] .work-priority-mark[data-priority-level]')
	).toHaveCount(boardCardCount);

	// Switch to List view: priority signal bars appear on every desktop row
	await listBtn.click();
	await expect(listBtn).toHaveAttribute('aria-pressed', 'true');
	const workRows = page.getByTestId('work-row');
	const initialCount = await workRows.count();
	expect(initialCount).toBeGreaterThan(2);
	await expect(
		page.locator('[data-testid="work-row"] .work-priority-mark[data-priority-level]')
	).toHaveCount(initialCount);

	// `/` shortcut focuses Work search input; live search filters rows; Escape preserves query and exits field (1149b4f1)
	await page.locator('#shell-page-title').focus();
	await page.keyboard.press('/');
	await expect(searchInput).toBeFocused();
	await searchInput.fill('Custody');
	await expect(workRows).toHaveCount(1);
	await expect(workRows.first()).toContainText('Custody lock fix');
	await searchInput.press('Escape');
	await expect(searchInput).toHaveValue('Custody');
	await expect(searchInput).not.toBeFocused();
	await page.getByRole('button', { name: 'Clear filters' }).click();
	await expect(searchInput).toHaveValue('');
	await expect(workRows).toHaveCount(initialCount);

	// Exact 720px viewport hit targets (>= 44px inclusive width <= 720px)
	await page.setViewportSize({ width: 720, height: 900 });
	for (const locator of [
		page.getByRole('tablist', { name: 'Work', exact: true }).getByRole('tab', { name: /^Tasks/ }),
		listBtn,
		boardBtn,
		page.getByRole('button', { name: 'New task' })
	]) {
		await expect(locator).toBeVisible();
		expect((await locator.boundingBox())?.height ?? 0).toBeGreaterThanOrEqual(44);
	}

	// Auto-hide sidebar: opening OrganizationSwitcher by keyboard keeps sidebar expanded & trigger mounted
	await page.setViewportSize({ width: 1440, height: 900 });
	await page.getByRole('button', { name: 'Auto-hide sidebar' }).click();
	await page.mouse.move(900, 300);
	const sidebar = page.locator('aside.exec-sidebar');
	await expect(sidebar).toHaveAttribute('data-collapsed', 'true');
	// Keyboard Tab inside sidebar rail sets :focus-visible, expands rail, and keeps OrganizationSwitcher open on Enter
	await sidebar.getByRole('link', { name: 'Agentic Org home' }).focus();
	await page.keyboard.press('Tab');
	await expect(sidebar).not.toHaveAttribute('data-collapsed', 'true');
	const switcherBtn = sidebar.getByRole('button', { name: 'Switch organization' });
	await expect(switcherBtn).toBeVisible();
	await switcherBtn.focus();
	await page.keyboard.press('Enter');
	await expect(page.getByRole('menu')).toBeVisible();
	await expect(sidebar).not.toHaveAttribute('data-collapsed', 'true');
	await page.keyboard.press('Escape');
	await expect(page.getByRole('menu')).toBeHidden();
	await expect(switcherBtn).toBeVisible();

	// Legacy /work?request=<id> links resolve directly to /work/[ref]
	await visit(page, '/work?request=w-r11');
	await expect(page).toHaveURL(/\/work\/R-11$/);
	await expect(
		page.getByRole('heading', { name: /Invite flow for a second office/ })
	).toBeVisible();
	await visit(page, '/work');

	// Switching away from POC organization persists instead of reverting to Agentic Org
	const sidebarAfter = page.locator('aside.exec-sidebar');
	await sidebarAfter.getByRole('link', { name: 'Agentic Org home' }).focus();
	await page.keyboard.press('Tab');
	await expect(sidebarAfter).not.toHaveAttribute('data-collapsed', 'true');
	await sidebarAfter.getByRole('button', { name: 'Switch organization' }).click();
	await page.getByRole('menuitem', { name: /Lumen Labs/ }).click();
	await expect(sidebarAfter.getByRole('link', { name: 'Lumen Labs home' })).toBeVisible();
	await page.reload();
	await page.locator('html[data-hydrated="true"]').waitFor();
	await expect(
		page.locator('aside.exec-sidebar').getByRole('link', { name: 'Lumen Labs home' })
	).toBeVisible();
});

test('Phase 4B + 5B: Activity & Projects toolbars, CmdK # channel jump, Ask -> Search # real Chat channels, and Report masthead simplification', async ({
	page
}) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await visit(page, '/activity?theme=graphite&mock=poc');

	// Activity .activity-header + .activity-toolbar render cleanly (with feed filter tabs in toolbar when Activity view is active)
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
	const cmdDialog = page.getByRole('dialog', { name: 'Command palette' });
	await expect(cmdDialog).toBeVisible();
	await cmdDialog.getByRole('combobox').fill('#office');
	await expect(
		cmdDialog.getByRole('option', { name: /#office-federation.*Channel/ })
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
	await expect(cmdDialog.getByRole('option', { name: /#general.*Channel/ })).toBeVisible();
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

	// Cmd+K after Cmd+/ Ask Tess loads real Chat conversations on first single-event `#release-v1` query
	await enterMockedOrganization(page);
	await visit(page, '/work?mock=default');
	await page.keyboard.press('ControlOrMeta+/');
	await expect(page.getByTestId('tess-panel')).toBeVisible();
	await page.keyboard.press('ControlOrMeta+k');
	await expect(cmdDialog).toBeVisible();
	await cmdDialog.getByRole('combobox').fill('#release-v1');
	await expect(cmdDialog.getByRole('option', { name: /#release-v1.*Channel/ })).toBeVisible();
	await page.keyboard.press('Enter');
	await expect(cmdDialog).toBeHidden();
	await expect(page).toHaveURL(/\/chat\?c=c-release-v1$/);
	await expect(page.getByTestId('conversation-title')).toHaveText('#release-v1');
});

test('Phase 6 (Prototype Release 3c4e3541): S/M keyboard navigation, Work sliding panel + 6-status board + 2-level grouping, Activity collapsible Release gates dock, Classic crisp radii, and icon tooltips', async ({
	page
}) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await visit(page, '/home?theme=classic&mock=poc');

	// Classic theme crisp 5px/7px radii parity (3c4e3541)
	const classicRadii = await page.evaluate(() => {
		const s = getComputedStyle(document.documentElement);
		return {
			panel: s.getPropertyValue('--exec-panel-radius').trim(),
			ctrl: s.getPropertyValue('--exec-ctrl-radius').trim()
		};
	});
	expect(classicRadii.panel).toBe('7px');
	expect(classicRadii.ctrl).toBe('5px');

	// S -> I single-key sidebar navigation navigates to /inbox
	await page.locator('main').click({ position: { x: 12, y: 12 } });
	await page.keyboard.press('s');
	await expect(page.getByTestId('nav-key-hint')).toContainText('Sidebar');
	await page.keyboard.press('i');
	await expect(page).toHaveURL(/\/inbox$/);

	// M -> A single-key Inbox category navigation switches to Approvals; M -> H opens overflow Handled
	await page.locator('main').click({ position: { x: 12, y: 12 } });
	await page.keyboard.press('m');
	await expect(page.getByTestId('nav-key-hint')).toContainText('Inbox categories');
	await page.keyboard.press('a');
	await expect(
		page
			.getByRole('group', { name: 'Inbox categories' })
			.getByRole('button', { name: /^Approvals/ })
	).toHaveAttribute('aria-pressed', 'true');

	await page.keyboard.press('m');
	await page.keyboard.press('h');
	await expect(page.locator('.triage-queue-summary')).toContainText('Handled items');

	// Hovering icon-only Inbox filter button reveals a visible tooltip
	const filterBtn = page.getByRole('button', { name: 'Filter inbox' });
	await filterBtn.hover();
	await expect(page.locator('[data-tooltip-content]', { hasText: 'Filter inbox' })).toBeVisible();
	await page.keyboard.press('Escape');

	// Work: 4 stages default, opt-in 6 statuses toggle, 2-level grouping, and right-hand sliding panel with expand/restore & backdrop close
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Work' }).click();
	await expect(page).toHaveURL(/\/work$/);
	await page.getByRole('tab', { name: /^Tasks/ }).click();
	await page.getByTestId('saved-filters').locator('[data-filter="all"]').click();
	await expect(page.getByTestId('board-column')).toHaveCount(4);

	const boardModeToggle = page.getByTestId('board-mode-toggle');
	await boardModeToggle.getByRole('button', { name: '6 statuses' }).click();
	await expect(page.getByTestId('board-column')).toHaveCount(6);
	await boardModeToggle.getByRole('button', { name: '4 stages' }).click();
	await expect(page.getByTestId('board-column')).toHaveCount(4);

	// Switch to List and verify 2-level grouping (Status -> Assignee) puts Blocked first
	await page.getByRole('group', { name: 'Layout' }).getByRole('button', { name: 'List' }).click();
	await page.getByTestId('group-by-select').selectOption('status');
	await page.getByTestId('subgroup-by-select').selectOption('assignee');
	await expect(page.getByTestId('work-subgroup-heading').first()).toBeVisible();

	// Open Work item -> right-hand sliding panel mounts over inert queue, expands/restores, traps Shift+Tab, backdrop closes
	const firstWorkLink = page.locator('[data-ref] a').first();
	await firstWorkLink.click();
	const readingPanel = page.getByTestId('work-reading-panel');
	await expect(readingPanel).toBeVisible();
	await expect(readingPanel.locator('#work-detail-title')).toBeVisible();

	const expandBtn = readingPanel.getByRole('button', { name: 'Expand panel to full width' });
	await expandBtn.hover();
	await expect(page.locator('[data-tooltip-content]', { hasText: 'Expand panel' })).toBeVisible();
	await expandBtn.click();
	await expect(readingPanel).toHaveAttribute('data-expanded', '');
	await readingPanel.getByRole('button', { name: 'Restore panel width' }).click();
	await expect(readingPanel).not.toHaveAttribute('data-expanded');

	await readingPanel.getByRole('button', { name: 'Back to Work' }).focus();
	await page.keyboard.press('Shift+Tab');
	const focusInsidePanel = await readingPanel.evaluate((el) => el.contains(document.activeElement));
	expect(focusInsidePanel).toBe(true);
	// Shell shortcut `S` must remain blocked while the modal Work reading panel is open.
	await readingPanel.getByRole('button', { name: 'Back to Work' }).focus();
	await page.keyboard.press('s');
	await expect(page.getByTestId('nav-key-hint')).toBeHidden();
	expect(await readingPanel.evaluate((el) => el.contains(document.activeElement))).toBe(true);

	await page.locator('.work-panel-backdrop').click({ position: { x: 320, y: 200 } });
	await expect(readingPanel).toBeHidden();

	// Subgroup state resets cleanly when switching from Bugs (Severity) to Requests
	await page.getByRole('tab', { name: /^Bugs/ }).click();
	await page.getByTestId('subgroup-by-select').selectOption('severity');
	await page.getByRole('tab', { name: /^Requests/ }).click();
	await expect(page.getByTestId('subgroup-by-select')).toHaveValue('none');

	// Activity: full-space Assembly Line with Readiness progress bar & collapsible Release gates dock + tooltip-first Escape
	await page
		.getByRole('navigation', { name: 'Main' })
		.getByRole('link', { name: 'Activity' })
		.click();
	await expect(page).toHaveURL(/\/activity$/);
	await expect(page.locator('.assembly-metrics .assembly-progress-bar')).toBeVisible();

	const gatesToggle = page.getByTestId('activity-gates-toggle');
	await expect(gatesToggle).toHaveAttribute('aria-expanded', 'true');
	await expect(page.locator('#activity-release-panel')).toBeVisible();
	await gatesToggle.focus();
	await gatesToggle.hover();
	await expect(
		page.locator('[data-tooltip-content]', { hasText: 'Hide release gates' })
	).toBeVisible();
	// First Escape dismisses the open tooltip without collapsing the dock; second Escape collapses the dock.
	await page.keyboard.press('Escape');
	await expect(
		page.locator('[data-tooltip-content]', { hasText: 'Hide release gates' })
	).toBeHidden();
	await expect(page.locator('#activity-release-panel')).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(gatesToggle).toHaveAttribute('aria-expanded', 'false');
	await expect(page.locator('#activity-release-panel')).toBeHidden();
	await gatesToggle.click();
	await expect(gatesToggle).toHaveAttribute('aria-expanded', 'true');
	await expect(page.locator('#activity-release-panel')).toBeVisible();

	// 6-status board on default mock dataset populates Awaiting verification with fixed bug T-A42;
	// generic board status moves remain limited to Tasks (bugs transition via their detail workflow).
	await enterMockedOrganization(page);
	await visit(page, '/work?mock=default');
	await page.getByRole('tab', { name: /^Tasks/ }).click();
	await page.getByTestId('saved-filters').locator('[data-filter="all"]').click();
	await page.getByTestId('board-mode-toggle').getByRole('button', { name: '6 statuses' }).click();
	const awaitingCol = page
		.getByTestId('board-column')
		.filter({ has: page.getByRole('heading', { name: /Awaiting verification/ }) });
	await expect(awaitingCol).toBeVisible();
	const bugCard = awaitingCol.locator('[data-ref="T-A42"]');
	await expect(bugCard).toBeVisible();
	await expect(bugCard).toHaveAttribute('draggable', 'false');
	await bugCard.focus();
	await page.keyboard.press('Alt+ArrowLeft');
	await expect(page.getByRole('dialog')).toHaveCount(0);
	await expect(awaitingCol.locator('[data-ref="T-A42"]')).toBeVisible();

	// Mobile (<640px): S -> I opens mobile sidebar drawer & navigates to /inbox; M -> A from mobile Inbox detail
	// survives ?item= clearing and selects Approvals; S then clicking a drawer link navigates cleanly.
	await page.setViewportSize({ width: 390, height: 844 });
	await visit(page, '/home?theme=graphite&mock=poc');
	await page.locator('main').click({ position: { x: 12, y: 12 } });
	await page.keyboard.press('s');
	await expect(page.getByTestId('nav-key-hint')).toContainText('Sidebar');
	await page.keyboard.press('i');
	await expect(page).toHaveURL(/\/inbox$/);
	await page.locator('[data-triage-row]').first().click();
	const mobileDetail = page.getByRole('region', { name: 'Item details' });
	await expect(mobileDetail).toBeVisible();
	await expect(page.locator('#triage-categories')).toBeHidden();
	await mobileDetail.click({ position: { x: 16, y: 16 } });
	await page.keyboard.press('m');
	await expect(page.locator('#triage-categories')).toBeVisible();
	await expect(page).toHaveURL(/\/inbox$/);
	await expect(page.getByTestId('nav-key-hint')).toContainText('Inbox categories');
	await page.keyboard.press('a');
	await expect(
		page
			.getByRole('group', { name: 'Inbox categories' })
			.getByRole('button', { name: /^Approvals/ })
	).toHaveAttribute('aria-pressed', 'true');
	await page.keyboard.press('s');
	await expect(page.getByTestId('nav-key-hint')).toContainText('Sidebar');
	await page
		.locator('[data-exec-sidebar-nav]:visible')
		.getByRole('link', { name: 'Activity' })
		.click();
	await expect(page).toHaveURL(/\/activity$/);
});

test('TASK-81: Inbox snooze presets, custom validation & expiry warning, Snoozed view, re-snooze, and one-click Un-snooze', async ({
	page
}) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await visit(page, '/inbox?theme=graphite&mock=poc');

	await expect(page.locator('[data-triage-row]').first()).toBeVisible();
	const waitingBadge = page.locator('.queue-count');
	await expect(waitingBadge).toContainText(/[1-9]\d*\s+waiting on you/);
	const initialText = (await waitingBadge.textContent()) ?? '';
	const initialMatch = /(\d+)\s+waiting on you/.exec(initialText);
	expect(initialMatch).not.toBeNull();
	const initialOpen = Number(initialMatch![1]);
	expect(initialOpen).toBeGreaterThan(0);

	const detailHeading = page.locator('#triage-detail-heading');
	const firstTitle = ((await detailHeading.textContent()) ?? '').trim();
	expect(firstTitle.length).toBeGreaterThan(0);

	// Open Snooze menu on the first (expiring) item and verify past-time + after-expiry rejections match office rules.
	await page.getByTestId('inbox-snooze-trigger').click();
	const laterTodayItem = page.getByRole('menuitem', { name: /Later today/ });
	if ((await laterTodayItem.count()) > 0) {
		await expect(laterTodayItem).toContainText('Today,');
	}
	await expect(page.getByRole('menuitem', { name: /Tomorrow/ })).toBeVisible();
	await expect(page.getByRole('menuitem', { name: /Next week/ })).toBeVisible();
	await page.getByRole('menuitem', { name: /Custom date/ }).click();

	const dialog = page.getByRole('dialog');
	await expect(dialog.getByRole('heading', { name: 'Snooze until…' })).toBeVisible();
	await expect(dialog).toContainText('Not private: visible to the sender');

	// Reject past timestamp cleanly.
	await dialog.getByLabel('Return date').fill('2020-01-01');
	await dialog.getByLabel('Return time').fill('09:00');
	await dialog.getByRole('button', { name: 'Snooze' }).click();
	await expect(dialog.getByLabel('Return time')).toHaveAttribute('aria-invalid', 'true');
	await expect(dialog).toContainText('Return time must be in the future.');

	// Reject timestamp at/after item expiry (matching messaging_ops.rs expiry guard).
	await dialog.getByLabel('Return date').fill('2030-05-10');
	await dialog.getByLabel('Return time').fill('10:00');
	await expect(dialog.getByTestId('snooze-expiry-warning')).toBeVisible();
	await dialog.getByRole('button', { name: 'Snooze' }).click();
	await expect(dialog.getByLabel('Return time')).toHaveAttribute('aria-invalid', 'true');
	await expect(dialog.getByLabel('Return time')).toBeFocused();
	await expect(dialog.getByTestId('snooze-preview')).toBeHidden();
	await expect(dialog).toContainText(
		"Return time must be earlier than the item's expiry deadline."
	);
	await dialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(dialog).toBeHidden();

	// Select a non-expiring open item (Today group), verify snooze with note, Snoozed view, re-snooze, and Un-snooze.
	const todayRow = page.locator('section[aria-label="Today"] [data-triage-row]').first();
	await todayRow.click();
	const snoozedTitle = ((await detailHeading.textContent()) ?? '').trim();
	expect(snoozedTitle.length).toBeGreaterThan(0);

	await page.getByTestId('inbox-snooze-trigger').click();
	await page.getByRole('menuitem', { name: /Custom date/ }).click();
	await expect(dialog.getByRole('heading', { name: 'Snooze until…' })).toBeVisible();
	await dialog.getByLabel('Return date').fill('2030-05-10');
	await dialog.getByLabel('Return time').fill('10:00');
	await dialog.getByLabel('Snooze note (optional)').fill('Wait for post-deploy verification');
	await dialog.getByRole('button', { name: 'Snooze' }).click();
	await expect(dialog).toBeHidden();

	// Open count drops by 1 and item leaves open queue.
	await expect(waitingBadge).toContainText(`${initialOpen - 1} waiting on you`);

	// Switch to More -> Snoozed and verify return time, note, re-snooze, and one-click Un-snooze.
	const moreTrigger = page
		.getByRole('group', { name: 'Inbox categories' })
		.getByRole('button', { name: 'More' });
	await moreTrigger.click();
	const snoozedMenuItem = page.getByRole('menuitemradio', { name: /^Snoozed\s+1$/ });
	await expect(snoozedMenuItem).toBeVisible();
	await snoozedMenuItem.click();

	await expect(page.locator('.triage-queue-summary')).toContainText('Snoozed items');
	await expect(detailHeading).toHaveText(snoozedTitle);
	await expect(page.getByTestId('detail-snoozed-until')).toContainText('10 May 2030, 10:00');
	await expect(page.getByTestId('detail-snooze-note')).toHaveText(
		'Wait for post-deploy verification'
	);

	// Re-snooze via preset updates return time in place.
	await page.getByRole('button', { name: 'Re-snooze' }).click();
	await page.getByRole('menuitem', { name: /Tomorrow/ }).click();
	await expect(page.getByTestId('snoozed-badge')).toContainText('Tomorrow, 09:00');

	// One-click Un-snooze returns the item to the open Inbox queue.
	await page.getByRole('button', { name: 'Un-snooze' }).click();
	await expect(waitingBadge).toContainText(`${initialOpen} waiting on you`);
	await page
		.getByRole('group', { name: 'Inbox categories' })
		.getByRole('button', { name: /^All/ })
		.click();
	await expect(page.locator('[data-triage-row]', { hasText: snoozedTitle })).toBeVisible();

	// Live resurfacing without navigation: a snoozed item whose return time arrives resurfaces automatically
	// via shell inbox subscription and updates the open waiting badge.
	await visit(page, '/inbox?theme=graphite&mock=poc,snooze-due');
	await expect(
		page.locator('[data-triage-row]', { hasText: 'Resurfaced snoozed item' })
	).toBeVisible({ timeout: 10_000 });
	await expect(waitingBadge).toContainText(`${initialOpen + 1} waiting on you`);
});

test('Phase 7 (5da47e1d + 53728c05): Assembly Display row dividers persistence, Release gates dock independent scroll, and cross-theme surface parity', async ({
	page
}) => {
	await page.setViewportSize({ width: 1440, height: 900 });

	// 1. Portfolio Activity in Classic Night: Assembly & Release gates dock #1e2939, row dividers off by default & persisted
	await visit(page, '/activity?theme=classic&mock=poc');
	await expect(page.locator('html')).toHaveAttribute('data-ui-theme', 'classic');
	await expect(page.locator('.assembly-view')).toHaveCSS('background-color', 'rgb(30, 41, 57)');
	await expect(page.locator('#activity-release-panel')).toHaveCSS(
		'background-color',
		'rgb(30, 41, 57)'
	);

	await expect(page.locator('html')).not.toHaveAttribute('data-assembly-dividers');
	await page.getByTestId('assembly-display-btn').click();
	const dividersItem = page.getByTestId('assembly-row-dividers-toggle');
	await expect(dividersItem).toHaveAttribute('aria-checked', 'false');
	await dividersItem.click();
	await expect(page.locator('html')).toHaveAttribute('data-assembly-dividers', '');
	await page.reload();
	await page.locator('html[data-hydrated="true"]').waitFor();
	await expect(page.locator('html')).toHaveAttribute('data-assembly-dividers', '');
	// Turn back off so default state stays clean
	await page.getByTestId('assembly-display-btn').click();
	await page.getByTestId('assembly-row-dividers-toggle').click();
	await expect(page.locator('html')).not.toHaveAttribute('data-assembly-dividers');

	// 2. Theme surfaces across Report Day, Report Night, Graphite, Cozy on Assembly
	await visit(page, '/activity?theme=report&mock=poc');
	await expect(page.locator('.assembly-view')).toHaveCSS('background-color', 'rgb(255, 255, 255)');
	await visit(page, '/activity?theme=report-dark&mock=poc');
	await expect(page.locator('.assembly-view')).toHaveCSS('background-color', 'rgb(30, 41, 57)');
	await visit(page, '/activity?theme=graphite&mock=poc');
	await expect(page.locator('main')).toHaveCSS('background-color', 'rgb(24, 25, 26)');
	await visit(page, '/activity?theme=cozy&mock=poc');
	// Phase 8 (1149b4f1): Cozy Glass keeps matte #463c30 in the body so the Activity header stays translucent.
	await expect(page.locator('.activity-body')).toHaveCSS('background-color', 'rgb(70, 60, 48)');
	await expect(page.locator('#activity-release-panel')).toHaveCSS(
		'background-color',
		'rgb(53, 43, 34)'
	);

	// Phase 8 (1149b4f1): Activity arrow navigation moves focus from the focused tab without premature activation.
	const actTabs = page.locator('.activity-view-tabs');
	const assemblyBtn = actTabs.getByRole('button', { name: 'Assembly line', exact: true });
	const activityBtn = actTabs.getByRole('button', { name: 'Activity', exact: true });
	await expect(assemblyBtn).toHaveAttribute('aria-pressed', 'true');
	await activityBtn.focus();
	await page.keyboard.press('ArrowLeft');
	await expect(assemblyBtn).toBeFocused();
	await expect(assemblyBtn).toHaveAttribute('aria-pressed', 'true');

	// 3. Phase 8 (1149b4f1): Cozy Glass keeps Work header AND toolbar translucent above matte #463c30 results canvas; Inbox Escape preserves query
	await visit(page, '/work?theme=cozy&mock=poc');
	await expect(page.locator('.work-header')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
	await expect(page.locator('.work-toolbar-bar')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
	await expect(page.locator('.work-results-canvas')).toHaveCSS(
		'background-color',
		'rgb(70, 60, 48)'
	);

	await visit(page, '/inbox?theme=cozy&mock=poc');
	const inboxSearch = page.getByLabel('Search inbox');
	await page.locator('#shell-page-title').focus();
	await page.keyboard.press('/');
	await expect(inboxSearch).toBeFocused();
	await inboxSearch.fill('Custody');
	await inboxSearch.press('Escape');
	await expect(inboxSearch).toHaveValue('Custody');
	await expect(page.locator('#shell-page-title')).toBeFocused();
});
