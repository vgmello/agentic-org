import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { posix, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import {
	classifyPath,
	extractSpecifiers,
	findCyclicSccs,
	parseEdgeAllowlist,
	resolveTargetRel
} from './module-graph';
import { isNonProductionModule } from './production-import-guard';

const UI_ROOT = resolve(import.meta.dirname, '../../../..');
const SRC_LIB = resolve(UI_ROOT, 'src/lib');
const LAYER_ALLOW_PATH = resolve(UI_ROOT, 'layer-exceptions.allow');
const FEATURES_ALLOW_PATH = resolve(UI_ROOT, 'cross-module-features.allow');

function resolveBaseRef(): string | undefined {
	const explicit = process.env.UI_FILE_SIZE_RATCHET_BASE?.trim();
	if (explicit) return explicit;
	try {
		return execFileSync('git', ['merge-base', 'HEAD', 'origin/v1'], {
			cwd: UI_ROOT,
			encoding: 'utf8',
			stdio: ['ignore', 'pipe', 'ignore']
		}).trim();
	} catch {
		return undefined;
	}
}

function checkAllowlistShrinkOnly(relPath: string, current: Set<string>): string[] {
	const baseRef = resolveBaseRef();
	if (!baseRef) return [];
	let baseText: string;
	try {
		baseText = execFileSync('git', ['show', `${baseRef}:apps/agentic-org-ui/${relPath}`], {
			cwd: UI_ROOT,
			encoding: 'utf8',
			stdio: ['ignore', 'pipe', 'ignore']
		});
	} catch {
		return [];
	}
	const base = new Set(
		baseText
			.split('\n')
			.map((l) => l.trim())
			.filter((l) => l && !l.startsWith('#'))
	);
	const errors: string[] = [];
	for (const entry of current) {
		if (!base.has(entry)) {
			errors.push(`${relPath}: added new exception "${entry}" (allowlist may only shrink)`);
		}
	}
	return errors;
}

describe('UI layered module dependency graph (AST-parsed, zero cycles)', () => {
	it('enforces zero cycles across modules/platform/components/lib-root, strict layer boundaries, and shrink-only feature exceptions', () => {
		const files = execFileSync(
			'git',
			['ls-files', '--cached', '--others', '--exclude-standard', '-z', '--', 'src/lib'],
			{
				cwd: UI_ROOT,
				encoding: 'utf8'
			}
		)
			.split('\0')
			.filter((rel) => rel.length > 0 && /\.(?:svelte|ts|js)$/.test(rel))
			.filter((rel) => !isNonProductionModule(`/${rel}`))
			.filter((rel) => existsSync(resolve(UI_ROOT, rel)))
			.sort();

		const graph = new Map<string, Set<string>>();
		const hardLayerErrors: string[] = [];
		const actualComponentToShell = new Set<string>();
		const actualCrossModuleFeatures = new Set<string>();

		for (const rel of files) {
			const norm = posix.normalize(rel.split('\\').join('/'));
			const from = classifyPath(norm);
			if (!from) continue;

			if (from.kind !== 'shell' && !graph.has(from.unit)) {
				graph.set(from.unit, new Set());
			}

			const abs = resolve(UI_ROOT, rel);
			for (const spec of extractSpecifiers(abs)) {
				const targetRel = resolveTargetRel(spec, abs, SRC_LIB);
				if (!targetRel) continue;
				const to = classifyPath(targetRel);
				if (!to) continue;

				if (from.kind !== 'shell' && to.kind !== 'shell' && from.unit !== to.unit) {
					graph.get(from.unit)!.add(to.unit);
				}

				// Layer rule 1: base platform/* never imports modules/* or shell; non-Svelte platform/* never imports components
				if (from.kind === 'platform') {
					if (to.kind === 'module' || to.kind === 'shell') {
						hardLayerErrors.push(
							`${norm} -> ${targetRel} (base platform cannot import ${to.unit})`
						);
					} else if (to.kind === 'components' && !norm.endsWith('.svelte')) {
						hardLayerErrors.push(
							`${norm} -> ${targetRel} (non-UI platform module cannot import components)`
						);
					}
				}

				// Layer rule 2: components never imports modules/*; components -> shell is shrink-only legacy debt
				if (from.kind === 'components') {
					if (to.kind === 'module') {
						hardLayerErrors.push(
							`${norm} -> ${targetRel} (shared component cannot import ${to.unit})`
						);
					} else if (to.kind === 'shell') {
						actualComponentToShell.add(`${norm} -> ${targetRel}`);
					}
				}

				// Layer rule 3: cross-module /features/ imports are locked shrink-only
				if (
					from.kind === 'module' &&
					to.kind === 'module' &&
					from.domain !== to.domain &&
					targetRel.includes(`/modules/${to.domain}/features/`)
				) {
					actualCrossModuleFeatures.add(`${norm} -> ${targetRel}`);
				}
			}
		}

		if (process.env.UI_MODULE_GRAPH_PRINT === '1') {
			console.log(
				'=== layer-exceptions.allow ===\n' + [...actualComponentToShell].sort().join('\n')
			);
			console.log(
				'=== cross-module-features.allow ===\n' + [...actualCrossModuleFeatures].sort().join('\n')
			);
		}

		const actualCycles = findCyclicSccs(graph).map((scc) => scc.join(','));
		expect(actualCycles).toEqual([]);
		expect(hardLayerErrors).toEqual([]);

		const layerAllow = parseEdgeAllowlist(LAYER_ALLOW_PATH, UI_ROOT);
		const featAllow = parseEdgeAllowlist(FEATURES_ALLOW_PATH, UI_ROOT);
		const allowErrors = [
			...layerAllow.errors,
			...featAllow.errors,
			...checkAllowlistShrinkOnly('layer-exceptions.allow', layerAllow.entries),
			...checkAllowlistShrinkOnly('cross-module-features.allow', featAllow.entries)
		];

		for (const edge of actualComponentToShell) {
			if (!layerAllow.entries.has(edge)) {
				allowErrors.push(`New components -> shell edge forbidden: ${edge}`);
			}
		}
		for (const edge of layerAllow.entries) {
			if (!actualComponentToShell.has(edge)) {
				allowErrors.push(`Stale layer-exceptions.allow entry (delete it): ${edge}`);
			}
		}

		for (const edge of actualCrossModuleFeatures) {
			if (!featAllow.entries.has(edge)) {
				allowErrors.push(
					`New cross-module /features/ import forbidden (import via public index.ts or ui.ts): ${edge}`
				);
			}
		}
		for (const edge of featAllow.entries) {
			if (!actualCrossModuleFeatures.has(edge)) {
				allowErrors.push(`Stale cross-module-features.allow entry (delete it): ${edge}`);
			}
		}

		expect(allowErrors).toEqual([]);
	}, 20_000);
});
