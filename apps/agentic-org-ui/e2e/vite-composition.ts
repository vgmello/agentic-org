import { fileURLToPath } from 'node:url';
import { dirname, isAbsolute, resolve } from 'node:path';
import type { Plugin } from 'vite';

/** Resolve test APIs after SvelteKit expands $lib, without shipping a runtime fake toggle. */
export function testComposition(): Plugin {
	const app = fileURLToPath(new URL('../', import.meta.url));
	const fakes = resolve(app, 'e2e/fakes');
	const replacements = new Map([
		['platform/shell/capabilities', 'capabilities'], ['office/api', 'office-api'],
		['org/api', 'org-api'], ['work', 'work-api']
	].map(([source, target]) => [resolve(app, `src/lib/${source}`), resolve(fakes, `${target}.ts`)]));
	return {
		name: 'org-test-owned-composition',
		enforce: 'pre',
		resolveId(source, importer) {
			// Test modules explicitly import real factories/types by relative path. Their $lib API
			// imports still compose the other test APIs, rather than creating separate instances.
			if (importer?.startsWith(`${fakes}/`) && source.startsWith('.')) return null;
			let candidate: string;
			if (source.startsWith('$lib/')) candidate = resolve(app, 'src/lib', source.slice(5));
			else if (source.startsWith('/src/')) candidate = resolve(app, `.${source}`);
			else if (isAbsolute(source)) candidate = source;
			else if (source.startsWith('.') && importer) candidate = resolve(dirname(importer.split('?')[0]), source);
			else return null;
			return replacements.get(candidate.split('?')[0].replace(/\.[jt]s$/, '')) ?? null;
		}
	};
}
