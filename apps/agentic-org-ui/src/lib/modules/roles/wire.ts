import type { AppError } from '$lib/proto';
import { readOperationId } from '$lib/platform/http/wire';

export interface WireRoleRecord {
	id: string;
	key?: string;
	name: string;
	body: string;
	revision: number;
	created_at?: string;
	created_by_kind?: string;
	created_by_id?: string;
	updated_at?: string;
	updated_by_kind?: string;
	updated_by_id?: string;
}

export interface WireRoleEntityAck {
	id: string;
	revision: number;
	generation: number;
}

const RFC3339_TIMESTAMP =
	/^(\d{4})-(\d{2})-(\d{2})([Tt])(\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?(?:([Zz])|([+-])(\d{2}):(\d{2}))$/;
/** `9999-12-31T23:59:59.999Z`: largest UTC millisecond with a four-digit RFC 3339 year. */
const MAX_RFC3339_4DIGIT_YEAR_MS = 253_402_300_799_999;

function malformed(fallbackMessage: string): never {
	throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies AppError;
}

const MONTH_DAYS = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31] as const;

function daysInMonth(year: number, month: number): number {
	if (month === 2 && year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0)) return 29;
	return MONTH_DAYS[month] ?? 0;
}

function isValidCalendarAndClock(
	year: number,
	month: number,
	day: number,
	hour: number,
	minute: number,
	second: number
): boolean {
	if (year < 1970 || month < 1 || month > 12) return false;
	if (day < 1 || day > daysInMonth(year, month)) return false;
	return hour <= 23 && minute <= 59 && second <= 60;
}

function parseOffsetMinutes(
	zulu: string | undefined,
	sign: string | undefined,
	offHourRaw: string | undefined,
	offMinuteRaw: string | undefined
): number | undefined {
	if (zulu) return 0;
	const offHour = Number(offHourRaw);
	const offMinute = Number(offMinuteRaw);
	if (offHour > 23 || offMinute > 59) return undefined;
	return (sign === '+' ? 1 : -1) * (offHour * 60 + offMinute);
}

function formatValidatedTimestamp(
	value: string,
	epochMs: number,
	sep: string,
	zulu: string | undefined,
	second: number
): string | undefined {
	if (!Number.isFinite(epochMs) || epochMs < 0) return undefined;
	if (epochMs > MAX_RFC3339_4DIGIT_YEAR_MS) {
		if (second === 60) return new Date(MAX_RFC3339_4DIGIT_YEAR_MS).toISOString();
		const normalized = value.replace('t', 'T').replace('z', 'Z');
		return Number.isFinite(Date.parse(normalized)) ? normalized : undefined;
	}
	if (sep === 't' || zulu === 'z' || second === 60) {
		return new Date(epochMs).toISOString();
	}
	return value;
}

/**
 * Validates an RFC 3339 timestamp with exact Gregorian calendar bounds (matching
 * `crates/org-office/src/time.rs::parse_rfc3339_ms`). Accepts lowercase `t`/`z`, offsets,
 * and leap seconds (`second == 60`), rejecting impossible dates like `2026-02-30` and
 * normalizing non-ISO-`Date`-safe forms (lowercase `t`/`z` or leap second `60`) to canonical UTC.
 */
export function parseRfc3339Timestamp(value: string): string | undefined {
	const match = RFC3339_TIMESTAMP.exec(value);
	if (!match) return undefined;
	const year = Number(match[1]);
	const month = Number(match[2]);
	const day = Number(match[3]);
	const hour = Number(match[5]);
	const minute = Number(match[6]);
	const second = Number(match[7]);
	if (!isValidCalendarAndClock(year, month, day, hour, minute, second)) return undefined;
	const offsetMinutes = parseOffsetMinutes(match[9], match[10], match[11], match[12]);
	if (offsetMinutes === undefined) return undefined;
	const frac = match[8];
	const millis = frac ? Number(frac.slice(0, 3).padEnd(3, '0')) : 0;
	const epochMs =
		Date.UTC(year, month - 1, day, hour, minute, second, millis) - offsetMinutes * 60_000;
	return formatValidatedTimestamp(value, epochMs, match[4], match[9], second);
}

function readRoleUuid(value: unknown, fallbackMessage: string): string {
	try {
		return readOperationId(value);
	} catch {
		malformed(fallbackMessage);
	}
}

function optionalString(
	record: Record<string, unknown>,
	field: string,
	fallbackMessage: string
): string | undefined {
	const value = record[field];
	if (value === undefined || value === null) return undefined;
	if (typeof value !== 'string') malformed(fallbackMessage);
	return value.length > 0 ? value : undefined;
}

function optionalTimestamp(
	record: Record<string, unknown>,
	field: string,
	fallbackMessage: string
): string | undefined {
	const value = optionalString(record, field, fallbackMessage);
	if (value === undefined) return undefined;
	const parsed = parseRfc3339Timestamp(value);
	if (parsed === undefined) malformed(fallbackMessage);
	return parsed;
}

function positiveSafeInteger(value: unknown, fallbackMessage: string): number {
	if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 1) {
		malformed(fallbackMessage);
	}
	return value;
}

function nonNegativeSafeInteger(value: unknown, fallbackMessage: string): number {
	if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0) {
		malformed(fallbackMessage);
	}
	return value;
}

/** Strictly decodes an untrusted gateway `RoleRecord` payload. */
export function readRoleRecord(value: unknown, fallbackMessage: string): WireRoleRecord {
	if (!value || typeof value !== 'object' || Array.isArray(value)) malformed(fallbackMessage);
	const rec = value as Record<string, unknown>;
	if (typeof rec.name !== 'string' || typeof rec.body !== 'string') {
		malformed(fallbackMessage);
	}
	const id = readRoleUuid(rec.id, fallbackMessage);
	return {
		id,
		key: optionalString(rec, 'key', fallbackMessage),
		name: rec.name,
		body: rec.body,
		revision: positiveSafeInteger(rec.revision, fallbackMessage),
		created_at: optionalTimestamp(rec, 'created_at', fallbackMessage),
		created_by_kind: optionalString(rec, 'created_by_kind', fallbackMessage),
		created_by_id: optionalString(rec, 'created_by_id', fallbackMessage),
		updated_at: optionalTimestamp(rec, 'updated_at', fallbackMessage),
		updated_by_kind: optionalString(rec, 'updated_by_kind', fallbackMessage),
		updated_by_id: optionalString(rec, 'updated_by_id', fallbackMessage)
	};
}

/** Decodes a single `RoleRecord` and requires its `id` to match canonical UUIDv7 `expectedId`. */
export function readRoleRecordWithId(
	value: unknown,
	expectedId: string,
	fallbackMessage: string
): WireRoleRecord {
	const record = readRoleRecord(value, fallbackMessage);
	if (record.id !== expectedId) malformed(fallbackMessage);
	return record;
}

/** Strictly decodes `EntityOpResult` from role create/update replies before any fallback display. */
export function readRoleEntityAck(
	value: unknown,
	expectedId: string,
	fallbackMessage: string
): WireRoleEntityAck {
	if (!value || typeof value !== 'object' || Array.isArray(value)) malformed(fallbackMessage);
	const rec = value as Record<string, unknown>;
	const id = readRoleUuid(rec.id, fallbackMessage);
	if (id !== expectedId) malformed(fallbackMessage);
	return {
		id,
		revision: positiveSafeInteger(rec.revision, fallbackMessage),
		generation: nonNegativeSafeInteger(rec.generation, fallbackMessage)
	};
}
