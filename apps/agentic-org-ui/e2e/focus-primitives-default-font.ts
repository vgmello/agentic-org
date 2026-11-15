import { test } from '@playwright/test';
import { buttons, menu, consumerFocus } from './focus-primitives';
for (const width of [320, 640, 960, 1280]) for (const scale of [1, 2]) {
	test(`BUG-102: real20px default Button focus at ${width}/${scale * 100}`, async ({ page }) => { await buttons(page, width, scale, 20); });
	test(`BUG-101: real20px default menu focus at ${width}/${scale * 100}`, async ({ page }) => { await menu(page, width, scale, 20); });
	test(`BUG-101/102: real20px consumer families at ${width}/${scale * 100}`, async ({ page }) => { await consumerFocus(page, width, scale, 20); });
}
