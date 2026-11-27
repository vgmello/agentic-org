import type {
	AppError,
	OfficeInfo,
	OfficeRole,
	OfficeStatus,
	OfficeStatusReason
} from '$lib/proto';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import type { OfficesOverview, SatelliteDetails } from './types';

import { asRec, asStr, type Rec } from '$lib/platform/http/decode';

const VALID_ROLES = new Set<OfficeRole>(['hq', 'satellite', 'unknown']);
const VALID_STATUSES = new Set<OfficeStatus>([
	'active',
	'starting',
	'pending',
	'offline',
	'unavailable',
	'update_needed',
	'unknown'
]);
const VALID_REASONS = new Set<OfficeStatusReason>([
	'in_use',
	'storage_error',
	'invalid_request',
	'unsupported_version',
	'crashed',
	'start_timeout',
	'restarts_exhausted',
	'stopped_by_owner',
	'unknown'
]);

function asEnum<T extends string>(v: unknown, valid: ReadonlySet<T>): T | undefined {
	const str = asStr(v);
	return str && valid.has(str as T) ? (str as T) : undefined;
}

function decodeOfficeOptionalFields(r: Rec): Partial<OfficeInfo> {
	const extra: Partial<OfficeInfo> = {};
	const status = asEnum(r.status, VALID_STATUSES);
	const statusReason = asEnum(r.status_reason, VALID_REASONS);
	const nextRestartAt = asStr(r.next_restart_at);
	if (status) extra.status = status;
	if (statusReason) extra.status_reason = statusReason;
	if (nextRestartAt) extra.next_restart_at = nextRestartAt;
	return extra;
}

function decodeOfficeInfo(raw: unknown): OfficeInfo | undefined {
	const r = asRec(raw);
	if (!r) return undefined;
	const id = asStr(r.id);
	const organizationId = asStr(r.organization_id);
	const name = asStr(r.name);
	const host = asStr(r.host);
	if (!id || !organizationId || !name || !host) return undefined;

	return {
		id,
		organization_id: organizationId,
		name,
		role: asEnum(r.role, VALID_ROLES) ?? 'unknown',
		host,
		...decodeOfficeOptionalFields(r)
	};
}

/**
 * Production HTTP read adapter for `GET /api/v1/organizations/{org}/offices`.
 * Layers live satellite offices beside the connection's HQ office in Home, Shell, and Offices views.
 */
export function createHttpOfficesOverviewApi({
	http
}: {
	http: Pick<GatewayHttpTransport, 'read'>;
}): { overview(organizationId: string): Promise<OfficesOverview> } {
	return Object.freeze({
		async overview(organizationId: string): Promise<OfficesOverview> {
			const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/offices`;
			let reply: GatewayReply;
			try {
				reply = await http.read(path);
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Offices could not be reached.'
				} satisfies AppError;
			}

			const result = unwrapGatewayResult(reply, 'Offices could not be loaded.');
			if (!Array.isArray(result.offices)) {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'The offices list could not be read.'
				} satisfies AppError;
			}

			const allOffices = result.offices
				.map(decodeOfficeInfo)
				.filter((o): o is OfficeInfo => o !== undefined);
			const satellites = allOffices.filter((o) => o.role !== 'hq');
			const details: Record<string, SatelliteDetails> = {};
			for (const sat of satellites) {
				details[sat.id] = {
					officeId: sat.id,
					where: sat.host,
					sync: { kind: 'up_to_date' },
					follows: [],
					tasks: 0
				};
			}

			return {
				satellites,
				details,
				joinRequests: [],
				invitations: [],
				pendingAdds: [],
				outbox: [],
				placements: {},
				names: {},
				viewer: { isAdministrator: false }
			};
		}
	});
}
