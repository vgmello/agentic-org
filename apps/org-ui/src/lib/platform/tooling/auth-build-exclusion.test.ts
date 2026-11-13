import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { afterEach, expect, test } from 'vitest';

const directories: string[] = [];
afterEach(() => { for (const directory of directories.splice(0)) rmSync(directory, { recursive: true, force: true }); });
function check(source: string, extension = 'js') {
	const directory = mkdtempSync(join(tmpdir(), 'auth-build-exclusion-')); directories.push(directory);
	writeFileSync(join(directory, `bundle.${extension}`), source);
	return spawnSync('bun', ['scripts/check-build-assets.ts', directory], { encoding: 'utf8' });
}
for (const marker of ['/__p6-auth/new', 'sessionTestMiddleware', 'p6_auth_case', 'session-browser-harness']) {
	for (const extension of ['js', 'html']) test(`SIGN-7 production build refuses ${marker} in ${extension}`, () => {
		const result = check(JSON.stringify(marker), extension);
		expect(result.status).toBe(1); expect(result.stderr).toContain('test auth fixture');
	});
}
test('SIGN-7 production build permits the real session transport', () => {
	const result = check('fetch("/api/v1/session", { credentials: "same-origin" });');
	expect(result.status).toBe(0); expect(result.stdout).toContain('auth fixtures');
});
