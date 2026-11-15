// Run under heavy: bun e2e/memory/probe.ts [--browser=chromium|firefox|webkit] [--footprint]
import { cpSync, mkdirSync, openSync, closeSync, writeFileSync, symlinkSync, rmSync } from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium, firefox, webkit, expect, type Browser, type CDPSession, type Page } from '@playwright/test';
import { portalCalibration } from './portal-calibration';

const args = process.argv.slice(2);
if (args.some(arg => !/^--browser=(chromium|firefox|webkit)$/.test(arg) && arg !== '--footprint')) throw new Error('Unknown argument');
const engine = args.find(arg => arg.startsWith('--browser='))?.split('=')[1] as 'chromium' | 'firefox' | 'webkit' | undefined ?? 'chromium';
const footprint = args.includes('--footprint');
if (footprint && (process.platform !== 'darwin' || engine === 'firefox')) throw new Error('Footprint requires macOS Chromium or WebKit');
const app = fileURLToPath(new URL('../../', import.meta.url));
const out = resolve(app, 'test-results/memory', new Date().toISOString().replace(/[:.]/g, '-'));
const sandbox = resolve(out, 'app');
mkdirSync(sandbox, { recursive: true });
for (const item of ['src', 'static', 'e2e', 'package.json', 'tsconfig.json', 'svelte.config.js']) cpSync(resolve(app, item), resolve(sandbox, item), { recursive: true });
writeFileSync(resolve(sandbox, '.memory-build-sandbox'), 'Test-owned build; never release.\n');
symlinkSync(resolve(app, 'node_modules'), resolve(sandbox, 'node_modules'), 'dir');
const buildLog = openSync(resolve(out, 'build.log'), 'w');
try {
	for (const argv of [['x', 'svelte-kit', 'sync'], ['x', 'vite', 'build', '--config', 'e2e/memory/vite.memory.config.ts']]) {
		const result = spawnSync('bun', argv, { cwd: sandbox, stdio: ['ignore', buildLog, buildLog] });
		if (result.status !== 0) throw new Error(`Memory build failed (${result.status}): ${out}/build.log`);
	}
} finally { closeSync(buildLog); }
const port = Number(process.env.PW_PORT) || 4860;
const url = `http://127.0.0.1:${port}`;
const previewLog = openSync(resolve(out, 'preview.log'), 'w');
const preview = spawn('bun', ['x', 'vite', 'preview', '--config', 'e2e/memory/vite.memory.config.ts', '--host', '127.0.0.1', '--port', String(port), '--strictPort'], { cwd: sandbox, detached: true, stdio: ['ignore', previewLog, previewLog] });
const results: Record<string, unknown>[] = [];
const failures: string[] = [];
const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));

