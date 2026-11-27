/** Domain-level gateway payload readers (access, TLS, offices, sessions, org snapshots, events). */
import {
	array,
	bool,
	cancelState,
	checked,
	defaulted,
	field,
	fields,
	invalid,
	json,
	knownScope,
	nonempty,
	object,
	omittedFalse,
	openWord,
	operationId,
	optional,
	readGatewayError,
	readResponseScope,
	readSafeInteger,
	readScope,
	sessionReason,
	text,
	u32,
	word,
	type Reader
} from './wire-primitives';

const privateProjectionKeys = new Set([
	'endpoint',
	'socket',
	'path',
	'recovery_code',
	'db_key',
	'owner_grant',
	'host_control'
]);

function publicProjection(value: unknown, depth = 0): void {
	if (depth > 128) return invalid();
	if (Array.isArray(value)) {
		for (const item of value) publicProjection(item, depth + 1);
	} else if (value !== null && typeof value === 'object') {
		for (const [key, item] of Object.entries(fields(value))) {
			if (privateProjectionKeys.has(key) || key.endsWith('_path')) return invalid();
			publicProjection(item, depth + 1);
		}
	}
}

export const readRestartResult = checked((value) => {
	fields(value);
	publicProjection(value);
	return {};
});
export const readCancelResult = checked((value) => {
	const result = object({ operation_id: operationId, state: cancelState })(value);
	return result.state === 'in_progress' ? invalid() : result;
});
export const cancelStatus = (result: ReturnType<typeof readCancelResult>): number | undefined =>
	['abandoned', 'linked'].includes(result.state) ? 200 : undefined;

const listenScheme = word(['https', 'http_loopback', 'http_published'] as const);
export type ListenScheme = ReturnType<typeof listenScheme>;
export const readAccess = checked(
	object({
		auth_enabled: bool,
		listen_scheme: listenScheme
	})
);
export type AccessView = ReturnType<typeof readAccess>;

export type HstsMode = 'off' | 'on' | 'clear';
const strictHsts: Reader<HstsMode> = (value) => {
	const name = text(value);
	return name === 'off' || name === 'on' || name === 'clear' ? name : invalid();
};
export type TlsEnableRequest =
	| { mode: 'self_signed'; hsts?: HstsMode }
	| { mode: 'supplied'; cert: string; key: string; hsts?: HstsMode };

export const readTlsEnableRequest: Reader<TlsEnableRequest> = checked((value) => {
	const input = fields(value);
	const keys = Object.keys(input);
	const mode = text(field(input, 'mode'));
	const hsts = optional(strictHsts)(field(input, 'hsts'));
	if (mode === 'self_signed') {
		if (keys.some((key) => key !== 'mode' && key !== 'hsts')) return invalid();
		return hsts === undefined ? { mode } : { mode, hsts };
	}
	if (mode === 'supplied') {
		if (keys.some((key) => !['mode', 'cert', 'key', 'hsts'].includes(key))) return invalid();
		const cert = nonempty(field(input, 'cert'));
		const key = nonempty(field(input, 'key'));
		return hsts === undefined ? { mode, cert, key } : { mode, cert, key, hsts };
	}
	return invalid();
});

export const readTlsEnableResult = checked(
	object({
		scheme: listenScheme,
		origin: nonempty,
		fingerprint: nonempty,
		source: word(['made', 'supplied'] as const),
		restart_required: bool
	})
);
export type TlsEnableResult = ReturnType<typeof readTlsEnableResult>;

export const readCliApprovalDecide = checked(object({ link: nonempty, approve: bool }));
export type CliApprovalDecide = ReturnType<typeof readCliApprovalDecide>;
export const readCliApprovalDecision = checked(
	object({
		status: word(['approved', 'declined'] as const)
	})
);
export type CliApprovalDecision = ReturnType<typeof readCliApprovalDecision>;

