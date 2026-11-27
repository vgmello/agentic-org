import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, posix, relative, resolve } from 'node:path';
import { parse as parseSvelte } from 'svelte/compiler';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { isNonProductionModule } from './production-import-guard';

const UI_ROOT = resolve(import.meta.dirname, '../../../..');
const SRC_LIB = resolve(UI_ROOT, 'src/lib');
const LAYER_ALLOW_PATH = resolve(UI_ROOT, 'layer-exceptions.allow');
const FEATURES_ALLOW_PATH = resolve(UI_ROOT, 'cross-module-features.allow');

function collectSpecifiersFromTs(code: string, filename: string): string[] {
	const out: string[] = [];
	const sf = ts.createSourceFile(filename, code, ts.ScriptTarget.Latest, true);
	function visit(node: ts.Node) {
		if (
			(ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) &&
			node.moduleSpecifier &&
			ts.isStringLiteralLike(node.moduleSpecifier)
		) {
			out.push(node.moduleSpecifier.text);
		} else if (ts.isCallExpression(node)) {
			if (node.expression.kind === ts.SyntaxKind.ImportKeyword && node.arguments.length > 0) {
				const arg = node.arguments[0];
				if (ts.isStringLiteralLike(arg)) out.push(arg.text);
			} else if (
				ts.isPropertyAccessExpression(node.expression) &&
				node.expression.name.text === 'glob' &&
				ts.isMetaProperty(node.expression.expression)
			) {
				const arg = node.arguments[0];
				if (arg && ts.isStringLiteralLike(arg)) out.push(arg.text);
				else if (arg && ts.isArrayLiteralExpression(arg)) {
					for (const el of arg.elements) {
						if (ts.isStringLiteralLike(el)) out.push(el.text);
					}
				}
			}
		}
		ts.forEachChild(node, visit);
	}
	visit(sf);
	return out;
}

function extractSpecifiers(absPath: string): string[] {
	const raw = readFileSync(absPath, 'utf8');
	if (!absPath.endsWith('.svelte')) {
		return collectSpecifiersFromTs(raw, absPath);
	}
	const ast = parseSvelte(raw, { modern: true, filename: absPath });
	const specs: string[] = [];
	for (const script of [ast.instance, ast.module]) {
		if (!script) continue;
		const content = script.content as { start?: number; end?: number };
		if (typeof content.start === 'number' && typeof content.end === 'number') {
			specs.push(...collectSpecifiersFromTs(raw.slice(content.start, content.end), `${absPath}.ts`));
		}
	}
	return specs;
}

type LayerUnit =
	| { kind: 'components'; unit: 'components' }
	| { kind: 'shell'; unit: 'shell' }
	| { kind: 'platform'; unit: string }
	| { kind: 'module'; domain: string; unit: string };

function classifyPath(relFromSrc: string): LayerUnit | undefined {
	const mod = /^src\/lib\/modules\/([^/]+)\//.exec(relFromSrc);
	if (mod) return { kind: 'module', domain: mod[1], unit: `module:${mod[1]}` };
	if (relFromSrc.startsWith('src/lib/components/')) return { kind: 'components', unit: 'components' };
	if (relFromSrc.startsWith('src/lib/platform/shell/')) return { kind: 'shell', unit: 'shell' };
	const plat = /^src\/lib\/platform\/([^/]+)(?:\/|\.)/.exec(relFromSrc);
	if (plat) return { kind: 'platform', unit: `platform:${plat[1]}` };
	return undefined;
}

function resolveTargetRel(specifier: string, importerAbs: string): string | undefined {
	if (specifier.startsWith('$lib/')) {
		return posix.normalize(`src/lib/${specifier.slice('$lib/'.length)}`);
	}
	if (specifier.startsWith('.')) {
		const targetAbs = resolve(dirname(importerAbs), specifier);
		const relFromLib = relative(SRC_LIB, targetAbs).split('\\').join('/');
		if (!relFromLib.startsWith('..')) {
			return posix.normalize(`src/lib/${relFromLib}`);
		}
	}
	return undefined;
}

