import { expect, it } from 'vitest';
import { isControl, type Violation } from '../../../e2e-csp-gateway/control';

const control: Violation = { kind: 'violation', documentURI: 'http://127.0.0.1:4273/welcome?csp-control=c12345678', blockedURI: 'inline', effectiveDirective: 'script-src-elem' };
for (const effectiveDirective of ['script-src', 'script-src-elem']) {
	it(`recognizes the marked script positive control reported as ${effectiveDirective}`, () => {
		expect(isControl({ ...control, effectiveDirective }, 'c12345678')).toBe(true);
	});
}
for (const change of [{ documentURI: 'http://127.0.0.1:4273/welcome' }, { documentURI: 'http://127.0.0.1:4273/welcome?csp-control=c87654321' }, { blockedURI: 'http://other.invalid/script.js' }, { effectiveDirective: 'img-src' }, { effectiveDirective: 'script-src-attr' }]) {
	it(`retains an unrelated real violation: ${JSON.stringify(change)}`, () => {
		expect(isControl({ ...control, ...change }, 'c12345678')).toBe(false);
	});
}
