/**
 * Checks how many UI iterations (non-merge commits touching `apps/agentic-org-ui/src`),
 * calendar days with UI changes, and changed UI lines have accumulated since the last
 * verified >= 9.0/10 independent audit in `audit-checkpoint.json`.
 *
 * Non-blocking by default so offline CI and hotfixes are never wedged; prints a prominent
 * ACTION REQUIRED banner when any threshold is reached. Pass `--strict` to exit non-zero
 * when an audit is due.
 */
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

interface AuditCheckpoint {
	lastPassSha: string;
	lastPassDate: string;
	auditor: string;
	overall: number;
	scores: Record<string, number>;
	cadence: {
		maxUiCommits: number;
		maxDaysWithUiChanges: number;
		maxChangedUiLines: number;
	};
}

function runGit(args: string[], cwd: string): string {
	return execFileSync('git', args, {
		cwd,
		encoding: 'utf8',
		stdio: ['ignore', 'pipe', 'ignore']
	}).trim();
}

function parseChangedLines(shortstat: string): number {
	const ins = Number(shortstat.match(/(\d+)\s+insertion/)?.[1] ?? 0);
	const del = Number(shortstat.match(/(\d+)\s+deletion/)?.[1] ?? 0);
	return ins + del;
}

function main() {
	const strict = process.argv.includes('--strict');
	const uiDir = resolve(import.meta.dirname, '..');
	const checkpointPath = resolve(uiDir, 'audit-checkpoint.json');
	const checkpoint = JSON.parse(readFileSync(checkpointPath, 'utf8')) as AuditCheckpoint;

	let repoRoot: string;
	try {
		repoRoot = runGit(['rev-parse', '--show-toplevel'], uiDir);
		runGit(['cat-file', '-e', `${checkpoint.lastPassSha}^{commit}`], repoRoot);
	} catch {
		console.log(
			`UI audit cadence: skipped (checkpoint ${checkpoint.lastPassSha} not in local git history).`
		);
		return;
	}

	const range = `${checkpoint.lastPassSha}..HEAD`;
	const uiCommits = Number(
		runGit(
			['rev-list', '--count', '--no-merges', range, '--', 'apps/agentic-org-ui/src'],
			repoRoot
		) || '0'
	);
	const shortstat =
		uiCommits > 0
			? runGit(['diff', '--shortstat', range, '--', 'apps/agentic-org-ui/src'], repoRoot)
			: '';
	const changedLines = parseChangedLines(shortstat);

	const lastDateMs = Date.parse(`${checkpoint.lastPassDate}T00:00:00Z`);
	const daysElapsed = Number.isFinite(lastDateMs)
		? Math.max(0, Math.floor((Date.now() - lastDateMs) / 86_400_000))
		: 0;

	const { maxUiCommits, maxDaysWithUiChanges, maxChangedUiLines } = checkpoint.cadence;
	const reasons: string[] = [];
	if (uiCommits >= maxUiCommits) {
		reasons.push(`${uiCommits}/${maxUiCommits} UI commits`);
	}
	if (uiCommits > 0 && daysElapsed >= maxDaysWithUiChanges) {
		reasons.push(`${daysElapsed}/${maxDaysWithUiChanges} days with UI changes`);
	}
	if (changedLines >= maxChangedUiLines) {
		reasons.push(`${changedLines}/${maxChangedUiLines} changed UI lines`);
	}

	if (reasons.length === 0) {
		console.log(
			`UI audit cadence: OK (${uiCommits}/${maxUiCommits} UI commits, ${changedLines}/${maxChangedUiLines} changed lines, ${daysElapsed}d since ${checkpoint.lastPassSha} [${checkpoint.overall}/10 PASS]).`
		);
		return;
	}

	console.warn(
		[
			'',
			'====================================================================',
			`ACTION REQUIRED: UI 6-dimension audit due (${reasons.join(', ')})`,
			`Last PASS: ${checkpoint.lastPassSha} on ${checkpoint.lastPassDate} (${checkpoint.overall}/10 by ${checkpoint.auditor})`,
			'Run: (1) fresh internal code-reviewer pass, then (2) `bun run audit:ui`',
			'Fix any High/Medium finding or dimension < 9.0/10, then update audit-checkpoint.json.',
			'See: .agents/rules/svelte-architecture-and-quality.md §9',
			'====================================================================',
			''
		].join('\n')
	);
	if (strict) process.exit(1);
}

main();
