import { ORGANIZATION_NAME_MAX_CHARS } from './proto';

export type NameCheck = { ok: true; name: string } | { ok: false; message: string };

/**
 * The same rule the office applies (1 to 64 characters after trimming, no control characters),
 * so the form can answer before a round trip. The office stays the authority.
 */
export function validateName(raw: string): NameCheck {
	const name = raw.trim();
	if (name === '') return { ok: false, message: 'Give the organization a name.' };
	if ([...name].length > ORGANIZATION_NAME_MAX_CHARS)
		return { ok: false, message: `Use ${ORGANIZATION_NAME_MAX_CHARS} characters or fewer.` };
	// eslint-disable-next-line no-control-regex
	if (/[\u0000-\u001f\u007f-\u009f]/.test(name))
		return { ok: false, message: "The name can't contain control characters." };
	return { ok: true, name };
}
