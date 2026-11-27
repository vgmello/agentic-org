import { expect, test } from '@playwright/test';
import { visit } from './ready';

test(
	'PersonSidePanel desktop (>=960px): lazy-loads from Home lead click, tooltip-first Escape, and WebKit focus restore',
	{ tag: '@webkit' },
	async ({ page }) => {
		await page.setViewportSize({ width: 1440, height: 900 });
		await visit(page, '/home?mock=poc');

		// Desktop Home no longer renders the inline Home search bar; sidebar search handles ⌘K
		await expect(page.locator('header.exec-home-topbar')).toHaveCount(0);
		await expect(page.locator('#exec-cmdk-home-listbox')).toHaveCount(0);

		const leadLink = page.locator('a[data-person="rowan"]').first();
		await expect(leadLink).toBeVisible();

		// Open with a real pointer click (in WebKit, link click does not move document.activeElement beforehand)
		await leadLink.click();

		const panel = page.getByTestId('person-side-panel');
		await expect(panel).toBeVisible();
		await expect(panel).toHaveAttribute('data-mode', 'docked');
		await expect(panel.getByRole('heading', { level: 2, name: 'Rowan' })).toBeVisible();
		await expect(panel).toContainText('Agent');
		await expect(panel.getByRole('link', { name: 'Platform' })).toBeVisible();
		await expect(panel.getByRole('button', { name: 'Message' })).toBeVisible();
		await expect(panel.getByRole('link', { name: 'View full profile' })).toBeVisible();
		await expect(panel.getByRole('heading', { level: 3, name: 'Current work' })).toBeVisible();

		// Read-only summary: at most 3 open items listed and zero destructive/edit mutation controls
		await expect(panel.locator('.pp-list > li')).toHaveCount(
			await panel.locator('.pp-list > li').count()
		);
		expect(await panel.locator('.pp-list > li').count()).toBeLessThanOrEqual(3);
		await expect(
			panel.getByRole('button', { name: /Edit|Delete|Remove|Pause|Offboard/i })
		).toHaveCount(0);

		// Hover Close button -> ModalTooltip shows -> first Escape dismisses tooltip only -> second closes panel & restores focus
		const closeBtn = panel.getByTestId('person-panel-close');
		await closeBtn.hover();
		const closeTip = panel.getByRole('tooltip', { name: 'Close' });
		await expect(closeTip).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(closeTip).toBeHidden();
		await expect(panel).toBeVisible();

		await page.keyboard.press('Escape');
		await expect(panel).toHaveCount(0);
		await expect(leadLink).toBeFocused();
		expect(await page.evaluate(() => document.activeElement?.tagName)).not.toBe('BODY');

		// Reopen with pointer click and close via Close button -> restores focus to leadLink
		await leadLink.click();
		await expect(panel).toBeVisible();
		await panel.getByTestId('person-panel-close').click();
		await expect(panel).toHaveCount(0);
		await expect(leadLink).toBeFocused();

		// Open via keyboard Enter and follow "View full profile" -> navigates to /members/rowan and closes panel
		await page.keyboard.press('Enter');
		await expect(panel).toBeVisible();
		await panel.getByRole('link', { name: 'View full profile' }).click();
		await expect(page).toHaveURL(/\/members\/rowan$/);
		await expect(panel).toHaveCount(0);
	}
);

