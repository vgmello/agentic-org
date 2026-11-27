/** Primitive runtime readers, scopes, envelopes, and error mappers for the published HTTP contract. */
export const API_VERSION = 1;
export const CONTRACT_REVISION = 2;

export type JsonValue = null | boolean | number | string | JsonValue[] | JsonObject;
export interface JsonObject {
	[key: string]: JsonValue;
}
export type Reader<T> = (value: unknown) => T;

export class WireDecodeError extends Error {
	constructor() {
		super('Invalid gateway payload');
		this.name = 'WireDecodeError';
	}
}

export const invalid = (): never => {
	throw new WireDecodeError();
};

export const checked =
	<T>(reader: Reader<T>): Reader<T> =>
	(value) => {
		try {
			return reader(value);
		} catch {
			throw new WireDecodeError();
		}
	};

export const fields = (value: unknown): Record<string, unknown> => {
	if (
		!value ||
		typeof value !== 'object' ||
		Array.isArray(value) ||
		![Object.prototype, null].includes(Object.getPrototypeOf(value))
	)
		return invalid();
	return value as Record<string, unknown>;
};

export const own = (value: object, key: string) => Object.hasOwn(value, key);
export const field = (value: Record<string, unknown>, key: string): unknown =>
	own(value, key) ? value[key] : undefined;
export const text: Reader<string> = (value) => (typeof value === 'string' ? value : invalid());
export const bool: Reader<boolean> = (value) => (typeof value === 'boolean' ? value : invalid());
export const nonempty: Reader<string> = (value) => {
	const result = text(value);
	return result ? result : invalid();
};

export const readSafeInteger: Reader<number> = checked((value) =>
	typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
		? value === 0
			? 0
			: value
		: invalid()
);

export const u32: Reader<number> = (value) => {
	const result = readSafeInteger(value);
	return result <= 0xffff_ffff ? result : invalid();
};

export const optional =
	<T>(reader: Reader<T>): Reader<T | undefined> =>
	(value) =>
		value == null ? undefined : reader(value);

export const defaulted =
	<T>(reader: Reader<T>, fallback: () => T): Reader<T> =>
	(value) =>
		value === undefined ? fallback() : reader(value);

export const array =
	<T>(reader: Reader<T>): Reader<T[]> =>
	(value) =>
		Array.isArray(value) ? value.map(reader) : invalid();

export const word =
	<const T extends readonly string[]>(known: T): Reader<T[number] | 'unknown'> =>
	(value) => {
		const name = text(value);
		return known.includes(name) ? (name as T[number]) : 'unknown';
	};

export const openWord =
	<const T extends readonly string[]>(known: T): Reader<T[number] | 'unknown'> =>
	(value) =>
		typeof value === 'string' && known.includes(value) ? (value as T[number]) : 'unknown';

export const omittedFalse: Reader<true | undefined> = (value) =>
	value === undefined ? undefined : bool(value) || undefined;

type Shape = Record<string, Reader<unknown>>;
type Parsed<S extends Shape> = {
	[K in keyof S as undefined extends ReturnType<S[K]> ? never : K]: ReturnType<S[K]>;
} & {
	[K in keyof S as undefined extends ReturnType<S[K]> ? K : never]?: Exclude<
		ReturnType<S[K]>,
		undefined
	>;
};

export function object<S extends Shape>(shape: S): Reader<Parsed<S>> {
	return (value) => {
		const input = fields(value);
		return Object.fromEntries(
			Object.entries(shape).flatMap(([key, reader]) => {
				const result = reader(field(input, key));
				return result === undefined ? [] : [[key, result]];
			})
		) as Parsed<S>;
	};
}

export function json(value: unknown, depth = 0): JsonValue {
	if (depth > 128) return invalid();
	if (value === null || typeof value === 'string' || typeof value === 'boolean') return value;
	if (typeof value === 'number') return Number.isFinite(value) ? value : invalid();
	if (Array.isArray(value)) return value.map((item) => json(item, depth + 1));
	return Object.fromEntries(
		Object.entries(fields(value)).map(([key, item]) => [key, json(item, depth + 1)])
	);
}

export const jsonObject: Reader<JsonObject> = (value) => {
	fields(value);
	return json(value) as JsonObject;
};
export const readJsonObject = checked(jsonObject);

