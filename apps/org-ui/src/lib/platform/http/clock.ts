const ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
const MAX_TIME = 2 ** 48 - 1;

/** Server-clock calibration is supplied by the session adapter, never inferred from a cookie. */
export function createOperationClock({
	now = Date.now,
	random = () => globalThis.crypto.getRandomValues(new Uint8Array(10))
}: { now?: () => number; random?: () => Uint8Array } = {}) {
	let offset: number | undefined;
	return {
		synchronize(serverTimeMs: number) {
			if (!Number.isSafeInteger(serverTimeMs) || serverTimeMs < 0 || serverTimeMs > MAX_TIME)
				throw new Error('Invalid server_time_ms');
			offset = serverTimeMs - now();
		},
		newId(): string {
			if (offset === undefined) throw new Error('Synchronize the server clock before creating an operation');
			let time = Math.floor(now() + offset);
			if (!Number.isSafeInteger(time) || time < 0 || time > MAX_TIME) throw new Error('Invalid operation time');
			let id = '';
			for (let i = 0; i < 10; i++, time = Math.floor(time / 32)) id = ALPHABET[time % 32] + id;
			const bytes = random();
			if (bytes.length !== 10) throw new Error('Operation IDs require 80 random bits');
			let bits = 0;
			let value = 0;
			for (const byte of bytes) {
				value = (value << 8) | byte;
				bits += 8;
				while (bits >= 5) {
					bits -= 5;
					id += ALPHABET[(value >>> bits) & 31];
				}
			}
			return id;
		}
	};
}
