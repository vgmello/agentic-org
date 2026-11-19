// The check code typed at HQ to approve a join (offices-and-teams-ux §6.3, OFC-6): 8 characters of Crockford base32.
// Case, spaces and dashes are ignored; O reads as 0, I and L as 1; a U or any other character that can't be in a code is rejected.

export const CODE_LENGTH = 8;
export const CODE_HINT = 'Codes use digits and letters, without U';
const ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

export interface CodeRead {
	/** The code as it counts: canonical characters, at most 8. */
	code: string;
	/** Set when something typed was rejected. */
	rejected: boolean;
	/** Whether it has all 8 characters, so Approve is enabled. */
	complete: boolean;
}

export function readCheckCode(raw: string): CodeRead {
	let code = '';
	let rejected = false;
	for (const ch of raw.toUpperCase()) {
		if (/[\s-]/u.test(ch)) continue;
		const mapped = ch === 'O' ? '0' : ch === 'I' || ch === 'L' ? '1' : ch;
		if (!ALPHABET.includes(mapped)) {
			rejected = true;
			continue;
		}
		if (code.length < CODE_LENGTH) code += mapped;
	}
	return { code, rejected, complete: code.length === CODE_LENGTH };
}

/** "K7QP4MZT" shown as "K7QP 4MZT". */
export const groupCode = (code: string): string =>
	code.length > 4 ? `${code.slice(0, 4)} ${code.slice(4)}` : code;

/** Whether a typed code is the one HQ expects, by the same reading. */
export const codesMatch = (typed: string, expected: string): boolean =>
	readCheckCode(typed).code === readCheckCode(expected).code;