const UUID_V7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
export const operationId: Reader<string> = (value) => {
	const id = text(value);
	return UUID_V7.test(id) ? id : invalid();
};
export const readOperationId = checked(operationId);
export function operationTimeMs(value: unknown): number {
	const id = readOperationId(value);
	return Number.parseInt(id.slice(0, 8) + id.slice(9, 13), 16);
}

export const readVersion = checked(object({ api_version: u32, contract_revision: u32 }));
export type Version = ReturnType<typeof readVersion>;
export const versionCompatible = (
	version: Version,
	required: Version = { api_version: API_VERSION, contract_revision: CONTRACT_REVISION }
) =>
	version.api_version === required.api_version &&
	version.contract_revision >= required.contract_revision;

export type Scope =
	| { kind: 'instance'; instance_id: string }
	| { kind: 'organization'; instance_id: string; org_id: string }
	| { kind: 'office'; instance_id: string; org_id: string; office_id: string };
export type ResponseScope = Scope | (JsonObject & { kind: string });

export const readScope: Reader<Scope> = checked((value) => {
	const input = fields(value);
	const kind = text(field(input, 'kind'));
	const instance_id = text(field(input, 'instance_id'));
	if (kind === 'instance') return { kind, instance_id };
	const org_id = text(field(input, 'org_id'));
	if (kind === 'organization') return { kind, instance_id, org_id };
	if (kind === 'office')
		return { kind, instance_id, org_id, office_id: text(field(input, 'office_id')) };
	return invalid();
});

export const readResponseScope: Reader<ResponseScope> = checked((value) => {
	const input = fields(value);
	const kind = text(field(input, 'kind'));
	return ['instance', 'organization', 'office'].includes(kind)
		? readScope(value)
		: { ...jsonObject(value), kind };
});

export function knownScope(scope: ResponseScope): Scope | undefined {
	try {
		return readScope(scope);
	} catch {
		return undefined;
	}
}

export const scopeMatches = (scope: Scope, target: Scope): boolean =>
	JSON.stringify(readScope(scope)) === JSON.stringify(readScope(target));

export type OperationKind = 'host' | 'org' | 'work' | 'msg';
export const readKind: Reader<OperationKind> = (value) =>
	['host', 'org', 'work', 'msg'].includes(text(value)) ? (value as OperationKind) : invalid();

const idFields = { host: 'operation_id', org: 'command_id', work: 'change_id', msg: 'id' } as const;
const businessParams: Reader<JsonObject> = (value) => {
	const params = fields(value);
	if (
		['operation_id', 'command_id', 'change_id', 'id', 'home', 'clear_home', 'app_token'].some(
			(key) => own(params, key)
		)
	)
		return invalid();
	if (own(params, 'profile') && own(fields(params.profile), 'home')) return invalid();
	return jsonObject(params);
};

export interface CommandEnvelope<P> {
	operation_id: string;
	scope: Scope;
	params: P;
}
export function readCommandEnvelope(value: unknown): CommandEnvelope<JsonObject>;
export function readCommandEnvelope<P>(value: unknown, params: Reader<P>): CommandEnvelope<P>;
export function readCommandEnvelope<P = JsonObject>(
	value: unknown,
	params: Reader<P> = readJsonObject as Reader<P>
): CommandEnvelope<P> {
	return checked((value) => {
		const input = fields(value);
		// The envelope has three keys. Any other top-level key is refused, not ignored, as the server does.
		if (Object.keys(input).some((key) => !['operation_id', 'scope', 'params'].includes(key)))
			return invalid();
		return {
			operation_id: operationId(field(input, 'operation_id')),
			scope: readScope(field(input, 'scope')),
			params: params(businessParams(field(input, 'params')))
		};
	})(value);
}

export function nativeParams<P>(envelope: CommandEnvelope<P>, kind: OperationKind): JsonObject {
	return checked(() => ({
		...businessParams(envelope.params),
		[idFields[readKind(kind)]]: operationId(envelope.operation_id)
	}))(undefined);
}

export const readRestartRequest = (value: unknown) => readCommandEnvelope(value, object({}));
export const readCancelRequest = checked((value) =>
	Object.keys(fields(value)).length === 0 ? {} : invalid()
);

