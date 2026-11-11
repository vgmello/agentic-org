import { afterEach, describe, expect, it } from 'vitest';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { build, loadConfigFromFile } from 'vite';

const script = fileURLToPath(new URL('../../../scripts/check-build-assets.ts', import.meta.url));
const work: string[] = [];
afterEach(async () => { await Promise.all(work.splice(0).map((path) => rm(path, { recursive: true, force: true }))); });
async function fixture(files: Record<string, string>) {
	const root = await mkdtemp(join(tmpdir(), 'org-ui-csp-assets-'));
	work.push(root);
	for (const [name, text] of Object.entries(files)) {
		const target = join(root, name);
		await mkdir(join(target, '..'), { recursive: true });
		await writeFile(target, text);
	}
	return root;
}
function check(root: string) {
	const result = spawnSync('bun', [script, root], { encoding: 'utf8' });
	return { status: result.status, output: result.stdout + result.stderr };
}

describe('B1-05 production build data-URI gate', () => {
	it('refuses a missing build instead of skipping validation', async () => {
		const root = await fixture({});
		const result = check(join(root, 'missing-build'));
		expect(result.status).toBe(1);
		expect(result.output).toContain('ENOENT');
		expect(result.output).toContain('missing-build');
	});
	it('refuses a file in place of the build directory', async () => {
		const root = await fixture({ 'not-a-directory': 'text' });
		const result = check(join(root, 'not-a-directory'));
		expect(result.status).toBe(1);
		expect(result.output).toContain('ENOTDIR');
	});
	for (const value of ['url(data:image/svg+xml,a)', 'url( " data:image/png;base64,AA")', "URL( 'DATA:image/png,AA')"]) {
		it(`refuses built CSS data URL ${value}`, async () => {
			const root = await fixture({ 'nested/app.css': `body{background:${value}}`, 'app.js': 'console.log(1)' });
			const result = check(root);
			expect(result.status).toBe(1);
			expect(result.output).toContain('nested/app.css: CSS data URL');
		});
	}
	for (const value of ['"data:image/png;base64,AA"', "'data:text/plain,hello'", '`data:,empty`', '"DATA:application/font-woff2;base64,AA"']) {
		it(`refuses built JS data URI ${value}`, async () => {
			const root = await fixture({ 'nested/app.js': `const image=${value};`, 'app.css': 'body{color:white}' });
			const result = check(root);
			expect(result.status).toBe(1);
			expect(result.output).toContain('nested/app.js: JS data URI');
		});
	}
	it('accepts external assets and the SSE data prefix', async () => {
		const root = await fixture({ 'app.css': 'body{background:url(/image.svg)}', 'app.js': 'const isSse=(line)=>line.startsWith("data:");', 'font.woff2': '"data:font/woff2;base64,AA"' });
		const result = check(root);
		expect(result.status).toBe(0);
		expect(result.output).toContain('1 CSS and 1 JS');
	});
	it('keeps a tiny asset external using the actual application build settings', async () => {
		const config = await loadConfigFromFile({ command: 'build', mode: 'production' }, fileURLToPath(new URL('../../../vite.config.ts', import.meta.url)));
		expect(config).not.toBeNull();
		const root = await fixture({ 'index.html': '<script type="module" src="/entry.js"></script>', 'entry.js': 'import image from "./tiny.svg";document.body.textContent=image;', 'tiny.svg': '<svg xmlns="http://www.w3.org/2000/svg"/>' });
		await build({ configFile: false, root, publicDir: false, logLevel: 'silent', build: { ...config!.config.build, outDir: 'output' } });
		const result = check(join(root, 'output'));
		expect(result, result.output).toMatchObject({ status: 0 });
	}, 15_000);
});
