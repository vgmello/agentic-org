import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const cli = (...args: string[]) =>
	spawnSync('bun', [join(import.meta.dirname, 'flow.ts'), ...args], { encoding: 'utf8' });

describe('flow CLI', () => {
	it('returns the next block markdown and marks handoffs', () => {
		const r = cli('fire', 'execute-tasks', 'status:in_review');
		expect(r.status).toBe(0);
		expect(r.stdout).toContain('agent: reviewer | via: execute-tasks on status:in_review');
		expect(r.stdout).toContain('HANDOFF: deliver this block to agent reviewer');
		expect(r.stdout).toContain('# Review');
	});

	it('passes event flags to the guard', () => {
		expect(cli('fire', 'publish-product-dss', 'product-dss.published', 'standards.defined').stdout).toContain(
			'block: tech-dss-autonomous'
		);
		const same = cli('fire', 'product-dss-dialogue', 'context.sufficient');
		expect(same.stdout).not.toContain('HANDOFF');
	});

	it.each([
		[[], /bad arguments/],
		[['command'], /bad arguments/],
		[['command', 'nope'], /Unknown command/],
		[['fire', 'execute-tasks', 'status:approved'], /No transition/],
		[['fire', 'publish-product-dss', 'product-dss.published', 'standard.defined'], /not used by/],
		[['fire', 'publish-product-dss', 'product-dss.published', '!standards.defined'], /not used by/],
		[['fire', 'publish-product-dss', 'product-dss.published', 'standards.changed'], /not used by/]
	])('rejects %j with exit 2', (args, error) => {
		const r = cli(...args);
		expect(r.status).toBe(2);
		expect(r.stderr).toMatch(error);
	});
});
