import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';

test('welcome screen has no automatically detectable WCAG A/AA violations', async ({ page }) => {
	await page.goto('/welcome');
	await expect(page.getByRole('link', { name: /Create organization/ })).toBeVisible();
	const scan = await new AxeBuilder({ page })
		.withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
		.analyze();
	expect(scan.violations).toEqual([]);
});
