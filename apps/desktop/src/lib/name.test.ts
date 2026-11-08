import { describe, expect, it } from 'vitest';
import vectors from '../../test-vectors/organization-names.json';
import { validateName } from './name';

// The same vectors the Rust mock is tested against (src-tauri/src/mock.rs).
describe('validateName', () => {
	it.each(vectors.vectors.map((v) => [JSON.stringify(v.input).slice(0, 40), v] as const))(
		'%s',
		(_label, v) => {
			const result = validateName(v.input);
			if ('name' in v) expect(result).toEqual({ ok: true, name: v.name });
			else expect(result.ok).toBe(false);
		}
	);
});
