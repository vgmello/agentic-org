/** Runtime readers for the published HTTP contract, independent of routes and dispatch.
 * Canonical fixtures belong to org-proto; production code never imports them.
 * A decoding failure after dispatch says nothing about whether the operation committed.
 */
export const API_VERSION = 1;
export const CONTRACT_REVISION = 2;
export const SESSION_PROOF_HEADER = 'X-Org-Session-Proof';
export type JsonValue = null | boolean | number | string | JsonValue[] | JsonObject;
export interface JsonObject { [key: string]: JsonValue }
export type Reader<T> = (value: unknown) => T;
export class WireDecodeError extends Error {
	constructor() { super('Invalid gateway payload'); this.name = 'WireDecodeError'; }
}
const invalid = (): never => { throw new WireDecodeError(); };
const checked = <T>(reader: Reader<T>): Reader<T> => (value) => {
	try { return reader(value); } catch { throw new WireDecodeError(); }
};
const fields = (value: unknown): Record<string, unknown> => {
	if (!value || typeof value !== 'object' || Array.isArray(value) ||
		![Object.prototype, null].includes(Object.getPrototypeOf(value))) return invalid();
	return value as Record<string, unknown>;
};
const own = (value: object, key: string) => Object.hasOwn(value, key);
const field = (value: Record<string, unknown>, key: string): unknown => own(value, key) ? value[key] : undefined;
const text: Reader<string> = (value) => typeof value === 'string' ? value : invalid();
const bool: Reader<boolean> = (value) => typeof value === 'boolean' ? value : invalid();
const nonempty: Reader<string> = (value) => { const result = text(value); return result ? result : invalid(); };
export const readSafeInteger: Reader<number> = checked((value) =>
	typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? (value === 0 ? 0 : value) : invalid());
const u32: Reader<number> = (value) => { const result = readSafeInteger(value); return result <= 0xffff_ffff ? result : invalid(); };
const optional = <T>(reader: Reader<T>): Reader<T | undefined> => (value) => value == null ? undefined : reader(value);
const defaulted = <T>(reader: Reader<T>, fallback: () => T): Reader<T> => (value) => value === undefined ? fallback() : reader(value);
const array = <T>(reader: Reader<T>): Reader<T[]> => (value) => Array.isArray(value) ? value.map(reader) : invalid();
const word = <const T extends readonly string[]>(known: T): Reader<T[number] | 'unknown'> => (value) => {
	const name = text(value);
	return known.includes(name) ? name as T[number] : 'unknown';
};
const openWord = <const T extends readonly string[]>(known: T): Reader<T[number] | 'unknown'> => (value) =>
	typeof value === 'string' && known.includes(value) ? value as T[number] : 'unknown';
const omittedFalse: Reader<true | undefined> = (value) => value === undefined ? undefined : bool(value) || undefined;
type Shape = Record<string, Reader<unknown>>;
type Parsed<S extends Shape> =
	{ [K in keyof S as undefined extends ReturnType<S[K]> ? never : K]: ReturnType<S[K]> } &
	{ [K in keyof S as undefined extends ReturnType<S[K]> ? K : never]?: Exclude<ReturnType<S[K]>, undefined> };