test(
	'PersonSidePanel narrow (<960px): modal Sheet, registerModalBack, and WebKit focus restore',
	{ tag: '@webkit' },
	async ({ page }) => {
		await page.setViewportSize({ width: 600, height: 900 });
		// Start on a same-origin route and navigate in-app to /home so browser Back is same-document
		await visit(page, '/preferences?mock=poc');
		await page.getByRole('button', { name: 'Menu', exact: true }).click();
		await page
			.getByRole('dialog', { name: 'Main navigation' })
			.getByRole('link', { name: 'Home', exact: true })
			.click();
		await expect(page).toHaveURL(/\/home$/);

		const leadLink = page.locator('a[data-person="rowan"]').first();
		await expect(leadLink).toBeVisible();

		// Open with real pointer click on narrow viewport -> modal Sheet
		await leadLink.click();
		const panel = page.getByTestId('person-side-panel');
		await expect(panel).toBeVisible();
		await expect(panel).toHaveAttribute('data-mode', 'modal');
		await expect(panel).toHaveAttribute('role', 'dialog');

		// Escape closes modal Sheet and restores focus to clicked lead link in WebKit
		await page.keyboard.press('Escape');
		await expect(panel).toBeHidden();
		await expect(leadLink).toBeFocused();
		expect(await page.evaluate(() => document.activeElement?.tagName)).not.toBe('BODY');

		// Browser Back dismisses modal via registerModalBack while staying on /home
		await leadLink.click();
		await expect(panel).toBeVisible();
		await page.evaluate(() => window.history.back());
		await expect(panel).toBeHidden();
		await expect(page).toHaveURL(/\/home$/);
		await expect(leadLink).toBeFocused();
	}
);

test(
	'PageHeader Title | subtitle divider (1px x 18px) and reduced motion on PersonSidePanel',
	{ tag: '@webkit' },
	async ({ page }) => {
		await page.emulateMedia({ reducedMotion: 'reduce' });
		await page.setViewportSize({ width: 1440, height: 900 });
		await visit(page, '/home?mock=poc');

		// Reduced motion disables PersonSidePanel enter animation
		await page.locator('a[data-person="rowan"]').first().click();
		const panel = page.getByTestId('person-side-panel');
		await expect(panel).toBeVisible();
		await expect(panel).toHaveCSS('animation-name', 'none');
		await page.keyboard.press('Escape');
		await expect(panel).toHaveCount(0);

		// PageHeader shows 1px x 18px inline divider on desktop and wraps cleanly without divider on <640px
		await visit(page, '/preferences');
		const sep = page.locator('header.page-header-lit .ph-sep');
		await expect(sep).toBeVisible();
		const sepDims = await sep.evaluate((el) => {
			const cs = getComputedStyle(el);
			return { width: cs.width, height: cs.height };
		});
		expect(sepDims).toEqual({ width: '1px', height: '18px' });

		await page.setViewportSize({ width: 390, height: 844 });
		await expect(sep).toBeHidden();
	}
);

test(
	'Offline Search controls expose accessible tooltips via aria-disabled and block palette open',
	{ tag: '@webkit' },
	async ({ page }) => {
		await page.setViewportSize({ width: 1440, height: 900 });
		await visit(page, '/home?mock=poc');

		const sidebarSearch = page.getByTestId('sidebar-search-trigger');
		await expect(sidebarSearch).toBeVisible();
		await page.evaluate(() => {
			(
				window as unknown as { __mockOrgLive?: (organizationId: string, live: boolean) => void }
			).__mockOrgLive?.('poc-agentic-org', false);
		});
		await expect(sidebarSearch).toHaveAttribute('aria-disabled', 'true');
		expect(await sidebarSearch.evaluate((el) => el.hasAttribute('disabled'))).toBe(false);
		await sidebarSearch.hover();
		await expect(
			page.getByRole('tooltip', { name: 'Search unavailable while offline' })
		).toBeVisible();
		await page.keyboard.press('Escape');
		await sidebarSearch.click({ force: true });
		await expect(page.locator('.exec-cmdk-dialog')).toHaveCount(0);

		await page.setViewportSize({ width: 390, height: 844 });
		const mobileSearch = page.getByTestId('mobile-search-trigger');
		await expect(mobileSearch).toBeVisible();
		await expect(mobileSearch).toHaveAttribute('aria-disabled', 'true');
		expect(await mobileSearch.evaluate((el) => el.hasAttribute('disabled'))).toBe(false);
		await mobileSearch.focus();
		await expect(
			page.getByRole('tooltip', { name: 'Search unavailable while offline' })
		).toBeVisible();
		await page.keyboard.press('Escape');
		await mobileSearch.click({ force: true });
		await expect(page.locator('.exec-cmdk-dialog')).toHaveCount(0);
	}
);
