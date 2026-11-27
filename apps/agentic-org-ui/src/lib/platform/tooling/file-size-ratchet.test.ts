import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { findDisallowedCssUsage } from './file-size-ratchet';
import { isNonProductionModule } from './production-import-guard';

const UI_ROOT = resolve(import.meta.dirname, '../../../..');
const ALLOW_PATH = resolve(UI_ROOT, 'large-files.allow');
const CAP = 700;
const NEW_FILE_CAP = 500;

function countLinesFromBuffer(buf: Buffer): number {
	if (buf.length === 0) return 0;
	let newlines = 0;
	for (let i = 0; i < buf.length; i++) {
		if (buf[i] === 10) newlines++;
	}
	return buf[buf.length - 1] === 10 ? newlines : newlines + 1;
}

function countLines(filePath: string): number {
	return countLinesFromBuffer(readFileSync(filePath));
}

function listProductionFiles(): string[] {
	const raw = execFileSync(
		'git',
		['ls-files', '--cached', '--others', '--exclude-standard', '-z', '--', 'src'],
		{
			cwd: UI_ROOT,
			encoding: 'utf8'
		}
	);
	return raw
		.split('\0')
		.filter((rel) => rel.length > 0 && /\.(?:svelte|ts|js|css)$/.test(rel))
		.filter((rel) => !isNonProductionModule(`/${rel}`))
		.filter((rel) => existsSync(resolve(UI_ROOT, rel)))
		.sort();
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

function readGitFile(ref: string, repoPath: string): Buffer | undefined {
	try {
		return execFileSync('git', ['show', `${ref}:${repoPath}`], {
			cwd: UI_ROOT,
			stdio: ['ignore', 'pipe', 'ignore']
		});
	} catch {
		return undefined;
	}
}

function gitRenamedPaths(baseRef: string): Map<string, string> {
	const renamedNewToOld = new Map<string, string>();
	try {
		const raw = execFileSync(
			'git',
			['diff', '--find-renames=50%', '--name-status', '-z', baseRef, '--', 'src'],
			{ cwd: UI_ROOT, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }
		);
		const tokens = raw.split('\0');
		for (let i = 0; i < tokens.length; i++) {
			const status = tokens[i];
			if (!status) continue;
			if (status.startsWith('R')) {
				const oldRepoPath = tokens[i + 1] ?? '';
				const newRepoPath = tokens[i + 2] ?? '';
				i += 2;
				const prefix = 'apps/agentic-org-ui/';
				if (oldRepoPath.startsWith(prefix) && newRepoPath.startsWith(prefix)) {
					renamedNewToOld.set(newRepoPath.slice(prefix.length), oldRepoPath.slice(prefix.length));
				}
			} else if (status.startsWith('C')) {
				i += 2;
			} else {
				i += 1;
			}
		}
	} catch {
		// Ignore when diff cannot run.
	}
	return renamedNewToOld;
}

function parseAllowlist(text: string): { entries: Map<string, number>; errors: string[] } {
	const entries = new Map<string, number>();
	const errors: string[] = [];
	const lines = text.split('\n');
	if (!lines[0]?.startsWith('#')) {
		errors.push('large-files.allow must start with a # comment header');
	}
	let prevPath = '';
	for (let i = 0; i < lines.length; i++) {
		const line = lines[i].trim();
		if (!line || line.startsWith('#')) continue;
		const match = /^(\S+)\s+(\d+)$/.exec(line);
		if (!match) {
			errors.push(`large-files.allow:${i + 1}: malformed entry "${line}"`);
			continue;
		}
		const [, filePath, countStr] = match;
		const count = Number(countStr);
		if (entries.has(filePath)) {
			errors.push(`large-files.allow:${i + 1}: duplicate path "${filePath}"`);
		}
		if (prevPath && filePath < prevPath) {
			errors.push(`large-files.allow:${i + 1}: unsorted path "${filePath}" after "${prevPath}"`);
		}
		prevPath = filePath;
		if (isNonProductionModule(`/${filePath}`)) {
			errors.push(`large-files.allow:${i + 1}: lists non-production file "${filePath}"`);
		}
		if (count <= CAP) {
			errors.push(`large-files.allow:${i + 1}: "${filePath}" (${count}) <= ${CAP}; delete entry`);
		}
		entries.set(filePath, count);
	}
	return { entries, errors };
}

describe('UI file-size ratchet (CAP=700 legacy, NEW_FILE_CAP=500, no CSS extraction)', () => {
	it('enforces exact allowlist counts, bans plain .css imports & @import under lib/routes, and caps unlisted files', () => {
		const files = listProductionFiles();
		const actualCounts = new Map<string, number>();
		const cssImportErrors: string[] = [];

		for (const rel of files) {
			const abs = resolve(UI_ROOT, rel);
			actualCounts.set(rel, countLines(abs));
			const isCssRestricted =
				(/^src\/lib\/(?:modules|components|platform)\//.test(rel) ||
					(rel.startsWith('src/routes/') && rel !== 'src/routes/+layout.svelte')) &&
				/\.(?:svelte|ts|js)$/.test(rel);
			if (isCssRestricted) {
				const err = findDisallowedCssUsage(rel, readFileSync(abs, 'utf8'));
				if (err) cssImportErrors.push(err);
			}
		}

		if (process.env.UI_FILE_SIZE_RATCHET_PRINT === '1') {
			const over = [...actualCounts.entries()]
				.filter(([, n]) => n > CAP)
				.sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
				.map(([p, n]) => `${p} ${n}`)
				.join('\n');
			console.log(over);
		}

		const { entries: allow, errors } = parseAllowlist(readFileSync(ALLOW_PATH, 'utf8'));
		errors.push(...cssImportErrors);

		for (const [rel, n] of actualCounts) {
			const max = allow.get(rel);
			if (max === undefined && n > CAP) {
				errors.push(
					`${rel}: ${n} > ${CAP}; split into cohesive <= ${NEW_FILE_CAP}-line modules (never add to allowlist)`
				);
			} else if (max !== undefined && n !== max) {
				const hint =
					n > max
						? `grew from ${max} to ${n}; extract a cohesive module first`
						: n <= CAP
							? `shrank to ${n} <= ${CAP}; delete its entry from large-files.allow`
							: `shrank from ${max} to ${n}; lower its entry in large-files.allow`;
				errors.push(`${rel}: ${hint}`);
			}
		}

		for (const rel of allow.keys()) {
			if (!actualCounts.has(rel)) {
				errors.push(`${rel}: file removed or untracked; delete its entry from large-files.allow`);
			}
		}

		expect(errors).toEqual([]);
	});

	it('enforces <= 500 LOC on new files, no growth on 501–700 legacy files, and rename-only allowlist additions vs base', () => {
		const baseRef = resolveBaseRef();
		if (!baseRef) return;

		const baseAllowBuf = readGitFile(baseRef, 'apps/agentic-org-ui/large-files.allow');
		if (!baseAllowBuf) return;

		const baseAllow = parseAllowlist(baseAllowBuf.toString('utf8')).entries;
		const headAllow = parseAllowlist(readFileSync(ALLOW_PATH, 'utf8')).entries;
		const renamedNewToOld = gitRenamedPaths(baseRef);
		const errors: string[] = [];

		for (const [p, count] of headAllow) {
			const prev = baseAllow.get(p);
			if (prev !== undefined) {
				if (count > prev) {
					errors.push(`${p}: raised from ${prev} to ${count} (allowlist may only shrink)`);
				}
				continue;
			}
			const renamedFrom = renamedNewToOld.get(p);
			const prevRenamed = renamedFrom ? baseAllow.get(renamedFrom) : undefined;
			if (!renamedFrom || prevRenamed === undefined || headAllow.has(renamedFrom)) {
				errors.push(
					`${p} (${count}): newly added to large-files.allow (only genuine git renames of removed entries are permitted)`
				);
			} else if (count > prevRenamed) {
				errors.push(`${p} (${count}): renamed from ${renamedFrom} (${prevRenamed}) may not grow`);
			}
		}

		for (const rel of listProductionFiles()) {
			if (headAllow.has(rel)) continue;
			const actual = countLines(resolve(UI_ROOT, rel));
			if (actual <= NEW_FILE_CAP) continue;
			const prevBuf =
				readGitFile(baseRef, `apps/agentic-org-ui/${rel}`) ??
				(renamedNewToOld.has(rel)
					? readGitFile(baseRef, `apps/agentic-org-ui/${renamedNewToOld.get(rel)}`)
					: undefined);
			if (!prevBuf) {
				errors.push(
					`${rel}: new file is ${actual} lines > ${NEW_FILE_CAP} (split into cohesive <= ${NEW_FILE_CAP}-line files)`
				);
				continue;
			}
			const prevLines = countLinesFromBuffer(prevBuf);
			if (actual > prevLines) {
				errors.push(
					`${rel}: legacy 501–700 file grew from ${prevLines} to ${actual} lines (files > ${NEW_FILE_CAP} may only shrink)`
				);
			}
		}

		expect(errors).toEqual([]);
	});
});