function object<S extends Shape>(shape: S): Reader<Parsed<S>> {
	return (value) => {
		const input = fields(value);
		return Object.fromEntries(Object.entries(shape).flatMap(([key, reader]) => {
			const result = reader(field(input, key));
			return result === undefined ? [] : [[key, result]];
		})) as Parsed<S>;
	};
}
function json(value: unknown, depth = 0): JsonValue {
	if (depth > 128) return invalid();
	if (value === null || typeof value === 'string' || typeof value === 'boolean') return value;
	if (typeof value === 'number') return Number.isFinite(value) ? value : invalid();
	if (Array.isArray(value)) return value.map((item) => json(item, depth + 1));
	return Object.fromEntries(Object.entries(fields(value)).map(([key, item]) => [key, json(item, depth + 1)]));
}
const jsonObject: Reader<JsonObject> = (value) => { fields(value); return json(value) as JsonObject; };
export const readJsonObject = checked(jsonObject);
const operationId: Reader<string> = (value) => {
	const id = text(value);
	return /^[0-7][0-9A-HJKMNP-TV-Z]{25}$/.test(id) ? id : invalid();
};
export const readOperationId = checked(operationId);
export function operationTimeMs(value: unknown): number {
	const id = readOperationId(value);
	return [...id.slice(0, 10)].reduce((time, digit) => time * 32 + '0123456789ABCDEFGHJKMNPQRSTVWXYZ'.indexOf(digit), 0);
}
export const readVersion = checked(object({ api_version: u32, contract_revision: u32 }));
export type Version = ReturnType<typeof readVersion>;
export const versionCompatible = (version: Version, required: Version = { api_version: API_VERSION, contract_revision: CONTRACT_REVISION }) =>
	version.api_version === required.api_version && version.contract_revision >= required.contract_revision;

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
	if (kind === 'office') return { kind, instance_id, org_id, office_id: text(field(input, 'office_id')) };
	return invalid();
});
export const readResponseScope: Reader<ResponseScope> = checked((value) => {
	const input = fields(value);
	const kind = text(field(input, 'kind'));
	return ['instance', 'organization', 'office'].includes(kind) ? readScope(value) : { ...jsonObject(value), kind };
});
export function knownScope(scope: ResponseScope): Scope | undefined {
	try { return readScope(scope); } catch { return undefined; }
}
export const scopeMatches = (scope: Scope, target: Scope): boolean => JSON.stringify(readScope(scope)) === JSON.stringify(readScope(target));
export type OperationKind = 'host' | 'org' | 'work' | 'msg';
const readKind: Reader<OperationKind> = (value) => ['host', 'org', 'work', 'msg'].includes(text(value)) ? value as OperationKind : invalid();
const idFields = { host: 'operation_id', org: 'command_id', work: 'change_id', msg: 'id' } as const;
const businessParams: Reader<JsonObject> = (value) => {
	const params = fields(value);
	if (['operation_id', 'command_id', 'change_id', 'id', 'home', 'clear_home', 'app_token'].some((key) => own(params, key))) return invalid();
	if (own(params, 'profile') && own(fields(params.profile), 'home')) return invalid();
	return jsonObject(params);
};
export interface CommandEnvelope<P> { operation_id: string; scope: Scope; params: P }
export function readCommandEnvelope(value: unknown): CommandEnvelope<JsonObject>;
export function readCommandEnvelope<P>(value: unknown, params: Reader<P>): CommandEnvelope<P>;
export function readCommandEnvelope<P = JsonObject>(value: unknown, params: Reader<P> = readJsonObject as Reader<P>): CommandEnvelope<P> {
	return checked((value) => {
		const input = fields(value);
		return { operation_id: operationId(field(input, 'operation_id')), scope: readScope(field(input, 'scope')), params: params(businessParams(field(input, 'params'))) };
	})(value);
}
export function nativeParams<P>(envelope: CommandEnvelope<P>, kind: OperationKind): JsonObject {
	return checked(() => ({ ...businessParams(envelope.params), [idFields[readKind(kind)]]: operationId(envelope.operation_id) }))(undefined);
}
export const readCreateRequest = (value: unknown) => readCommandEnvelope(value, object({ name: text }));
export const readRestartRequest = (value: unknown) => readCommandEnvelope(value, object({}));
export const readCancelRequest = checked((value) => Object.keys(fields(value)).length === 0 ? {} : invalid());

