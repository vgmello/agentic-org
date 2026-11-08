import { ORGANIZATION_NAME_MAX_CHARS } from './proto';

export type NameCheck = { ok: true; name: string } | { ok: false; message: string };

// The organization-name rule is org-proto's `OrganizationName::parse` (crates/org-proto/src/name.rs):
// trimmed, 1..=64 characters, no control characters, and no invisible or direction-changing ones.
// The office is the authority; this only answers before a round trip, and org-proto's shared vectors
// (crates/org-proto/test-vectors/organization-names.json) test it.

// "Trimmed" is Unicode White_Space, which is what Rust's `str::trim` removes. JavaScript's own `trim()`
// differs: it strips U+FEFF and keeps U+0085.
const WHITE_SPACE = '\\t\\n\\v\\f\\r \\u0085\\u00a0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000';
const EDGES = new RegExp(`^[${WHITE_SPACE}]+|[${WHITE_SPACE}]+$`, 'g');

export const trimWhiteSpace = (s: string) => s.replace(EDGES, '');

// Format characters (Cf), default-ignorable code points, and the line/paragraph separators are refused,
// except the four that scripts and emoji need.
const INVISIBLE = /[\p{Cf}\p{Default_Ignorable_Code_Point}\u2028\u2029]/u;
const ALLOWED_INVISIBLE = new Set(['\u200c', '\u200d', '\ufe0e', '\ufe0f']);
// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

export function validateName(raw: string): NameCheck {
	const name = trimWhiteSpace(raw);
	if (name === '') return { ok: false, message: 'Give the organization a name.' };
	if ([...name].length > ORGANIZATION_NAME_MAX_CHARS)
		return { ok: false, message: `Use ${ORGANIZATION_NAME_MAX_CHARS} characters or fewer.` };
	if (CONTROL.test(name)) return { ok: false, message: "The name can't contain control characters." };
	if ([...name].some((c) => INVISIBLE.test(c) && !ALLOWED_INVISIBLE.has(c)))
		return { ok: false, message: "The name can't contain invisible or direction-changing characters." };
	return { ok: true, name };
}
