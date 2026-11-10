import type { Page } from '@playwright/test';

/** Go to a page and wait until it is interactive (the app marks <html data-hydrated> once it has started). */
export async function visit(page: Page, url: string) {
	await page.goto(url);
	await page.locator('html[data-hydrated="true"]').waitFor();
}