export interface LookupQuery { instance_id: string; org_id?: string; office_id?: string }
const lookupId: Reader<string> = (value) => /^[A-Za-z0-9]{1,64}$/.test(text(value)) ? value as string : invalid();
const queryReader = object({ instance_id: lookupId, org_id: optional(lookupId), office_id: optional(lookupId) });
export function readLookupQuery(pairs: Iterable<readonly [string, string]>): LookupQuery {
	return checked(() => {
		const entries: [string, string][] = [];
		const keys = new Set<string>();
		for (const pair of pairs) {
			if (!Array.isArray(pair) || pair.length !== 2) return invalid();
			const key = text(pair[0]);
			if (!['instance_id', 'org_id', 'office_id'].includes(key) || keys.has(key)) return invalid();
			keys.add(key); entries.push([key, text(pair[1])]);
		}
		return queryReader(Object.fromEntries(entries));
	})(undefined);
}
export function lookupScope(query: LookupQuery, kind: OperationKind, sessionInstance: string): Scope {
	return checked(() => {
		const normalized = queryReader(query);
		if (normalized.instance_id !== sessionInstance) return invalid();
		const { instance_id, org_id, office_id } = normalized;
		switch (readKind(kind)) {
			case 'host': if (org_id === undefined && office_id === undefined) return { kind: 'instance' as const, instance_id }; break;
			case 'org': if (org_id !== undefined && office_id === undefined) return { kind: 'organization' as const, instance_id, org_id }; break;
			case 'work': case 'msg': if (org_id !== undefined && office_id !== undefined) return { kind: 'office' as const, instance_id, org_id, office_id }; break;
		}
		return invalid();
	})(undefined);
}
/** Raw single-segment gateway policy; this neither URL-normalizes nor authorizes an ID. */
export const validRawPathSegment = (segment: string): boolean => typeof segment === 'string' && segment.length > 0 &&
	!/[\/\\]/.test(segment) && !/%(?:2f|5c)/i.test(segment) && !['.', '..'].includes(segment.replace(/%2e/ig, '.'));

const sessionReason = word(['session_ended', 'hq_restarted'] as const);
const cancelState = word(['abandoned', 'linked', 'in_progress'] as const);
const gatewayErrorReader = object({
	code: text, message: text, retry_after_ms: optional(readSafeInteger), session_reason: optional(sessionReason),
	operation_id: optional(operationId), state: optional(cancelState)
});
export const readGatewayError = checked((value) => {
	const error = gatewayErrorReader(value);
	if (error.code === 'rate_limited' && error.retry_after_ms === undefined) return invalid();
	return error;
});
export type GatewayError = ReturnType<typeof readGatewayError>;
export function gatewayErrorStatus(error: GatewayError, authenticated: boolean): number | undefined {
	if (['invalid_request', 'unknown_method', 'invalid_name', 'cycle', 'not_empty', 'not_a_member', 'owner_protected', 'checklist_incomplete', 'clock_skew', 'unknown_recipient'].includes(error.code)) return 400;
	if (error.code === 'not_authorized') return authenticated ? 403 : 401;
	if (error.code === 'not_found') return 404;
	if (['conflict', 'already_exists', 'operation_reused', 'snapshot_required'].includes(error.code)) return 409;
	if (error.code === 'expired') return 410;
	if (error.code === 'too_large') return 413;
	if (error.code === 'unsupported_version') return 426;
	if (error.code === 'rate_limited') return 429;
	if (['not_ready', 'unavailable', 'storage_error', 'timeout'].includes(error.code)) return 503;
	return undefined;
}
export function resourceError(access: 'missing' | 'hidden' | 'read_only'): GatewayError {
	return access === 'read_only' ? { code: 'not_authorized', message: 'Not authorized.' } : { code: 'not_found', message: 'Not found.' };
}
export type ReadResponse<T> = { result: T } | { error: GatewayError };
export function readResponse<T>(value: unknown, result: Reader<T>): ReadResponse<T> {
	return checked((value) => {
		const input = fields(value);
		if (own(input, 'result') === own(input, 'error')) return invalid();
		if (own(input, 'error')) return { error: readGatewayError(input.error) };
		fields(input.result);
		return { result: result(input.result) };
	})(value);
}
export type MutationOutcome<T> =
	| { operation_id: string; outcome: 'accepted' | 'unknown' | 'unsupported' }
	| { operation_id: string; outcome: 'completed'; result: T }
	| { operation_id: string; outcome: 'rejected'; error: GatewayError };