export const readInstance = checked(
	object({
		instance_id: text,
		name: text,
		server_version: text,
		web_status: word(['ok', 'degraded'] as const),
		listen_scheme: listenScheme,
		auth_enabled: optional(bool),
		tls: optional(word(['ok', 'missing', 'expiring'] as const)),
		server_time_ms: readSafeInteger
	})
);

export const readOffice = checked(
	object({
		id: text,
		organization_id: text,
		name: text,
		role: word(['hq', 'satellite'] as const),
		host: text,
		status: word([
			'active',
			'starting',
			'pending',
			'offline',
			'unavailable',
			'update_needed'
		] as const),
		status_reason: optional(
			word([
				'in_use',
				'storage_error',
				'invalid_request',
				'unsupported_version',
				'crashed',
				'start_timeout',
				'restarts_exhausted',
				'stopped_by_owner'
			] as const)
		),
		next_restart_at: optional(text)
	})
);
export const readOfficeList = checked(object({ offices: array(readOffice) }));

const capabilityReader = object({
	name: text,
	scope: readResponseScope,
	available: defaulted(bool, () => false),
	reason: optional(text)
});
export type Capability = ReturnType<typeof capabilityReader>;
export const readCapabilities = checked(
	object({
		api_version: u32,
		contract_revision: u32,
		instance_id: text,
		operations: array(capabilityReader),
		feeds: array(capabilityReader)
	})
);
export const capabilityAvailable = (
	capability: Capability,
	implementedNames: readonly string[]
): boolean =>
	capability.available &&
	knownScope(capability.scope) !== undefined &&
	implementedNames.includes(capability.name);

export const GATEWAY_OPERATION_NAMES = [
	'version',
	'session.sign_in',
	'session.get',
	'session.refresh',
	'session.sign_out',
	'capabilities',
	'instance.get',
	'organization.list',
	'organization.create',
	'organization.cancel',
	'office.list',
	'org.snapshot',
	'office.restart',
	'operation.get',
	'events',
	'events.poll'
] as const;

const accountReader = object({ id: text, login: text, time_zone: optional(text) });
const linkReader = object({ org_id: text, employee_id: text, office_ids: array(text) });
const sessionFields = {
	instance_id: text,
	account: accountReader,
	admin: bool,
	organizations: array(linkReader),
	server_time_ms: readSafeInteger,
	idle_expires_at: text
};
export const readSignIn = checked(object({ login: text, password: nonempty }));
/** Registration redeems the fragment secret of a one-time link; the secret travels only in this JSON body. */
export const readRegister = checked(object({ link: nonempty, login: text, password: nonempty }));
export const readSession = checked(object(sessionFields));
const proof: Reader<string> = (value) =>
	/^[a-f0-9]{64}$/.test(text(value)) ? (value as string) : invalid();
export const readSessionProof = checked(proof);
export const readEstablishedSession = checked(object({ ...sessionFields, proof }));

