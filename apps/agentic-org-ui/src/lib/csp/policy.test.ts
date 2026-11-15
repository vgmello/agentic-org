import { expect, it } from 'vitest';
import { readFile } from 'node:fs/promises';

it('pins the production HTML policy independently of the source file', async () => {
	const policy = await readFile(new URL('../../../../../config/csp/html.txt', import.meta.url), 'utf8');
	expect(policy.replace(/\n$/, '')).toBe("default-src 'none'; script-src 'self' {script_hashes}; style-src 'self' 'unsafe-inline'; img-src 'self'; font-src 'self'; connect-src 'self'; manifest-src 'self'; worker-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; object-src 'none'");
});

it('pins the production non-HTML policy independently of the source file', async () => {
	const policy = await readFile(new URL('../../../../../config/csp/non-html.txt', import.meta.url), 'utf8');
	expect(policy.replace(/\n$/, '')).toBe("default-src 'none'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'");
});
