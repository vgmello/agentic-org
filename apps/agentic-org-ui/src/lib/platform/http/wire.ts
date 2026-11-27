/** Runtime readers for the published HTTP contract, independent of routes and dispatch.
 * Canonical fixtures belong to org-proto; production code never imports them.
 * A decoding failure after dispatch says nothing about whether the operation committed.
 */
import {
	checked,
	field,
	fields,
	gatewayErrorStatus,
	invalid,
	operationId,
	own,
	readGatewayError,
	readJsonObject,
	readKind,
	text,
	type GatewayError,
	type JsonObject,
	type OperationKind,
	type Reader
} from './wire-primitives';

export const SESSION_PROOF_HEADER = 'X-Org-Session-Proof';

export {
	API_VERSION,
	CONTRACT_REVISION,
	WireDecodeError,
	gatewayErrorStatus,
	knownScope,
	lookupScope,
	nativeParams,
	operationTimeMs,
	readCancelRequest,
	readCommandEnvelope,
	readGatewayError,
	readJsonObject,
	readLookupQuery,
	readOperationId,
	readResponseScope,
	readRestartRequest,
	readSafeInteger,
	readScope,
	readVersion,
	resourceError,
	scopeMatches,
	validRawPathSegment,
	versionCompatible,
	type CommandEnvelope,
	type GatewayError,
	type JsonObject,
	type JsonValue,
	type LookupQuery,
	type OperationKind,
	type Reader,
	type ResponseScope,
	type Scope,
	type Version
} from './wire-primitives';

export {
	GATEWAY_OPERATION_NAMES,
	cancelStatus,
	capabilityAvailable,
	readAccess,
	readCancelResult,
	readCapabilities,
	readCliApprovalDecide,
	readCliApprovalDecision,
	readEstablishedSession,
	readEventFrame,
	readInstance,
	readOffice,
	readOfficeList,
	readOrgSnapshot,
	readPollResponse,
	readPosition,
	readRegister,
	readRestartResult,
	readSession,
	readSessionProof,
	readSignIn,
	readSnapshot,
	readStreamRequest,
	readTlsEnableRequest,
	readTlsEnableResult,
	type AccessView,
	type Capability,
	type CliApprovalDecide,
	type CliApprovalDecision,
	type EventFrame,
	type HstsMode,
	type ListenScheme,
	type TlsEnableRequest,
	type TlsEnableResult
} from './wire-domain';

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
export function readMutationOutcome<T = JsonObject>(
	value: unknown,
	result: Reader<T> = readJsonObject as Reader<T>
): MutationOutcome<T> {
	return checked((value): MutationOutcome<T> => {
		const input = fields(value);
		const operation_id = operationId(field(input, 'operation_id'));
		const outcome = text(field(input, 'outcome'));
		const hasResult = own(input, 'result');
		const hasError = own(input, 'error');
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
			return error.code === 'unknown'
				? { operation_id, outcome: 'unknown' }
				: { operation_id, outcome, error };
		}
		return { operation_id, outcome: 'unsupported' };
	})(value);
}

export const outcomeFinal = (outcome: MutationOutcome<unknown>) =>
	['completed', 'rejected', 'unsupported'].includes(outcome.outcome);

export function outcomeStatus(
	outcome: MutationOutcome<unknown>,
	authenticated: boolean
): number | undefined {
	switch (outcome.outcome) {
		case 'accepted':
			return 202;
		case 'completed':
			return 200;
		case 'unknown':
			return 504;
		case 'rejected':
			return gatewayErrorStatus(outcome.error, authenticated);
		case 'unsupported':
			return undefined;
	}
}

export type OperationLookup<T> =
	| (MutationOutcome<T> & { kind: OperationKind; state?: 'reserved' | 'linked' })
	| { operation_id: string; kind: 'host'; state: 'abandoned' };

export function readOperationLookup(value: unknown): OperationLookup<JsonObject>;
export function readOperationLookup<T>(value: unknown, result: Reader<T>): OperationLookup<T>;
export function readOperationLookup<T = JsonObject>(
	value: unknown,
	result: Reader<T> = readJsonObject as Reader<T>
): OperationLookup<T> {
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
		if (state !== undefined && state !== 'reserved' && state !== 'linked')
			return { operation_id, kind, outcome: 'unsupported' };
		const outcome = readMutationOutcome(input, result);
		if (outcome.outcome === 'unsupported') return { operation_id, kind, outcome: 'unsupported' };
		const invalidState =
			(state === 'reserved' && outcome.outcome !== 'accepted') ||
			(state === 'linked' && outcome.outcome !== 'completed') ||
			(state === undefined && kind === 'host' && outcome.outcome === 'accepted');
		if (invalidState) return invalid();
		return { ...outcome, kind, ...(state === undefined ? {} : { state }) };
	})(value);
}

export const lookupFinal = (lookup: OperationLookup<unknown>): boolean =>
	('state' in lookup && lookup.state === 'abandoned') ||
	('outcome' in lookup && outcomeFinal(lookup));
