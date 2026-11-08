import type { Page } from '@playwright/test';

/** A named screenshot, kept in test-results/screens/ (ignored by git, rewritten on every run). */
export const shot = (page: Page, name: string) =>
	page.screenshot({ path: `test-results/screens/${name}.png` });