export function readMutationOutcome(value: unknown): MutationOutcome<JsonObject>;
export function readMutationOutcome<T>(value: unknown, result: Reader<T>): MutationOutcome<T>;
export function readMutationOutcome<T = JsonObject>(value: unknown, result: Reader<T> = readJsonObject as Reader<T>): MutationOutcome<T> {
	return checked((value): MutationOutcome<T> => {
		const input = fields(value);
		const operation_id = operationId(field(input, 'operation_id'));
		const outcome = text(field(input, 'outcome'));
		const hasResult = own(input, 'result'), hasError = own(input, 'error');
		if (outcome === 'accepted' || outcome === 'unknown') {
			if (hasResult || hasError) return invalid();
			return { operation_id, outcome };
		}
		if (outcome === 'completed') {
			if (!hasResult || hasError) return invalid();
			fields(input.result);
			return { operation_id, outcome, result: result(input.result) };
		}
		if (outcome === 'rejected') {
			if (hasResult || !hasError) return invalid();
			const error = readGatewayError(input.error);
			return error.code === 'unknown' ? { operation_id, outcome: 'unknown' } : { operation_id, outcome, error };
		}
		return { operation_id, outcome: 'unsupported' };
	})(value);
}
export const outcomeFinal = (outcome: MutationOutcome<unknown>) => ['completed', 'rejected', 'unsupported'].includes(outcome.outcome);
export function outcomeStatus(outcome: MutationOutcome<unknown>, authenticated: boolean): number | undefined {
	switch (outcome.outcome) {
		case 'accepted': return 202;
		case 'completed': return 200;
		case 'unknown': return 504;
		case 'rejected': return gatewayErrorStatus(outcome.error, authenticated);
		case 'unsupported': return undefined;
	}
}
export type OperationLookup<T> =
	| (MutationOutcome<T> & { kind: OperationKind; state?: 'reserved' | 'linked' })
	| { operation_id: string; kind: 'host'; state: 'abandoned' };
export function readOperationLookup(value: unknown): OperationLookup<JsonObject>;
export function readOperationLookup<T>(value: unknown, result: Reader<T>): OperationLookup<T>;
export function readOperationLookup<T = JsonObject>(value: unknown, result: Reader<T> = readJsonObject as Reader<T>): OperationLookup<T> {
	return checked((value): OperationLookup<T> => {
		const input = fields(value);
		if (field(input, 'outcome') === 'unknown') return invalid();
		const operation_id = operationId(field(input, 'operation_id'));
		const kind = readKind(field(input, 'kind'));
		const hasState = own(input, 'state');
		if (hasState && kind !== 'host') return invalid();
		const state = hasState ? text(input.state) : undefined;
		if (state === 'abandoned') {
			if (['outcome', 'result', 'error'].some((key) => own(input, key))) return invalid();
			return { operation_id, kind: 'host', state };
		}
		if (state !== undefined && state !== 'reserved' && state !== 'linked') return { operation_id, kind, outcome: 'unsupported' };
		const outcome = readMutationOutcome(input, result);
		if (outcome.outcome === 'unsupported') return { operation_id, kind, outcome: 'unsupported' };
		if ((state === 'reserved' && outcome.outcome !== 'accepted') || (state === 'linked' && outcome.outcome !== 'completed') ||
			(state === undefined && kind === 'host' && outcome.outcome === 'accepted')) return invalid();
		return { ...outcome, kind, ...(state === undefined ? {} : { state }) };
	})(value);
}
export const lookupFinal = (lookup: OperationLookup<unknown>): boolean =>
	('state' in lookup && lookup.state === 'abandoned') || ('outcome' in lookup && outcomeFinal(lookup));

