import {
	API_ERROR_CODES,
	type ApiErrorCode,
	type AppError,
	type Connection,
	type HelloResult,
	type OfficeInfo,
	type Organization
} from '$lib/proto';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { formatUuidV7 } from '$lib/platform/http/clock';
import { readJsonObject, readOperationId, readResponse } from '$lib/platform/http/wire';
import {
	readOrganizationsList,
	type OfficeView,
	type OrganizationView
} from '$lib/platform/organizations/readers';
import type { OfficeApi } from './office-api';

type Reply = { status: number; body: unknown };

export interface OfficeHttpTransport {
	read(path: string, signal?: AbortSignal): Promise<Reply>;
	submit(path: string, body: unknown, signal?: AbortSignal): Promise<Reply>;
}

const KNOWN_CODES: ReadonlySet<string> = new Set(API_ERROR_CODES);

function asApiCode(code: unknown): ApiErrorCode {
	return typeof code === 'string' && KNOWN_CODES.has(code) ? (code as ApiErrorCode) : 'unknown';
}

function readGatewayError(
	body: unknown,
	fallbackCode: ApiErrorCode,
	fallbackMessage: string
): AppError {
	const root = body && typeof body === 'object' ? (body as Record<string, unknown>) : undefined;
	const err =
		root?.error && typeof root.error === 'object'
			? (root.error as Record<string, unknown>)
			: undefined;
	return {
		source: 'api',
		code: err ? asApiCode(err.code) : fallbackCode,
		message: typeof err?.message === 'string' && err.message ? err.message : fallbackMessage
	};
}

function statusFallback(status: number): ApiErrorCode {
	if (status === 401 || status === 403) return 'not_authorized';
	if (status === 404) return 'not_found';
	if (status === 409) return 'conflict';
	if (status === 410) return 'expired';
	return 'unavailable';
}

function mapOffice(view: OfficeView): OfficeInfo {
	return {
		id: view.id,
		organization_id: view.organization_id,
		name: view.name,
		role: view.role,
		host: view.host,
		...(view.status ? { status: view.status } : {}),
		...(view.status_reason ? { status_reason: view.status_reason } : {}),
		...(view.next_restart_at ? { next_restart_at: view.next_restart_at } : {})
	};
}

function mapOrganization(view: OrganizationView): Organization {
	return {
		id: view.id,
		name: view.name,
		created_at: view.created_at,
		offices: view.offices.map(mapOffice)
	};
}

function toConnection(org: Organization): Connection {
	const hq = org.offices.find((o) => o.role === 'hq') ?? org.offices[0];
	return {
		organization: org,
		office: hq ?? {
			id: org.id,
			organization_id: org.id,
			name: 'HQ',
			role: 'hq',
			host: 'This server',
			status: 'unknown'
		}
	};
}

const GATEWAY_CAPABILITIES = ['office.status_changed', 'office.restart'];

const defaultNewId = () =>
	formatUuidV7(Date.now(), globalThis.crypto.getRandomValues(new Uint8Array(10)));

function parseInstanceHello(body: unknown): HelloResult {
	const envelope = readResponse(body, readJsonObject);
	if ('error' in envelope) {
		throw readGatewayError(body, 'unavailable', 'The host could not be reached.');
	}
	const result = envelope.result;
	if (result.web_status !== 'ok') {
		throw {
			source: 'api',
			code: 'unavailable',
			message: 'The host is degraded.'
		} satisfies AppError;
	}
	if (typeof result.server_version !== 'string' || typeof result.name !== 'string') {
		throw {
			source: 'api',
			code: 'unavailable',
			message: 'The host status response could not be read.'
		} satisfies AppError;
	}
	return {
		api_version: 1,
		server_version: result.server_version,
		host: result.name,
		capabilities: GATEWAY_CAPABILITIES
	};
}

/**
 * Production HTTP adapter for office/organization host gateway routes (`/api/v1/organizations`
 * and `/api/v1/organizations/{org}/offices/{office}/restart`). Never falls back to mocks.
 */
