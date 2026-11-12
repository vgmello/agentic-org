import { test, type Locator, type Page } from '@playwright/test';

type ShotOptions = {
	/** Capture an in-progress state immediately, without waiting for finite transitions. */
	settle?: boolean;
	/** Also capture a particular panel or dialog region, scrolling it into view if needed. */
	context?: Locator;
};

/** Named full-page evidence, isolated by test/worker/retry under ignored test-results/. */
export async function shot(page: Page, name: string, options: ShotOptions = {}) {
	if (options.settle !== false) {
		// Finite running transitions settle; looping spinners and paused animations never finish.
		await page.evaluate(() => Promise.all(document.getAnimations()
			.filter((animation) => animation.playState === 'running' && Number.isFinite(animation.effect?.getComputedTiming().endTime))
			.map((animation) => animation.finished.catch(() => undefined))));
	}
	await page.screenshot({ path: test.info().outputPath(`${name}.png`), fullPage: true });
	if (options.context) {
		// fullPage does not expand a fixed dialog's internal scroll area. Callers choose the useful region.
		await options.context.screenshot({ path: test.info().outputPath(`${name}-context.png`) });
	}
}
