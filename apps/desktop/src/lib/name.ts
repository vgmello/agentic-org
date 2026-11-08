import { ORGANIZATION_NAME_MAX_CHARS } from './proto';

export type NameCheck = { ok: true; name: string } | { ok: false; message: string };

// Unicode White_Space, which is what the office trims (Rust `str::trim`). JavaScript's own
// `trim()` differs: it strips U+FEFF and keeps U+0085.
const WHITE_SPACE = '\\t\\n\\v\\f\\r \\u0085\\u00a0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000';
const EDGES = new RegExp(`^[${WHITE_SPACE}]+|[${WHITE_SPACE}]+$`, 'g');

export const trimWhiteSpace = (s: string) => s.replace(EDGES, '');

/**
 * The same rule the office applies (1 to 64 characters after trimming White_Space, no control characters),
 * so the form can answer before a round trip. The office stays the authority.
 */
export function validateName(raw: string): NameCheck {
	const name = trimWhiteSpace(raw);
	if (name === '') return { ok: false, message: 'Give the organization a name.' };
	if ([...name].length > ORGANIZATION_NAME_MAX_CHARS)
		return { ok: false, message: `Use ${ORGANIZATION_NAME_MAX_CHARS} characters or fewer.` };
	// eslint-disable-next-line no-control-regex
	if (/[\u0000-\u001f\u007f-\u009f]/.test(name))
		return { ok: false, message: "The name can't contain control characters." };
	return { ok: true, name };
}