// Reused org-proto domain records. Read defaults/unknown variants match the native DTOs;
// versions and positions must remain exactly representable by this JavaScript reader.
export const readPosition = checked(object({ epoch: readSafeInteger, seq: readSafeInteger }));
const teamReader = object({
	id: text,
	name: text,
	owner_team_id: optional(text),
	lead_member_id: optional(text),
	version: readSafeInteger
});
const runtime: Reader<'claude_code' | 'codex' | 'antigravity' | 'unknown' | { other: string }> = (
	value
) => {
	if (typeof value === 'string')
		return openWord(['claude_code', 'codex', 'antigravity'] as const)(value);
	if (
		value &&
		typeof value === 'object' &&
		!Array.isArray(value) &&
		typeof field(fields(value), 'other') === 'string'
	)
		return { other: text(field(fields(value), 'other')) };
	return 'unknown';
};
const scheduleReader = object({
	spec: defaulted(text, () => ''),
	time_zone: defaulted(text, () => '')
});
const agentReader = object({
	role: text,
	title: text,
	focus: optional(text),
	runtime,
	model: optional(text),
	run_mode: optional(openWord(['always_on', 'on_call', 'scheduled'] as const)),
	schedule: optional(scheduleReader),
	home: optional(text),
	messaging_name: text,
	status: openWord(['active', 'paused', 'offboarded'] as const),
	offboarded_at: optional(text)
});
const memberReader = object({
	id: text,
	kind: (value) => openWord(['agent', 'person'] as const)(text(value)),
	name: text,
	handle: optional(text),
	home_team_id: optional(text),
	role_id: optional(operationId),
	title_in_team: optional(text),
	contact: optional(text),
	owner: omittedFalse,
	office_id: optional(text),
	agent: optional(agentReader),
	builtin_key: optional(text),
	reports_to_member_id: optional(text),
	created_at: optional(text),
	version: readSafeInteger
});
const projectTeamReader = object({
	id: text,
	project_id: text,
	team_id: text,
	version: readSafeInteger
});
const githubReader = object({
	owner: text,
	name: text,
	repo_id: readSafeInteger,
	private: defaulted(bool, () => false)
});
const repoReader = object({
	host: openWord(['local', 'github'] as const),
	default_branch: text,
	path: optional(text),
	github: optional(githubReader)
});
const projectReader = object({
	id: text,
	key: defaulted(text, () => ''),
	name: text,
	description: optional(text),
	repo: optional(repoReader),
	integration_branch: optional(text),
	archived: omittedFalse,
	created_at: optional(text),
	version: defaulted(readSafeInteger, () => 0)
});

export const readOrgSnapshot = checked(
	object({
		position: readPosition,
		teams: array(teamReader),
		members: array(memberReader),
		projects: defaulted(array(projectReader), () => []),
		project_teams: defaulted(array(projectTeamReader), () => [])
	})
);
const cursor = nonempty;
export const readSnapshot = checked(object({ snapshot: readOrgSnapshot, resume_cursor: cursor }));

const feed = word(['instance_status', 'organization', 'work', 'messaging', 'presence'] as const);
const subscriptionReader = object({ scope: readScope, feed, cursor: optional(cursor) });
export const readStreamRequest = checked(object({ subscriptions: array(subscriptionReader) }));
const subscriptionCursorReader = object({ scope: readResponseScope, feed, cursor });
const readyReader = object({ subscriptions: array(subscriptionCursorReader) });
const changeReader = object({
	scope: readResponseScope,
	feed,
	cursor,
	event: text,
	data: (value: unknown) => json(value)
});
const resyncReader = object({
	scope: readResponseScope,
	feed,
	reason: word(['snapshot_required', 'slow_consumer'] as const)
});
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

const scopedEventReader = (
	type: 'change' | 'resync' | 'unavailable',
	input: Record<string, unknown>
): EventFrame => {
	const frame =
		type === 'change'
			? changeReader(input)
			: type === 'resync'
				? resyncReader(input)
				: unavailableReader(input);
	return knownScope(frame.scope) ? ({ type, ...frame } as EventFrame) : { type: 'unsupported' };
};

export const readEventFrame: Reader<EventFrame> = checked((value): EventFrame => {
	const input = fields(value);
	const type = text(field(input, 'type'));
	if (type === 'ready') {
		const frame = readyReader(input);
		return frame.subscriptions.some((s) => !knownScope(s.scope))
			? { type: 'unsupported' }
			: { type, ...frame };
	}
	if (type === 'change' || type === 'resync' || type === 'unavailable') {
		return scopedEventReader(type, input);
	}
	if (type === 'heartbeat') return { type, ...heartbeatReader(input) };
	if (type === 'session_ended') return { type, ...endedReader(input) };
	return { type: 'unsupported' };
});

export const readPollResponse = checked(
	object({ frames: array(readEventFrame), retry_after_ms: readSafeInteger })
);
