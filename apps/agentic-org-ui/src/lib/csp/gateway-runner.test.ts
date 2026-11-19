import { expect, it } from 'vitest';
import { gatewayExecutable, gatewayPort } from '../../../scripts/csp-gateway';

const artifact = { reason: 'compiler-artifact', target: { name: 'csp_harness', kind: ['example'] }, executable: '/private/tmp/custom-target/debug/examples/csp_harness' };
it('uses the freshly reported example path, including a custom Cargo target directory', () => {
	const output = `\n\n${JSON.stringify({ reason: 'compiler-message' })}\n${JSON.stringify(artifact)}\n${JSON.stringify({ reason: 'build-finished', success: true })}\n`;
	expect(gatewayExecutable(output)).toBe(artifact.executable);
});
for (const change of [{ reason: 'build-finished' }, { target: { name: 'other', kind: ['example'] } }, { target: { name: 'csp_harness', kind: ['bin'] } }, { executable: null }, { executable: 42 }, { executable: '' }]) {
	it(`refuses a missing/stale artifact instead of assuming a target path: ${JSON.stringify(change)}`, () => {
		expect(() => gatewayExecutable(JSON.stringify({ ...artifact, ...change }))).toThrow('Cargo did not report the csp_harness executable');
	});
}
it('refuses malformed Cargo output', () => {
	expect(() => gatewayExecutable('not JSON')).toThrow();
});
for (const value of ['0', '65536', '-1', '2.5', 'text', '']) {
	it(`refuses invalid gateway port ${JSON.stringify(value)}`, () => { expect(() => gatewayPort(value)).toThrow('Invalid CSP_GW_PORT'); });
}
it('supports the default and a chosen gateway port', () => {
	expect(gatewayPort(undefined)).toBe(4273);
	expect(gatewayPort('4553')).toBe(4553);
});
