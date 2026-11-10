import type { Plugin } from 'vite';

/** Temporary mixed factory roots from the removal inventory; retire entries as fakes leave them. */
const mixedFactories = /\/(?:org|organization|work|chat|messaging)\/api\.[cm]?[jt]s$/;
const fixtureModule = /\/(?:mock|fake|sample|testkit)(?:[A-Z./-]|$)/;

/** Report the resolved build graph, including aliases/dynamic imports. No dev/test runtime toggle. */
export function productionImportGuard({ mode = 'report' }: { mode?: 'report' | 'error' } = {}): Plugin {
	let root = '';
	return {
		name: 'org-production-import-guard',
		apply: 'build',
		configResolved(config) { root = config.root.replaceAll('\\', '/').replace(/\/$/, ''); },
		generateBundle() {
			const leaks = new Set<string>();
			for (const id of this.getModuleIds()) {
				const path = id.split('?')[0].replaceAll('\\', '/');
				if (!path.startsWith(`${root}/src/`)) continue;
				const local = path.slice(root.length + 1);
				if (fixtureModule.test(`/${local}`) || mixedFactories.test(`/${local}`) || /\/(?:processes)\/world\.ts$/.test(local))
					leaks.add(local);
			}
			if (!leaks.size) return;
			const message = `Production fake/sample imports (${mode} mode):\n${[...leaks].sort().map((path) => `- ${path}`).join('\n')}`;
			if (mode === 'error') this.error(message);
			else this.warn(message);
		}
	};
}