function findCyclicSccs(graph: Map<string, Set<string>>): string[][] {
	let index = 0;
	const indices = new Map<string, number>();
	const lowlink = new Map<string, number>();
	const onStack = new Set<string>();
	const stack: string[] = [];
	const sccs: string[][] = [];

	function strongConnect(v: string) {
		indices.set(v, index);
		lowlink.set(v, index);
		index++;
		stack.push(v);
		onStack.add(v);

		for (const w of graph.get(v) ?? []) {
			if (!indices.has(w)) {
				strongConnect(w);
				lowlink.set(v, Math.min(lowlink.get(v)!, lowlink.get(w)!));
			} else if (onStack.has(w)) {
				lowlink.set(v, Math.min(lowlink.get(v)!, indices.get(w)!));
			}
		}

		if (lowlink.get(v) === indices.get(v)) {
			const comp: string[] = [];
			while (true) {
				const w = stack.pop()!;
				onStack.delete(w);
				comp.push(w);
				if (w === v) break;
			}
			if (comp.length > 1) sccs.push(comp.sort());
		}
	}

	for (const node of [...graph.keys()].sort()) {
		if (!indices.has(node)) strongConnect(node);
	}
	return sccs.sort((a, b) => a.join(',').localeCompare(b.join(',')));
}

function parseEdgeAllowlist(path: string): { entries: Set<string>; errors: string[] } {
	const entries = new Set<string>();
	const errors: string[] = [];
	if (!existsSync(path)) {
		errors.push(`Missing allowlist file: ${relative(UI_ROOT, path)}`);
		return { entries, errors };
	}
	const lines = readFileSync(path, 'utf8').split('\n');
	let prev = '';
	for (let i = 0; i < lines.length; i++) {
		const line = lines[i].trim();
		if (!line || line.startsWith('#')) continue;
		if (entries.has(line)) errors.push(`${relative(UI_ROOT, path)}:${i + 1}: duplicate "${line}"`);
		if (prev && line < prev) errors.push(`${relative(UI_ROOT, path)}:${i + 1}: unsorted "${line}"`);
		prev = line;
		entries.add(line);
	}
	return { entries, errors };
}

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
	it('enforces zero cycles across modules/platform/components, strict layer boundaries, and shrink-only feature exceptions', () => {
		const tracked = execFileSync('git', ['ls-files', '-z', '--', 'src/lib'], {
			cwd: UI_ROOT,
			encoding: 'utf8'
		})
			.split('\0')
			.filter((rel) => rel.length > 0 && /\.(?:svelte|ts|js)$/.test(rel))
			.filter((rel) => !isNonProductionModule(`/${rel}`))
			.filter((rel) => existsSync(resolve(UI_ROOT, rel)))
			.sort();

		const graph = new Map<string, Set<string>>();
		const hardLayerErrors: string[] = [];
		const actualComponentToShell = new Set<string>();
		const actualCrossModuleFeatures = new Set<string>();

		for (const rel of tracked) {
			const norm = posix.normalize(rel.split('\\').join('/'));
			const from = classifyPath(norm);
			if (!from) continue;

			if (from.kind !== 'shell' && !graph.has(from.unit)) {
				graph.set(from.unit, new Set());
			}

			const abs = resolve(UI_ROOT, rel);
			for (const spec of extractSpecifiers(abs)) {
				const targetRel = resolveTargetRel(spec, abs);
				if (!targetRel) continue;
				const to = classifyPath(targetRel);
				if (!to) continue;

				if (from.kind !== 'shell' && to.kind !== 'shell' && from.unit !== to.unit) {
					graph.get(from.unit)!.add(to.unit);
				}

				// Layer rule 1: base platform/* never imports modules/* or shell; non-Svelte platform/* never imports components
				if (from.kind === 'platform') {
					if (to.kind === 'module' || to.kind === 'shell') {
						hardLayerErrors.push(`${norm} -> ${targetRel} (base platform cannot import ${to.unit})`);
					} else if (to.kind === 'components' && !norm.endsWith('.svelte')) {
						hardLayerErrors.push(`${norm} -> ${targetRel} (non-UI platform module cannot import components)`);
					}
				}

				// Layer rule 2: components never imports modules/*; components -> shell is shrink-only legacy debt
				if (from.kind === 'components') {
					if (to.kind === 'module') {
						hardLayerErrors.push(`${norm} -> ${targetRel} (shared component cannot import ${to.unit})`);
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
			console.log('=== layer-exceptions.allow ===\n' + [...actualComponentToShell].sort().join('\n'));
			console.log(
				'=== cross-module-features.allow ===\n' + [...actualCrossModuleFeatures].sort().join('\n')
			);
		}

		const actualCycles = findCyclicSccs(graph).map((scc) => scc.join(','));
		expect(actualCycles).toEqual([]);
		expect(hardLayerErrors).toEqual([]);

		const layerAllow = parseEdgeAllowlist(LAYER_ALLOW_PATH);
		const featAllow = parseEdgeAllowlist(FEATURES_ALLOW_PATH);
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
