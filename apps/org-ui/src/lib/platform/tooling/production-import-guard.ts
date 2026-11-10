import type { Plugin } from 'vite';

/** Temporary mixed factory roots from the removal inventory; retire entries as fakes leave them. */
const mixedFactories = /\/(?:org|organization|work|chat|messaging)\/api\.[cm]?[jt]s$/;
const fixtureModule = /\/(?:__mocks__|mocks?|fakes?|samples?|fixtures?|testkit|testing)(?:[A-Z./-]|$)/;
const testModule = /\.(?:test|spec)\.[cm]?[jt]s$/;
const fakeCall = /\b(?:createMock[\w$]*|inTauri)\s*\(/;

/** Report the resolved build graph, including aliases/dynamic imports. No dev/test runtime toggle. */
export function productionImportGuard({ mode = 'report' }: { mode?: 'report' | 'error' } = {}): Plugin {
	let root = '';
	const sourceLeaks = new Set<string>();
	const normalize = (id: string) => id.split('?')[0].replaceAll('\\', '/');
	function localPath(id: string): string | undefined {
		if (id.includes('\0')) return;
		const path = normalize(id);
		if (!path.startsWith(`${root}/`)) return;
		const local = path.slice(root.length + 1);
		if (local.startsWith('node_modules/') || local.includes('/node_modules/') || local.startsWith('.svelte-kit/')) return;
		return local;
	}
	return {
		name: 'org-production-import-guard',
		apply: 'build',
		configResolved(config) { root = config.root.replaceAll('\\', '/').replace(/\/$/, ''); },
		transform(code, id) {
			if (localPath(id) && fakeCall.test(code)) sourceLeaks.add(normalize(id));
		},
		generateBundle() {
			const leaks = new Set<string>();
			for (const id of this.getModuleIds()) {
				const local = localPath(id);
				if (!local) continue;
				if (!local.startsWith('src/') || fixtureModule.test(`/${local}`) || testModule.test(local) ||
					mixedFactories.test(`/${local}`) || /\/processes\/world\.ts$/.test(local) || sourceLeaks.has(normalize(id)))
					leaks.add(local);
			}
			if (!leaks.size) return;
			const message = `Production fake/sample imports (${mode} mode):\n${[...leaks].sort().map((path) => `- ${path}`).join('\n')}`;
			if (mode === 'error') this.error(message);
			else this.warn(message);
		}
	};
}
