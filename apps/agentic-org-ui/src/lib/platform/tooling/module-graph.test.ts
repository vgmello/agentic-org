import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, posix, relative, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { isNonProductionModule } from './production-import-guard';

const UI_ROOT = resolve(import.meta.dirname, '../../../..');
const SRC_LIB = resolve(UI_ROOT, 'src/lib');

const COMMENT_RE = /\/\*[\s\S]*?\*\/|(?<![:\\])\/\/[^\n]*/g;
const IMPORT_RE =
	/\b(?:import|export)\s+(?:type\s+)?(?:[^;'"]*?\s+from\s+)?['"]([^'"]+)['"]|\bimport\(\s*['"]([^'"]+)['"]\s*\)/g;

/**
 * Source dependency cycles between domain modules.
 * Cleared to empty in Wave 2 (TASK-227); zero cycles are permitted.
 */
const KNOWN_CYCLES: readonly string[] = [];

function domainOf(relFromSrc: string): string | undefined {
	const mod = /^src\/lib\/modules\/([^/]+)\//.exec(relFromSrc);
	if (mod) return mod[1];
	if (relFromSrc.startsWith('src/lib/home/')) return 'home';
	return undefined;
}

function targetDomainOf(specifier: string, importerAbs: string): string | undefined {
	if (specifier.startsWith('$lib/modules/')) {
		return specifier.slice('$lib/modules/'.length).split('/')[0];
	}
	if (specifier === '$lib/home' || specifier.startsWith('$lib/home/')) {
		return 'home';
	}
	if (specifier.startsWith('.')) {
		const targetAbs = resolve(dirname(importerAbs), specifier);
		const relFromLib = relative(SRC_LIB, targetAbs).split('\\').join('/');
		if (relFromLib.startsWith('modules/')) {
			return relFromLib.slice('modules/'.length).split('/')[0];
		}
		if (relFromLib === 'home' || relFromLib.startsWith('home/')) {
			return 'home';
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

describe('UI domain module dependency graph (doc-8, TASK-225)', () => {
	it('allows no new source dependency cycles and requires legacy cycle entries to shrink', () => {
		const tracked = execFileSync('git', ['ls-files', '-z', '--', 'src/lib'], {
			cwd: UI_ROOT,
			encoding: 'utf8'
		})
			.split('\0')
			.filter((rel) => rel.length > 0 && /\.(?:svelte|ts|js)$/.test(rel))
			.filter((rel) => !isNonProductionModule(`/${rel}`))
			.filter((rel) => existsSync(resolve(UI_ROOT, rel)));

		const graph = new Map<string, Set<string>>();
		for (const rel of tracked) {
			const norm = posix.normalize(rel.split('\\').join('/'));
			const fromDomain = domainOf(norm);
			if (!fromDomain) continue;
			if (!graph.has(fromDomain)) graph.set(fromDomain, new Set());

			const abs = resolve(UI_ROOT, rel);
			const source = readFileSync(abs, 'utf8').replace(COMMENT_RE, '');
			IMPORT_RE.lastIndex = 0;
			let match: RegExpExecArray | null;
			while ((match = IMPORT_RE.exec(source)) !== null) {
				const spec = match[1] ?? match[2];
				if (!spec) continue;
				const toDomain = targetDomainOf(spec, abs);
				if (toDomain && toDomain !== fromDomain) {
					graph.get(fromDomain)!.add(toDomain);
				}
			}
		}

		const actualCycles = findCyclicSccs(graph).map((scc) => scc.join(','));
		expect(actualCycles).toEqual(KNOWN_CYCLES);
	});
});