const privateProjectionKeys = new Set(['endpoint', 'socket', 'path', 'recovery_code', 'db_key', 'owner_grant', 'host_control']);
function publicProjection(value: unknown, depth = 0): void {
	if (depth > 128) return invalid();
	if (Array.isArray(value)) { for (const item of value) publicProjection(item, depth + 1); }
	else if (value !== null && typeof value === 'object') {
		for (const [key, item] of Object.entries(fields(value))) {
			if (privateProjectionKeys.has(key) || key.endsWith('_path')) return invalid();
			publicProjection(item, depth + 1);
		}
	}
}
const organizationReader = object({ id: text, name: text, created_at: text, recovery: word(['pending', 'delivered', 'acknowledged'] as const) });
export const readOrganization = checked((value) => { fields(value); publicProjection(value); return organizationReader(value); });
export const readRestartResult = checked((value) => { fields(value); publicProjection(value); return {}; });
export const readCancelResult = checked((value) => {
	const result = object({ operation_id: operationId, state: cancelState })(value);
	return result.state === 'in_progress' ? invalid() : result;
});
export const cancelStatus = (result: ReturnType<typeof readCancelResult>): number | undefined => ['abandoned', 'linked'].includes(result.state) ? 200 : undefined;
export const readInstance = checked(object({
	instance_id: text, name: text, server_version: text, gateway_status: word(['ok', 'degraded'] as const),
	listen_scheme: word(['https', 'http_loopback'] as const), tls: word(['ok', 'missing', 'expiring'] as const), server_time_ms: readSafeInteger
}));
export const readOffice = checked(object({
	id: text, organization_id: text, name: text, role: word(['hq', 'satellite'] as const), host: text,
	status: word(['active', 'starting', 'pending', 'offline', 'unavailable', 'update_needed'] as const),
	status_reason: optional(word(['key_missing', 'in_use', 'storage_error', 'invalid_request', 'unsupported_version', 'crashed', 'start_timeout', 'restarts_exhausted', 'stopped_by_owner'] as const)),
	next_restart_at: optional(text)
}));
export const readOrganizationList = checked(object({ organizations: array(readOrganization) }));
export const readOfficeList = checked(object({ offices: array(readOffice) }));
const capabilityReader = object({ name: text, scope: readResponseScope, available: defaulted(bool, () => false), reason: optional(text) });
export type Capability = ReturnType<typeof capabilityReader>;
export const readCapabilities = checked(object({ api_version: u32, contract_revision: u32, instance_id: text, operations: array(capabilityReader), feeds: array(capabilityReader) }));
export const capabilityAvailable = (capability: Capability, implementedNames: readonly string[]): boolean =>
	capability.available && knownScope(capability.scope) !== undefined && implementedNames.includes(capability.name);
export const GATEWAY_OPERATION_NAMES = ['version', 'session.sign_in', 'session.get', 'session.refresh', 'session.sign_out', 'capabilities', 'instance.get', 'organization.list', 'organization.create', 'organization.cancel', 'office.list', 'org.snapshot', 'office.restart', 'operation.get', 'events', 'events.poll'] as const;

const accountReader = object({ id: text, login: text });
const linkReader = object({ org_id: text, member_id: text, office_ids: array(text) });
const sessionFields = {
	instance_id: text, account: accountReader, admin: bool, organizations: array(linkReader), server_time_ms: readSafeInteger,
	absolute_expires_at: text, idle_expires_at: text
};
export const readSignIn = checked(object({ login: text, password: nonempty }));
export const readSession = checked(object(sessionFields));
const proof: Reader<string> = (value) => /^[a-f0-9]{64}$/.test(text(value)) ? value as string : invalid();
export const readSessionProof = checked(proof);
export const readEstablishedSession = checked(object({ ...sessionFields, proof }));