function webContentPid() {
	const ps = spawnSync('ps', ['-axo', 'pid=,ppid=,command='], { encoding: 'utf8' });
	if (ps.status !== 0) throw new Error('Cannot inspect launched WebKit descendants');
	const processes = ps.stdout.split('\n').flatMap(line => { const m = line.trim().match(/^(\d+)\s+(\d+)\s+(.*)$/); return m ? [{ pid: +m[1], parent: +m[2], command: m[3] }] : []; });
	const ours = new Set([process.pid]);
	for (let before = -1; before !== ours.size;) { before = ours.size; for (const p of processes) if (ours.has(p.parent)) ours.add(p.pid); }
	const candidates = processes.filter(p => ours.has(p.pid) && p.command.includes('WebContent'));
	if (candidates.length !== 1) throw new Error(`Expected one launched WebContent process, found ${candidates.length}`);
	return candidates[0].pid;
}
async function measure(browser: Browser, page: Page, cdp?: CDPSession) {
	let heap: number | undefined, nodes: number | undefined, rendererPid: number | undefined;
	if (cdp) {
		for (let i = 0; i < 3; i++) await cdp.send('HeapProfiler.collectGarbage');
		heap = (await cdp.send('Runtime.getHeapUsage')).usedSize;
		nodes = (await cdp.send('Memory.getDOMCounters')).nodes;
		const browserCdp = await browser.newBrowserCDPSession();
		try {
			const renderers = (await browserCdp.send('SystemInfo.getProcessInfo')).processInfo.filter(p => p.type === 'renderer');
			if (renderers.length !== 1) throw new Error(`Expected one renderer, found ${renderers.length}`);
			rendererPid = renderers[0].id;
		} finally { await browserCdp.detach(); }
	} else if (footprint) rendererPid = webContentPid();
	const dom = await page.evaluate(() => ({
		rows: document.querySelectorAll('[data-testid="message"]').length,
		elements: document.querySelectorAll('*').length,
		bodyChildren: [...document.body.childNodes].map(node => ({ type: node.nodeType, name: node.nodeName, text: node.nodeType === Node.COMMENT_NODE ? node.textContent : undefined }))
	}));
	let footprintBytes: number | undefined;
	if (footprint) {
		const result = spawnSync('footprint', ['-p', String(rendererPid)], { encoding: 'utf8', timeout: 60_000 });
		if (result.status !== 0) throw new Error(`footprint failed for ${rendererPid}: ${result.stderr}`);
		const match = result.stdout.match(/Footprint:\s+([\d.]+)\s*(KB|MB|GB|B)/);
		if (!match) throw new Error('Unrecognized footprint output');
		footprintBytes = +match[1] * ({ B: 1, KB: 1024, MB: 1024 ** 2, GB: 1024 ** 3 }[match[2]]!);
	}
	return { ...dom, heap, nodes, rendererPid, footprintBytes };
}
async function portalNodes(page: Page) {
	return page.evaluate(() => {
		const roots = [...document.body.children].filter(node => node instanceof HTMLElement && node.style.display === 'contents');
		if (roots.length !== 1) throw new Error(`Expected one application root, found ${roots.length}`);
		// Text and comment anchors count too. Static whitespace cancels in the slope.
		return [...document.body.childNodes].filter(node => node !== roots[0]).length;
	});
}
async function settle(page: Page) {
	await page.evaluate(() => Promise.all(document.getAnimations()
		.filter(animation => animation.playState === 'running' && Number.isFinite(animation.effect?.getComputedTiming().endTime))
		.map(animation => animation.finished.catch(() => undefined))));
}
async function portalOracle(page: Page) {
	const variants: { rows: number; closed: number; afterClose: number }[] = [];
	for (const count of [1, 2, 1000]) {
		await page.evaluate(count => {
			localStorage.setItem('memory.exact', 'true');
			localStorage.setItem('memory.messages', String(count));
			localStorage.setItem('memory.conversation', 'c-dm-sam');
		}, count);
		await page.goto(`${url}/chat?c=c-dm-sam`);
		await expect(page.getByTestId('message')).toHaveCount(count);
		await expect(page.getByTestId('message-menu')).toHaveCount(count);
		await page.mouse.move(0, 0);
		await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Chat', exact: true }).focus();
		await expect(page.getByRole('menu')).toHaveCount(0);
		await settle(page);
		const closed = await portalNodes(page);
		const trigger = page.getByTestId('message-menu').last();
		await trigger.click();
		await expect(page.getByRole('menu')).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(page.getByRole('menu')).toHaveCount(0);
		await expect(trigger).toBeFocused();
		if (count === 1000) {
			const position = await page.getByTestId('message-list').evaluate(node => ({ top: node.scrollTop, height: node.clientHeight }));
			expect(position.top).toBeGreaterThan(3 * position.height);
			await expect(trigger).toBeInViewport();
		}
		await settle(page);
		await expect.poll(() => portalNodes(page)).toBe(closed);
		const afterClose = await portalNodes(page);
		variants.push({ rows: count, closed, afterClose });
		await page.screenshot({ path: resolve(out, `portals-${count}-closed.png`), fullPage: true });
	}
	expect(variants[2].closed - variants[0].closed).toBeLessThanOrEqual(portalCalibration.c_base);
	return variants;
}

