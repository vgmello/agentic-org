import type { Page } from '@playwright/test';

/** A named screenshot, kept in test-results/screens/ (ignored by git, rewritten on every run). */
export async function shot(page: Page, name: string) {
	// Dialogs and menus fade in and out: wait until nothing is animating, so the picture shows the finished state.
	await page.evaluate(() => Promise.all(document.getAnimations().map((a) => a.finished.catch(() => undefined))));
	await page.screenshot({ path: `test-results/screens/${name}.png` });
}
