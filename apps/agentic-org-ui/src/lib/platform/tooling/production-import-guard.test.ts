import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { build, createLogger } from 'vite';
import { describe, expect, it } from 'vitest';
import { productionImportGuard } from './production-import-guard';

async function fixture(mode: 'report' | 'error', leak: boolean, path = 'src/lib/mock.ts', code = 'export const sample = \"invented\";', entry?: string) {
	const root = await mkdtemp(join(tmpdir(), 'org-import-guard-'));
	const warnings: string[] = [];
	try {
		await mkdir(join(root, 'src/lib'), { recursive: true });
		await mkdir(join(root, path, '..'), { recursive: true });
		await writeFile(join(root, path), code);
		await writeFile(join(root, 'src/entry.ts'), entry ?? (leak
			? 'export async function load() { return (await import("@fixture")).sample; }'
			: 'export const real = "no test implementation";'));
		const logger = createLogger('silent');
		logger.warn = (message) => { warnings.push(message); };
		await build({
			root, configFile: false, customLogger: logger,
			resolve: { alias: { '@fixture': join(root, path) } },
			plugins: [productionImportGuard({ mode })],
			build: { write: false, minify: false, lib: { entry: join(root, 'src/entry.ts'), formats: ['es'] } }
		});
		return warnings;
	} finally { await rm(root, { recursive: true, force: true }); }
}

describe('resolved production imports', () => {
	it.each(['report', 'error'] as const)('BUG-46: allows root package version metadata in %s mode', async (mode) => {
		const warnings = await fixture(mode, true, 'package.json', '{"version":"1.2.3"}',
			'import pkg from "@fixture"; export const version = pkg.version;');
		expect(warnings.join('\n')).not.toContain('Production fake/sample imports');
	});
	it.each(['e2e/support/office.ts', 'test/helpers/office.ts'])(
		'BUG-46: still rejects ordinary outside-src modules at %s', async (path) => {
			await expect(fixture('error', true, path)).rejects.toThrow(path);
		}
	);
	it('BUG-46: does not exempt a nested package.json', async () => {
		await expect(fixture('error', true, 'config/package.json', '{"sample":"invented"}'))
			.rejects.toThrow('config/package.json');
	});
	it('reports an aliased dynamic fake import while the staged migration remains buildable', async () => {
		const warnings = await fixture('report', true);
		expect(warnings.join('\n')).toContain('Production fake/sample imports (report mode)');
		expect(warnings.join('\n')).toContain('src/lib/mock.ts');
	});
	it('can fail the real build on that same import when the final cutover enables enforcement', async () => {
		await expect(fixture('error', true)).rejects.toThrow('src/lib/mock.ts');
	});
	it.each([
		'e2e/fixtures/office.ts', 'test/fakes/office.ts', 'src/lib/testing/fakes.ts',
		'src/lib/office/mocks/index.ts', 'src/lib/office/fixtures.ts', 'src/lib/office/__mocks__/api.ts',
		'src/lib/office/samples.ts', 'src/lib/office/api.test.ts', 'src/lib/office/api.spec.ts'
	])('SRV-18: rejects a resolved aliased dynamic fake at its future location %s', async (path) => {
		await expect(fixture('error', true, path)).rejects.toThrow(path);
	});
	it.each(['src/lib/api.spec.tsx', 'src/lib/api.test.jsx', 'src/lib/widget.spec.svelte', 'src/lib/state.spec.svelte.ts', 'src/lib/state.test.svelte.js'])(
		'SRV-18: rejects resolved test modules with supported extension at %s', async (path) => {
			await expect(fixture('error', true, path)).rejects.toThrow('Production fake/sample imports');
		}
	);

	it.each(['node_modules/provider/index.js', '.svelte-kit/generated/entry.js'])(
		'allows framework and dependency modules at %s', async (path) => {
			expect((await fixture('report', true, path)).join('\n')).not.toContain('Production fake/sample imports');
		}
	);
	it.each(['createMockOffice', 'inTauri'])('reports production code that calls %s even in an ordinary file', async (name) => {
		await expect(fixture('error', true, 'src/lib/ordinary.ts', `function ${name}() { return 'invented'; } export const sample = ${name}();`)).rejects.toThrow('src/lib/ordinary.ts');
	});

	it.each([
		'const createMockOffice = () => 1; export const sample = createMockOffice?.();',
		'const inTauri = () => 1; export const sample = inTauri /* call */ ();',
		'const provider = { createMockOffice: () => 1 }; export const sample = provider?.createMockOffice?.();',
		'const provider = { inTauri: () => 1 }; export const sample = provider["inTauri"] /* call */ ();',
		'const createMockOffice = (): number => 1; export const sample = (createMockOffice as () => number)?.();'
	])('rejects forbidden call expressions independently of spelling: %s', async (code) => {
		await expect(fixture('error', true, 'src/lib/ordinary.ts', code)).rejects.toThrow('src/lib/ordinary.ts');
	});
	it.each([
		'export const sample = "inTauri(";',
		'// createMockOffice();\nexport const sample = "real";',
		'export const sample = `createMockOffice()`;',
		'function inTauri() { return true; } export const sample = typeof inTauri;'
	])('allows non-call source text and declarations: %s', async (code) => {
		expect(await fixture('error', true, 'src/lib/ordinary.ts', code)).not.toContain('Production fake/sample imports');
	});

	it('allows a clean production graph without a warning', async () => {
		expect((await fixture('report', false)).join('\n')).not.toContain('Production fake/sample imports');
	});
});