try {
	const deadline = Date.now() + 30_000;
	while (true) {
		if (preview.exitCode !== null) throw new Error(`Preview exited: ${out}/preview.log`);
		try { if ((await fetch(url)).ok) break; } catch { /* Server has not bound yet. */ }
		if (Date.now() >= deadline) throw new Error('Memory preview did not become ready');
		await sleep(100);
	}
	for (const conversation of ['c-dm-sam', 'c-general']) {
		const browser = await ({ chromium, firefox, webkit }[engine]).launch({ args: engine === 'chromium' ? ['--disable-features=SpareRendererForSitePerProcess'] : [] });
		try {
			const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
			const errors: string[] = [];
			page.on('pageerror', error => errors.push(error.message));
			page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
			page.setDefaultTimeout(30_000);
			await page.goto(`${url}/create`);
			await page.locator('html[data-hydrated="true"]').waitFor();
			await page.getByLabel('Organization name').fill('Acme');
			await page.getByRole('button', { name: 'Create organization' }).click();
			await page.waitForURL(/\/agents$/);
			await page.evaluate(conversation => { localStorage.setItem('memory.messages', '1000'); localStorage.setItem('memory.conversation', conversation); }, conversation);
			await page.goto(`${url}/chat?c=c-dm-avery`);
			await expect(page.getByRole('navigation', { name: 'Chat' })).toBeVisible();
			const cdp = engine === 'chromium' ? await page.context().newCDPSession(page) : undefined;
			const idle = await measure(browser, page, cdp);
			await page.getByRole('navigation', { name: 'Chat' }).locator(`[data-conversation="${conversation}"]`).click();
			await expect(page.locator('[data-testid="message"][data-id="m-memory-999"]')).toBeAttached();
			await page.mouse.move(0, 0);
			const opened = await measure(browser, page, cdp);
			const shortNodes = await page.locator('[data-testid="message"][data-id="m-memory-2"]').evaluate(node => { const walker = document.createTreeWalker(node.parentElement!, NodeFilter.SHOW_ALL); let n = 1; while (walker.nextNode()) n++; return n; });
			const shortBreakdown = await page.locator('[data-testid="message"][data-id="m-memory-2"]').evaluate(node => { const counts: Record<string, number> = {}; const walker = document.createTreeWalker(node.parentElement!, NodeFilter.SHOW_ALL); for (let child = walker.currentNode; child; child = walker.nextNode()!) { const kind = child.nodeType === Node.TEXT_NODE ? child.textContent === '' ? 'emptyText' : child.textContent!.trim() === '' ? 'whitespace' : 'text' : child.nodeType === Node.COMMENT_NODE ? 'comment' : 'element'; counts[kind] = (counts[kind] ?? 0) + 1; } return counts; });
			const shortHtml = await page.locator('[data-testid="message"][data-id="m-memory-2"]').evaluate(node => node.parentElement!.outerHTML);
			const meanNodes = opened.nodes === undefined ? undefined : (opened.nodes - idle.nodes!) / (opened.rows - idle.rows);
			const list = page.getByTestId('message-list');
			await list.evaluate(node => node.scrollTop = 0);
			await page.screenshot({ path: resolve(out, `${conversation}-top.png`), fullPage: true });
			const top = await measure(browser, page, cdp);
			await list.evaluate(node => node.scrollTop = node.scrollHeight);
			await page.screenshot({ path: resolve(out, `${conversation}-bottom.png`), fullPage: true });
			const bottom = await measure(browser, page, cdp);
			const portals = conversation === 'c-dm-sam' ? await portalOracle(page) : undefined;
			results.push({ conversation, idle, opened, top, bottom, shortNodes, shortBreakdown, shortHtml, meanNodes, portals, errors });
			if (errors.length) failures.push(`${conversation}: page errors: ${errors.join('; ')}`);
			if (engine === 'chromium' && shortNodes > 60) failures.push(`${conversation}: short row ${shortNodes} >60 nodes`);
			if (engine === 'chromium' && !(meanNodes! <= 90)) failures.push(`${conversation}: mean ${meanNodes} >90 nodes`);
		} finally { await browser.close(); }
	}
} finally {
	if (preview.pid) try { process.kill(-preview.pid, 'SIGTERM'); } catch { /* Already exited. */ }
	closeSync(previewLog);
	writeFileSync(resolve(out, 'results.json'), JSON.stringify({ engine, results, failures }, null, 2) + '\n');
	// The sandbox contains copied source/dependencies; keep logs, JSON and screenshots only.
	rmSync(sandbox, { recursive: true, force: true });
}
console.log(JSON.stringify({ engine, output: out, counts: results.map(({ conversation, shortNodes, meanNodes }) => ({ conversation, shortNodes, meanNodes })), failures }));
process.exitCode = failures.length ? 1 : 0;
