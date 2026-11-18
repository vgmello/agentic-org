/**
 * Browser mirror of the host's credential rules (`crates/org-host/src/accounts/rules.rs`).
 * The host stays authoritative: this only gives feedback before a request is sent.
 */
export const USERNAME_MAX_SCALARS = 64;
export const PASSWORD_MIN_SCALARS = 12;
export const PASSWORD_MAX_BYTES = 1024;

export type CredentialField = 'login' | 'password';
export type CredentialRule = 'required' | 'format' | 'taken' | 'too_short' | 'too_long' | 'mismatch';
export interface CredentialProblem {
	readonly field: CredentialField;
	readonly rule: CredentialRule;
}

const scalars = (text: string) => Array.from(text).length;
// Cc, Cf and the line/paragraph separators, as the host's `is_invisible_or_control`.
const invisibleOrControl = /[\p{Cc}\p{Cf}\u2028\u2029]/u;
// Rust's `char::is_whitespace` is Unicode White_Space; U+FEFF (in JS `\s`) is Cf and refused anyway.
const whitespace = /\s/u;

/** A new username: NFC, 1..64 scalars, no controls or format characters, no edge whitespace. */
export function checkUsername(input: string): CredentialProblem | undefined {
	if (input.length === 0) return { field: 'login', rule: 'required' };
	const display = input.normalize('NFC');
	const count = scalars(display);
	if (count === 0 || count > USERNAME_MAX_SCALARS || invisibleOrControl.test(display))
		return { field: 'login', rule: 'format' };
	const chars = Array.from(display);
	if (whitespace.test(chars[0]) || whitespace.test(chars[chars.length - 1]))
		return { field: 'login', rule: 'format' };
	return undefined;
}

/** A new password: at most 1024 UTF-8 bytes as typed (checked first), then at least 12 NFC scalars. */
export function checkNewPassword(input: string): CredentialProblem | undefined {
	if (input.length === 0) return { field: 'password', rule: 'required' };
	if (new TextEncoder().encode(input).byteLength > PASSWORD_MAX_BYTES)
		return { field: 'password', rule: 'too_long' };
	if (scalars(input.normalize('NFC')) < PASSWORD_MIN_SCALARS) return { field: 'password', rule: 'too_short' };
	return undefined;
}

/** Sign-in applies no format rule (an invalid name is an unknown login, SIGN-4); only emptiness is caught (SIGN-3). */
export function checkSignIn(login: string, password: string): CredentialProblem[] {
	const problems: CredentialProblem[] = [];
	if (login.length === 0) problems.push({ field: 'login', rule: 'required' });
	if (password.length === 0) problems.push({ field: 'password', rule: 'required' });
	return problems;
}

/** Registration: username, password rule, then the two entries must match (SIGN-21). */
export function checkRegistration(login: string, password: string, again: string): CredentialProblem[] {
	const problems: CredentialProblem[] = [];
	const name = checkUsername(login);
	if (name) problems.push(name);
	const pass = checkNewPassword(password);
	if (pass) problems.push(pass);
	else if (password !== again) problems.push({ field: 'password', rule: 'mismatch' });
	return problems;
}
