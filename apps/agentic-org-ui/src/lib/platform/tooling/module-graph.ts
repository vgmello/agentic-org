import { existsSync, readFileSync } from 'node:fs';
import { dirname, posix, relative, resolve } from 'node:path';
import { parse as parseSvelte } from 'svelte/compiler';
import ts from 'typescript';

export type LayerUnit =
	| { kind: 'components'; unit: 'components' }
	| { kind: 'shell'; unit: 'shell' }
	| { kind: 'platform'; unit: string }
	| { kind: 'lib-root'; unit: string }
	| { kind: 'module'; domain: string; unit: string };

function collectGlobSpecifiers(arg: ts.Expression | undefined, out: string[]): void {
	if (!arg) return;
	if (ts.isStringLiteralLike(arg)) {
		out.push(arg.text);
		return;
	}
	if (!ts.isArrayLiteralExpression(arg)) return;
	for (const el of arg.elements) {
		if (ts.isStringLiteralLike(el)) out.push(el.text);
	}
}

function collectCallSpecifiers(node: ts.CallExpression, out: string[]): void {
	if (node.expression.kind === ts.SyntaxKind.ImportKeyword) {
		const arg = node.arguments[0];
		if (arg && ts.isStringLiteralLike(arg)) out.push(arg.text);
		return;
	}
	if (
		ts.isPropertyAccessExpression(node.expression) &&
		node.expression.name.text === 'glob' &&
		ts.isMetaProperty(node.expression.expression)
	) {
		collectGlobSpecifiers(node.arguments[0], out);
	}
}

export function collectSpecifiersFromTs(code: string, filename: string): string[] {
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
			collectCallSpecifiers(node, out);
		}
		ts.forEachChild(node, visit);
	}
	visit(sf);
	return out;
}

export function extractSpecifiersFromSource(source: string, filename: string): string[] {
	if (!filename.endsWith('.svelte')) {
		return collectSpecifiersFromTs(source, filename);
	}
	const ast = parseSvelte(source, { modern: true, filename });
	const specs: string[] = [];
	for (const script of [ast.instance, ast.module]) {
		if (!script) continue;
		const content = script.content as { start?: number; end?: number };
		if (typeof content.start === 'number' && typeof content.end === 'number') {
			specs.push(
				...collectSpecifiersFromTs(source.slice(content.start, content.end), `${filename}.ts`)
			);
		}
	}
	return specs;
}

export function extractSpecifiers(absPath: string): string[] {
	return extractSpecifiersFromSource(readFileSync(absPath, 'utf8'), absPath);
}

function stripModuleExt(name: string): string {
	return name.replace(/(?:\.svelte)?\.(?:[cm]?[jt]s|svelte)$/, '');
}

export function classifyPath(relFromSrc: string): LayerUnit | undefined {
	const mod = /^src\/lib\/modules\/([^/]+)(?:\/|$)/.exec(relFromSrc);
	if (mod) return { kind: 'module', domain: mod[1], unit: `module:${mod[1]}` };
	if (relFromSrc === 'src/lib/components' || relFromSrc.startsWith('src/lib/components/')) {
		return { kind: 'components', unit: 'components' };
	}
	if (relFromSrc === 'src/lib/platform/shell' || relFromSrc.startsWith('src/lib/platform/shell/')) {
		return { kind: 'shell', unit: 'shell' };
	}
	const plat = /^src\/lib\/platform\/([^/.]+)(?:\/|\.|$)/.exec(relFromSrc);
	if (plat) return { kind: 'platform', unit: `platform:${plat[1]}` };
	const root = /^src\/lib\/([^/]+)$/.exec(relFromSrc);
	if (root) {
		const stem = stripModuleExt(root[1]);
		if (stem) return { kind: 'lib-root', unit: `lib-root:${stem}` };
	}
	return undefined;
}

export function resolveTargetRel(
	specifier: string,
	importerAbs: string,
	srcLibRoot: string
): string | undefined {
	if (specifier.startsWith('$lib/')) {
		return posix.normalize(`src/lib/${specifier.slice('$lib/'.length)}`);
	}
	if (specifier.startsWith('.')) {
		const targetAbs = resolve(dirname(importerAbs), specifier);
		const relFromLib = relative(srcLibRoot, targetAbs).split('\\').join('/');
		if (!relFromLib.startsWith('..')) {
			return posix.normalize(`src/lib/${relFromLib}`);
		}
	}
	return undefined;
}

export function findCyclicSccs(graph: Map<string, Set<string>>): string[][] {
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

export function parseEdgeAllowlist(
	path: string,
	uiRoot: string
): { entries: Set<string>; errors: string[] } {
	const entries = new Set<string>();
	const errors: string[] = [];
	if (!existsSync(path)) {
		errors.push(`Missing allowlist file: ${relative(uiRoot, path)}`);
		return { entries, errors };
	}
	const lines = readFileSync(path, 'utf8').split('\n');
	let prev = '';
	for (let i = 0; i < lines.length; i++) {
		const line = lines[i].trim();
		if (!line || line.startsWith('#')) continue;
		if (entries.has(line)) errors.push(`${relative(uiRoot, path)}:${i + 1}: duplicate "${line}"`);
		if (prev && line < prev) errors.push(`${relative(uiRoot, path)}:${i + 1}: unsorted "${line}"`);
		prev = line;
		entries.add(line);
	}
	return { entries, errors };
}
