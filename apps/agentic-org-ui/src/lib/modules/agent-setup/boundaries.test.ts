// Agent setup's place in the module graph (docs/architecture-rules.md: no cycles between declared source boundaries).
// Members links to an agent's setup through agent-setup's public index, so agent-setup must not depend, directly or
// through another module, on anything that depends on members: the route composes what it needs from elsewhere.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const MODULES = join(process.cwd(), 'src/lib/modules');

function filesUnder(dir: string): string[] {
	return readdirSync(dir).flatMap((name) => {
		const path = join(dir, name);
		if (statSync(path).isDirectory()) return filesUnder(path);
		return /\.(ts|svelte)$/.test(name) && !/\.test\.ts$/.test(name) ? [path] : [];
	});
}

/** Each module's direct dependencies on other modules, type-only imports included: a source edge is an edge. */
function moduleGraph(): Map<string, Set<string>> {
	const graph = new Map<string, Set<string>>();
	for (const module of readdirSync(MODULES)) {
		if (!statSync(join(MODULES, module)).isDirectory()) continue;
		const deps = new Set<string>();
		for (const file of filesUnder(join(MODULES, module)))
			for (const [, target] of readFileSync(file, 'utf8').matchAll(
				/from\s+['"]\$lib\/modules\/([a-z0-9-]+)/g
			))
				if (target !== module) deps.add(target);
		graph.set(module, deps);
	}
	return graph;
}

/** A path from `from` back to itself, if there is one. */
function cycleThrough(graph: Map<string, Set<string>>, from: string): string[] | undefined {
	const seen = new Set<string>();
	const walk = (node: string, path: string[]): string[] | undefined => {
		for (const next of graph.get(node) ?? []) {
			if (next === from) return [...path, next];
			if (seen.has(next)) continue;
			seen.add(next);
			const found = walk(next, [...path, next]);
			if (found) return found;
		}
		return undefined;
	};
	return walk(from, [from]);
}

describe('agent-setup module boundaries', () => {
	it('is on no dependency cycle between modules', () => {
		const graph = moduleGraph();
		expect(graph.has('agent-setup')).toBe(true);
		expect(cycleThrough(graph, 'agent-setup')).toBeUndefined();
	});

	it('is depended on by the member profile, through its public index only', () => {
		const graph = moduleGraph();
		expect(graph.get('members')?.has('agent-setup')).toBe(true);
		const internal = filesUnder(join(MODULES, 'members')).filter((f) =>
			/\$lib\/modules\/agent-setup\//.test(readFileSync(f, 'utf8'))
		);
		expect(internal).toEqual([]);
	});
});
