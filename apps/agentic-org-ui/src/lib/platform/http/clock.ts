const MAX_TIME = 2 ** 48 - 1;

function hex2(byte: number): string {
	return byte.toString(16).padStart(2, '0');
}

/** Encodes a 48-bit Unix millisecond timestamp and 10 random bytes as canonical lowercase UUIDv7. */
export function formatUuidV7(timeMs: number, bytes: Uint8Array): string {
	if (!Number.isSafeInteger(timeMs) || timeMs < 0 || timeMs > MAX_TIME)
		throw new Error('Invalid operation time');
	if (bytes.length !== 10) throw new Error('Operation IDs require 80 random bits');
	const out = new Uint8Array(16);
	let t = timeMs;
	for (let i = 5; i >= 0; i--) {
		out[i] = t % 256;
		t = Math.floor(t / 256);
	}
	out[6] = 0x70 | (bytes[0] & 0x0f);
	out[7] = bytes[1];
	out[8] = 0x80 | (bytes[2] & 0x3f);
	for (let i = 3; i < 10; i++) out[6 + i] = bytes[i];
	const h = Array.from(out, hex2).join('');
	return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

/** Server-clock calibration is supplied by the session adapter, never inferred from a cookie. */
export function createOperationClock({
	now = () => performance.timeOrigin + performance.now(),
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
			if (offset === undefined)
				throw new Error('Synchronize the server clock before creating an operation');
			return formatUuidV7(Math.floor(now() + offset), random());
		}
	};
}
