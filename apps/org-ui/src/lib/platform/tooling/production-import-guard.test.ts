import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { build, createLogger } from 'vite';
import { describe, expect, it } from 'vitest';
import { productionImportGuard } from './production-import-guard';

async function fixture(mode: 'report' | 'error', leak: boolean) {
	const root = await mkdtemp(join(tmpdir(), 'org-import-guard-'));
	const warnings: string[] = [];
	try {
		await mkdir(join(root, 'src/lib'), { recursive: true });
		await writeFile(join(root, 'src/lib/mock.ts'), 'export const sample = "invented";');
		await writeFile(join(root, 'src/entry.ts'), leak
			? 'export async function load() { return (await import("@fixture")).sample; }'
			: 'export const real = "no test implementation";');
		const logger = createLogger('silent');
		logger.warn = (message) => { warnings.push(message); };
		await build({
			root, configFile: false, customLogger: logger,
			resolve: { alias: { '@fixture': join(root, 'src/lib/mock.ts') } },
			plugins: [productionImportGuard({ mode })],
			build: { write: false, minify: false, lib: { entry: join(root, 'src/entry.ts'), formats: ['es'] } }
		});
		return warnings;
	} finally { await rm(root, { recursive: true, force: true }); }
}

describe('resolved production imports', () => {
	it('reports an aliased dynamic fake import while the staged migration remains buildable', async () => {
		const warnings = await fixture('report', true);
		expect(warnings.join('\n')).toContain('Production fake/sample imports (report mode)');
		expect(warnings.join('\n')).toContain('src/lib/mock.ts');
	});
	it('can fail the real build on that same import when the final cutover enables enforcement', async () => {
		await expect(fixture('error', true)).rejects.toThrow('src/lib/mock.ts');
	});
	it('allows a clean production graph without a warning', async () => {
		expect((await fixture('report', false)).join('\n')).not.toContain('Production fake/sample imports');
	});
});
