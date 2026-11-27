// Shared runtime wire-decoding primitives for HTTP API adapters (`src/lib/modules/*/http-api.ts`,
// `live-hydration.ts`, `tess/api.ts`). Replaces ad-hoc per-file `asRec`/`asStr`/`asNum` copies.

export type Rec = Record<string, unknown>;

export function asRec(value: unknown): Rec | undefined {
	return value !== null && typeof value === 'object' && !Array.isArray(value)
		? (value as Rec)
		: undefined;
}

export function asStr(value: unknown): string | undefined {
	return typeof value === 'string' ? value : undefined;
}

export function asNonEmptyStr(value: unknown): string | undefined {
	return typeof value === 'string' && value.trim().length > 0 ? value.trim() : undefined;
}

export function asNum(value: unknown): number | undefined {
	return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

export function asBool(value: unknown): boolean | undefined {
	return typeof value === 'boolean' ? value : undefined;
}

export function asArr(value: unknown): unknown[] {
	return Array.isArray(value) ? value : [];
}

export function asStrList(value: unknown): string[] {
	if (!Array.isArray(value)) return [];
	return value.filter((item): item is string => typeof item === 'string');
}

export function unwrapResult(body: unknown): Rec | undefined {
	const rec = asRec(body);
	if (!rec) return undefined;
	return asRec(rec.result) ?? rec;
}