export interface LookupQuery {
	instance_id: string;
	org_id?: string;
	office_id?: string;
}
// Instance, organization and office IDs are canonical UUID text: letters, digits and the hyphen.
const lookupId: Reader<string> = (value) =>
	/^[A-Za-z0-9-]{1,64}$/.test(text(value)) ? (value as string) : invalid();
const queryReader = object({
	instance_id: lookupId,
	org_id: optional(lookupId),
	office_id: optional(lookupId)
});

export function readLookupQuery(pairs: Iterable<readonly [string, string]>): LookupQuery {
	return checked(() => {
		const entries: [string, string][] = [];
		const keys = new Set<string>();
		for (const pair of pairs) {
			if (!Array.isArray(pair) || pair.length !== 2) return invalid();
			const key = text(pair[0]);
			if (!['instance_id', 'org_id', 'office_id'].includes(key) || keys.has(key)) return invalid();
			keys.add(key);
			entries.push([key, text(pair[1])]);
		}
		return queryReader(Object.fromEntries(entries));
	})(undefined);
}

export function lookupScope(
	query: LookupQuery,
	kind: OperationKind,
	sessionInstance: string
): Scope {
	return checked(() => {
		const normalized = queryReader(query);
		if (normalized.instance_id !== sessionInstance) return invalid();
		const { instance_id, org_id, office_id } = normalized;
		switch (readKind(kind)) {
			case 'host':
				if (org_id === undefined && office_id === undefined)
					return { kind: 'instance' as const, instance_id };
				break;
			case 'org':
				if (org_id !== undefined && office_id === undefined)
					return { kind: 'organization' as const, instance_id, org_id };
				break;
			case 'work':
			case 'msg':
				if (org_id !== undefined && office_id !== undefined)
					return { kind: 'office' as const, instance_id, org_id, office_id };
				break;
		}
		return invalid();
	})(undefined);
}

/** Raw single-segment gateway policy; this neither URL-normalizes nor authorizes an ID. */
export const validRawPathSegment = (segment: string): boolean =>
	typeof segment === 'string' &&
	segment.length > 0 &&
	!/[/\\]/.test(segment) &&
	!/%(?:2f|5c)/i.test(segment) &&
	!['.', '..'].includes(segment.replace(/%2e/gi, '.'));

export const sessionReason = word(['session_ended'] as const);
export const cancelState = word(['abandoned', 'linked', 'in_progress'] as const);
const credentialField = word(['login', 'password'] as const);
const credentialRule = word(['format', 'taken', 'too_short', 'too_long'] as const);
const gatewayErrorReader = object({
	code: text,
	message: text,
	retry_after_ms: optional(readSafeInteger),
	session_reason: optional(sessionReason),
	operation_id: optional(operationId),
	state: optional(cancelState),
	field: optional(credentialField),
	rule: optional(credentialRule)
});

export const readGatewayError = checked((value) => {
	const error = gatewayErrorReader(value);
	if (error.code === 'rate_limited' && error.retry_after_ms === undefined) return invalid();
	if ((error.field === undefined) !== (error.rule === undefined)) return invalid();
	return error;
});
export type GatewayError = ReturnType<typeof readGatewayError>;

export function gatewayErrorStatus(
	error: GatewayError,
	authenticated: boolean
): number | undefined {
	if (
		[
			'invalid_request',
			'unknown_method',
			'invalid_name',
			'cycle',
			'not_empty',
			'not_a_member',
			'owner_protected',
			'checklist_incomplete',
			'clock_skew',
			'unknown_recipient'
		].includes(error.code)
	)
		return 400;
	if (error.code === 'not_authorized') return authenticated ? 403 : 401;
	if (error.code === 'not_found') return 404;
	if (['conflict', 'already_exists', 'operation_reused', 'snapshot_required'].includes(error.code))
		return 409;
	if (error.code === 'expired') return 410;
	if (error.code === 'too_large') return 413;
	if (error.code === 'unsupported_version') return 426;
	if (error.code === 'rate_limited') return 429;
	if (['not_ready', 'unavailable', 'storage_error', 'timeout'].includes(error.code)) return 503;
	return undefined;
}

export function resourceError(access: 'missing' | 'hidden' | 'read_only'): GatewayError {
	return access === 'read_only'
		? { code: 'not_authorized', message: 'Not authorized.' }
		: { code: 'not_found', message: 'Not found.' };
}