export function createHttpOfficeApi({
	http,
	instanceId,
	newId = defaultNewId,
	getSelectedOrganizationId,
	selectOrganization
}: {
	http: OfficeHttpTransport;
	instanceId: () => string | undefined;
	newId?: () => string;
	getSelectedOrganizationId: () => string | undefined;
	selectOrganization: (organizationId: string) => void;
}): OfficeApi {
	async function fetchOrganizations(): Promise<Organization[]> {
		let reply: Reply;
		try {
			reply = await http.read('/api/v1/organizations');
		} catch {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'The host could not be reached.'
			} satisfies AppError;
		}
		if (reply.status !== 200) {
			throw readGatewayError(
				reply.body,
				statusFallback(reply.status),
				'The organizations could not be loaded.'
			);
		}
		try {
			const envelope = readResponse(reply.body, readJsonObject);
			if ('error' in envelope) {
				throw readGatewayError(reply.body, 'unavailable', 'The organizations could not be loaded.');
			}
			const parsed = readOrganizationsList(envelope.result);
			return parsed.organizations.map(mapOrganization);
		} catch (e) {
			if (e && typeof e === 'object' && 'source' in e) throw e;
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'The organizations response could not be read.'
			} satisfies AppError;
		}
	}

	async function connections(): Promise<Connection[]> {
		const orgs = await fetchOrganizations();
		if (orgs.length === 0) return [];
		const selectedId = getSelectedOrganizationId();
		const ordered = selectedId
			? [...orgs.filter((o) => o.id !== selectedId), ...orgs.filter((o) => o.id === selectedId)]
			: orgs;
		return ordered.map(toConnection);
	}

	async function restartOffice(officeId: string): Promise<void> {
		const instance = instanceId();
		if (!instance) {
			throw {
				source: 'api',
				code: 'not_authorized',
				message: 'Sign in to continue.'
			} satisfies AppError;
		}
		const orgs = await fetchOrganizations();
		const org = orgs.find((o) => o.offices.some((ofc) => ofc.id === officeId));
		if (!org) {
			throw {
				source: 'api',
				code: 'not_found',
				message: 'That office could not be found.'
			} satisfies AppError;
		}
		const operationId = readOperationId(newId());
		let reply: Reply;
		try {
			reply = await http.submit(
				`/api/v1/organizations/${encodeURIComponent(org.id)}/offices/${encodeURIComponent(officeId)}/restart`,
				{
					operation_id: operationId,
					scope: { kind: 'office', instance_id: instance, org_id: org.id, office_id: officeId },
					params: {}
				}
			);
		} catch {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'The restart request could not be confirmed.'
			} satisfies AppError;
		}
		const root =
			reply.body && typeof reply.body === 'object'
				? (reply.body as Record<string, unknown>)
				: undefined;
		if (reply.status === 200 && root?.outcome === 'completed') return;
		if (reply.status === 504 || root?.outcome === 'unknown') {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'The outcome of this restart request is not confirmed yet.'
			} satisfies AppError;
		}
		throw readGatewayError(
			reply.body,
			statusFallback(reply.status),
			'The office could not be restarted.'
		);
	}

	async function ping(): Promise<HelloResult> {
		let reply: Reply;
		try {
			reply = await http.read('/api/v1/instance');
		} catch {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'The host could not be reached.'
			} satisfies AppError;
		}
		if (reply.status !== 200) {
			throw readGatewayError(
				reply.body,
				statusFallback(reply.status),
				'The host could not be reached.'
			);
		}
		try {
			return parseInstanceHello(reply.body);
		} catch (e) {
			if (e && typeof e === 'object' && 'source' in e) throw e;
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'The host status response could not be read.'
			} satisfies AppError;
		}
	}

	return Object.freeze({
		connections,
		ensureRunning: ping,
		ping,
		peekOrganizations: fetchOrganizations,
		listOrganizations: fetchOrganizations,
		async openOrganization(id: string): Promise<Connection> {
			const orgs = await fetchOrganizations();
			const org = orgs.find((o) => o.id === id);
			if (!org) {
				throw {
					source: 'api',
					code: 'not_found',
					message: 'That organization could not be found.'
				} satisfies AppError;
			}
			selectOrganization(org.id);
			return toConnection(org);
		},
		restartOffice,
		createOrganization: unavailableCall,
		onOfficeStatus: async () => () => {}
	});
}