// Reused org-proto domain records. Read defaults/unknown variants match the native DTOs;
// versions and positions must remain exactly representable by this JavaScript reader.
export const readPosition = checked(object({ epoch: readSafeInteger, seq: readSafeInteger }));
const teamReader = object({ id: text, name: text, owner_team_id: optional(text), lead_member_id: optional(text), version: readSafeInteger });
const runtime: Reader<'claude_code' | 'codex' | 'antigravity' | 'unknown' | { other: string }> = (value) => {
	if (typeof value === 'string') return openWord(['claude_code', 'codex', 'antigravity'] as const)(value);
	if (value && typeof value === 'object' && !Array.isArray(value) && typeof field(fields(value), 'other') === 'string') return { other: text(field(fields(value), 'other')) };
	return 'unknown';
};
const scheduleReader = object({ spec: defaulted(text, () => ''), time_zone: defaulted(text, () => '') });
const agentReader = object({
	role: text, title: text, focus: optional(text), runtime, model: optional(text),
	run_mode: optional(openWord(['always_on', 'on_call', 'scheduled'] as const)), schedule: optional(scheduleReader), home: optional(text),
	messaging_name: text, status: openWord(['active', 'paused', 'offboarded'] as const), offboarded_at: optional(text)
});
const memberReader = object({
	id: text, kind: word(['agent', 'person'] as const), name: text, contact: optional(text), owner: omittedFalse,
	office_id: optional(text), agent: optional(agentReader), reports_to_member_id: optional(text), created_at: optional(text), version: readSafeInteger
});
const membershipReader = object({ id: text, team_id: text, member_id: text, title_in_team: optional(text), version: readSafeInteger });
const projectTeamReader = object({ id: text, project_id: text, team_id: text, version: readSafeInteger });
const githubReader = object({ owner: text, name: text, repo_id: readSafeInteger, private: defaulted(bool, () => false) });
const repoReader = object({ host: openWord(['local', 'github'] as const), default_branch: text, path: optional(text), github: optional(githubReader) });
const projectReader = object({
	id: text, key: defaulted(text, () => ''), name: text, description: optional(text), repo: optional(repoReader),
	integration_branch: optional(text), archived: omittedFalse, created_at: optional(text), version: defaulted(readSafeInteger, () => 0)
});
const areaReader = object({
	id: text, project_id: text, name: text, team_id: optional(text), lead_member_id: optional(text), general: omittedFalse,
	deleted: omittedFalse, replaced_by: optional(text), version: readSafeInteger
});
export const readOrgSnapshot = checked(object({
	position: readPosition, teams: array(teamReader), members: array(memberReader), memberships: array(membershipReader),
	projects: defaulted(array(projectReader), () => []), project_teams: defaulted(array(projectTeamReader), () => []), areas: defaulted(array(areaReader), () => [])
}));
const cursor = nonempty;
export const readSnapshot = checked(object({ snapshot: readOrgSnapshot, resume_cursor: cursor }));
const feed = word(['instance_status', 'organization', 'work', 'messaging', 'presence'] as const);
const subscriptionReader = object({ scope: readScope, feed, cursor: optional(cursor) });
export const readStreamRequest = checked(object({ subscriptions: array(subscriptionReader) }));
const subscriptionCursorReader = object({ scope: readResponseScope, feed, cursor });
const readyReader = object({ subscriptions: array(subscriptionCursorReader) });
const changeReader = object({ scope: readResponseScope, feed, cursor, event: text, data: (value: unknown) => json(value) });
const resyncReader = object({ scope: readResponseScope, feed, reason: word(['snapshot_required', 'slow_consumer'] as const) });
const unavailableReader = object({ scope: readResponseScope, feed, error: readGatewayError });
const heartbeatReader = object({ server_time_ms: readSafeInteger });
const endedReader = object({ reason: optional(sessionReason) });
export type EventFrame =
	| ({ type: 'ready' } & ReturnType<typeof readyReader>)
	| ({ type: 'change' } & ReturnType<typeof changeReader>)
	| ({ type: 'resync' } & ReturnType<typeof resyncReader>)
	| ({ type: 'unavailable' } & ReturnType<typeof unavailableReader>)
	| ({ type: 'heartbeat' } & ReturnType<typeof heartbeatReader>)
	| ({ type: 'session_ended' } & ReturnType<typeof endedReader>)
	| { type: 'unsupported' };
export const readEventFrame: Reader<EventFrame> = checked((value): EventFrame => {
	const input = fields(value);
	const type = text(field(input, 'type'));
	const unsupported = { type: 'unsupported' as const };
	switch (type) {
		case 'ready': { const frame = readyReader(input); return frame.subscriptions.some((s) => !knownScope(s.scope)) ? unsupported : { type, ...frame }; }
		case 'change': { const frame = changeReader(input); return knownScope(frame.scope) ? { type, ...frame } : unsupported; }
		case 'resync': { const frame = resyncReader(input); return knownScope(frame.scope) ? { type, ...frame } : unsupported; }
		case 'unavailable': { const frame = unavailableReader(input); return knownScope(frame.scope) ? { type, ...frame } : unsupported; }
		case 'heartbeat': return { type, ...heartbeatReader(input) };
		case 'session_ended': return { type, ...endedReader(input) };
		default: return unsupported;
	}
});
export const readPollResponse = checked(object({ frames: array(readEventFrame), retry_after_ms: readSafeInteger }));
