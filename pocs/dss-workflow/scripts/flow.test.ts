import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

let dir: string;
beforeEach(() => (dir = mkdtempSync(join(tmpdir(), 'flow-cli-'))));
afterEach(() => rmSync(dir, { recursive: true, force: true }));

const cli = (...args: string[]) =>
	spawnSync('bun', [join(import.meta.dirname, 'flow.ts'), '--state', join(dir, 's.json'), ...args], { encoding: 'utf8' });

describe('flow CLI (each call is a fresh process over the saved state)', () => {
	it('drives a run across processes and hands off on a task move', () => {
		expect(cli('--caps', 'task,project', 'start', 'dss', '--task', 'T-1').stdout).toContain('dispatch: run R1 | step R1.1 | agent: product-owner');
		for (const [step, outcome] of [['R1.1', 'context_sufficient'], ['R1.2', 'published'], ['R1.3', 'recorded'], ['R1.4', 'tasks_created']]) {
			expect(cli('begin', step).status).toBe(0);
			expect(cli('report', step, outcome).stdout).toContain('accepted');
		}
		const moved = cli('task', 'T-1', 'in_review');
		expect(moved.stdout).toContain('dispatch: run R1 | step R1.6 | agent: reviewer');
		expect(moved.stdout).toContain('# Review');
		expect(cli('status').stdout).toContain('R1 active: code-review [dispatched] @reviewer');
	});

	it('rejects bad input with exit 2 and refused reports with exit 3', () => {
		expect(cli('start', 'dss').status).toBe(0);
		expect(cli('begin', 'R1.1').status).toBe(0);
		const refused = cli('report', 'R1.1', 'published');
		expect(refused.status).toBe(3);
		expect(refused.stdout).toMatch(/not an outcome/);
		for (const args of [[], ['nope'], ['start'], ['task', 'T-1', 'shipped'], ['--caps', 'task', 'status'], ['begin', 'R9.9']])
			expect(cli(...args).status, args.join(' ')).toBe(2);
	});
});
