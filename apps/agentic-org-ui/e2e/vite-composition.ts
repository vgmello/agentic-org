import { existsSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, isAbsolute, resolve } from 'node:path';
import type { Plugin } from 'vite';
import { sessionTestMiddleware } from './fakes/session-server.ts';

/** Resolve test APIs after SvelteKit expands $lib, without shipping a runtime fake toggle. */
export function testComposition(): Plugin {
	const app = fileURLToPath(new URL('../', import.meta.url));
	const fakes = resolve(app, 'e2e/fakes');
	const replacements = new Map(
		[
			['platform/shell/capabilities', 'capabilities'],
			['platform/http/attachments-runtime', 'attachments-runtime'],
			['modules/offices/office-api', 'office-api'],
			['modules/organization/api', 'org-api'],
			['modules/work', 'work-api'],
			['modules/work/index', 'work-api']
		].map(([source, target]) => [resolve(app, `src/lib/${source}`), resolve(fakes, `${target}.ts`)])
	);
	// A module's mock API is found by name: e2e/fakes/modules/<module>.ts stands in for src/lib/modules/<module>/api.ts.
	const moduleFakes = resolve(fakes, 'modules');
	if (existsSync(moduleFakes))
		for (const file of readdirSync(moduleFakes))
			if (file.endsWith('.ts') && !file.endsWith('.test.ts'))
				replacements.set(
					resolve(app, 'src/lib/modules', file.slice(0, -3), 'api'),
					resolve(moduleFakes, file)
				);
	return {
		name: 'org-test-owned-composition',
		enforce: 'pre',
		config: () => ({ server: { fs: { allow: [resolve(app, 'e2e/session-browser-harness.ts')] } } }),
		configureServer(server) {
			server.middlewares.use(sessionTestMiddleware());
		},
		resolveId(source, importer) {
			// Test modules explicitly import real factories/types by relative path. Their $lib API
			// imports still compose the other test APIs, rather than creating separate instances.
			if (importer?.startsWith(`${fakes}/`) && source.startsWith('.')) return null;
			let candidate: string;
			if (source.startsWith('$lib/')) candidate = resolve(app, 'src/lib', source.slice(5));
			else if (source.startsWith('/src/')) candidate = resolve(app, `.${source}`);
			else if (isAbsolute(source)) candidate = source;
			else if (source.startsWith('.') && importer)
				candidate = resolve(dirname(importer.split('?')[0]), source);
			else return null;
			return replacements.get(candidate.split('?')[0].replace(/\.[jt]s$/, '')) ?? null;
		}
	};
}
