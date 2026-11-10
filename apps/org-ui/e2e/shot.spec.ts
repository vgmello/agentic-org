import { expect, test } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { shot } from './shot';

test('named screenshots include content below the fold and settle without waiting for an infinite animation', async ({ page }) => {
	test.setTimeout(15_000);
	await page.setContent(`
		<style>
			@keyframes pulse { to { opacity: .5; } }
			#loading { height: 80px; animation: pulse 1s infinite alternate; }
			main { height: 1800px; }
		</style>
		<div id="loading">Loading</div><main>Content below the fold</main>
	`);
	await shot(page, 'shared-state', { context: page.locator('#loading') });
	const fullPage = await readFile(test.info().outputPath('shared-state.png'));
	// PNG's IHDR height verifies the actual capture includes more than the viewport.
	expect(fullPage.readUInt32BE(20)).toBeGreaterThan(page.viewportSize()!.height);
	const context = await readFile(test.info().outputPath('shared-state-context.png'));
	expect(context.readUInt32BE(20)).toBe(80);
});

test('loading screenshots preserve an in-progress finite animation without waiting for it', async ({ page }) => {
	test.setTimeout(15_000);
	await page.setContent(`
		<style>@keyframes pulse { to { opacity: .5; } } #loading { animation: pulse 30s; }</style>
		<div id="loading">Still loading</div>
	`);
	await shot(page, 'shared-state', { settle: false });
	expect(await page.locator('#loading').evaluate((element) => element.getAnimations().some((animation) => animation.playState === 'running'))).toBe(true);
	expect((await readFile(test.info().outputPath('shared-state.png'))).byteLength).toBeGreaterThan(0);
});
