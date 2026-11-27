import type { AppError, Member, OrgSnapshot } from '$lib/proto';
import { asRec, asStr } from '$lib/platform/http/decode';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import {
	readInstance,
	readJsonObject,
	readOfficeList,
	readResponse
} from '$lib/platform/http/wire';
import { readOrganizationsList, type OrganizationView } from '$lib/platform/organizations/readers';
import type { OrgApi } from '$lib/modules/organization/api';
import type { SettingsApi } from './api';
import type { GeneralSettings, HandOverCandidate, RuntimeId, RuntimeSupport } from './types';

export interface SettingsSessionInfo {
	accountAdmin: boolean;
	login: string;
}

interface GatewayAccessEntry {
	member_id: string;
	account_id?: string;
	login?: string;
	admin?: boolean;
}

interface WireProviderConnection {
	provider_type?: string;
	provider?: string;
	status?: string;
	label?: string;
}

const RUNTIME_CATALOG: readonly { id: RuntimeId; label: string; providers: readonly string[] }[] = [
	{ id: 'claude_code', label: 'Claude Code', providers: ['anthropic', 'claude', 'claude_code'] },
	{ id: 'codex', label: 'Codex', providers: ['openai', 'codex'] },
	{ id: 'antigravity', label: 'Antigravity', providers: ['google', 'gemini', 'antigravity', 'agy'] }
];

function normalizeRuntimeId(raw: unknown): RuntimeId | undefined {
	if (raw === 'claude_code' || raw === 'codex' || raw === 'antigravity') return raw;
	return undefined;
}

function onlineOfficeNamesForRuntime(
	agentsUsing: readonly Member[],
	officeById: ReadonlyMap<string, { id: string; name: string; status: string }>
): string[] {
	const names = new Set<string>();
	for (const m of agentsUsing) {
		const office = m.office_id ? officeById.get(m.office_id) : undefined;
		if (office && office.status === 'active') names.add(office.name);
	}
	return [...names];
}

function connectionSupport(
	id: RuntimeId,
	label: string,
	conn: WireProviderConnection
): RuntimeSupport {
	const status = (conn.status ?? '').toLowerCase();
	const ready =
		status === '' || status === 'active' || status === 'ready' || status === 'connected';
	const suffix = conn.label ? ` (${conn.label})` : '';
	return {
		id,
		label,
		availability: ready ? 'ready' : 'needs_setup',
		note: ready
			? `Provider connection configured${suffix}.`
			: `Provider connection ${conn.status ?? 'needs attention'}.`
	};
}

function evaluateRuntime(
	entry: { id: RuntimeId; label: string; providers: readonly string[] },
	snapshot: OrgSnapshot,
	officeById: ReadonlyMap<string, { id: string; name: string; status: string }>,
	connections: readonly WireProviderConnection[]
): RuntimeSupport {
	const { id, label, providers } = entry;
	const agentsUsing = snapshot.members.filter(
		(m) =>
			m.kind === 'agent' &&
			m.agent?.status !== 'offboarded' &&
			normalizeRuntimeId(m.agent?.runtime) === id
	);
	const onlineOffices = onlineOfficeNamesForRuntime(agentsUsing, officeById);
	if (onlineOffices.length > 0) {
		return { id, label, availability: 'ready', note: `Ready on ${onlineOffices.join(', ')}.` };
	}
	const matchingConn = connections.find((c) => {
		const p = (c.provider_type ?? c.provider ?? '').toLowerCase();
		return providers.some((token) => p.includes(token));
	});
	if (matchingConn) return connectionSupport(id, label, matchingConn);
	if (agentsUsing.length > 0) {
		const noun = agentsUsing.length === 1 ? 'agent' : 'agents';
		return {
			id,
			label,
			availability: 'unknown',
			note: `Assigned to ${agentsUsing.length} ${noun}; office status pending.`
		};
	}
	return {
		id,
		label,
		availability: 'needs_setup',
		note: 'No active agents or provider connections configured yet.'
	};
}

function deriveRuntimes(
	snapshot: OrgSnapshot,
	offices: readonly { id: string; name: string; status: string }[],
	connections: readonly WireProviderConnection[]
): RuntimeSupport[] {
	const officeById = new Map(offices.map((o) => [o.id, o]));
	return RUNTIME_CATALOG.map((entry) => evaluateRuntime(entry, snapshot, officeById, connections));
}

function buildCandidates(
	members: readonly Member[],
	entries: readonly GatewayAccessEntry[]
): HandOverCandidate[] {
	const memberById = new Map(members.map((m) => [m.id, m]));
	const out: HandOverCandidate[] = [];
	for (const entry of entries) {
		if (!entry.account_id || !entry.login || entry.admin) continue;
		const member = memberById.get(entry.member_id);
		if (!member || member.kind !== 'person') continue;
		out.push({
			memberId: member.id,
			name: member.name,
			username: entry.login
		});
	}
	return out.sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * Production HTTP adapter for Organization settings › General backed by `/api/v1/instance`,
 * `/api/v1/organizations`, `/api/v1/organizations/{org}/{snapshot,offices,access,provider-connections}`,
 * and `POST /api/v1/instance/admin/handover`.
 */
export function createHttpSettingsApi(deps: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
	session: () => SettingsSessionInfo | undefined;
}): SettingsApi {
	async function readOrgView(
		organizationId: string,
		signal?: AbortSignal
	): Promise<OrganizationView | undefined> {
		try {
			const reply = await (signal
				? deps.http.read('/api/v1/organizations', signal)
				: deps.http.read('/api/v1/organizations'));
			if (reply.status !== 200) return undefined;
			const envelope = readResponse(reply.body, readJsonObject);
			if ('error' in envelope) return undefined;
			return readOrganizationsList(envelope.result).organizations.find(
				(o) => o.id === organizationId
			);
		} catch (err) {
			if (signal?.aborted) throw err;
			return undefined;
		}
	}

	async function readInstanceInfo(signal?: AbortSignal): Promise<{ name: string } | undefined> {
		try {
			const reply = await (signal
				? deps.http.read('/api/v1/instance', signal)
				: deps.http.read('/api/v1/instance'));
			if (reply.status !== 200) return undefined;
			const envelope = readResponse(reply.body, readInstance);
			return 'result' in envelope ? { name: envelope.result.name || 'default' } : undefined;
		} catch (err) {
			if (signal?.aborted) throw err;
			return undefined;
		}
	}

	async function readOffices(organizationId: string, signal?: AbortSignal) {
		const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/offices`;
		try {
			const reply = await (signal ? deps.http.read(path, signal) : deps.http.read(path));
			if (reply.status !== 200) return [];
			const envelope = readResponse(reply.body, readOfficeList);
			return 'result' in envelope ? envelope.result.offices : [];
		} catch (err) {
			if (signal?.aborted) throw err;
			return [];
		}
	}

	async function readAccessEntries(
		organizationId: string,
		signal?: AbortSignal
	): Promise<GatewayAccessEntry[]> {
		const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/access`;
		try {
			const reply = await (signal ? deps.http.read(path, signal) : deps.http.read(path));
			if (reply.status !== 200) return [];
			const result = unwrapGatewayResult(reply, 'Access settings could not be loaded.');
			if (!Array.isArray(result.entries)) return [];
			return result.entries
				.map((item): GatewayAccessEntry | undefined => {
					const r = asRec(item);
					const member_id = asStr(r?.member_id);
					if (!r || !member_id) return undefined;
					return {
						member_id,
						account_id: asStr(r.account_id),
						login: asStr(r.login),
						admin: r.admin === true
					};
				})
				.filter((e): e is GatewayAccessEntry => e !== undefined);
		} catch (err) {
			if (signal?.aborted) throw err;
			return [];
		}
	}

	async function readProviderConnections(
		organizationId: string,
		signal?: AbortSignal
	): Promise<WireProviderConnection[]> {
		const path = `/api/v1/organizations/${encodeURIComponent(organizationId)}/provider-connections`;
		try {
			const reply = await (signal ? deps.http.read(path, signal) : deps.http.read(path));
			if (reply.status !== 200) return [];
			const result = unwrapGatewayResult(reply, 'Provider connections could not be loaded.');
			const raw = Array.isArray(result.connections)
				? result.connections
				: Array.isArray(result.provider_connections)
					? result.provider_connections
					: [];
			return raw
				.map((item): WireProviderConnection | undefined => {
					const r = asRec(item);
					if (!r) return undefined;
					return {
						provider_type: asStr(r.provider_type),
						provider: asStr(r.provider),
						status: asStr(r.status),
						label: asStr(r.label) ?? asStr(r.name)
					};
				})
				.filter((c): c is WireProviderConnection => c !== undefined);
		} catch (err) {
			if (signal?.aborted) throw err;
			return [];
		}
	}

	async function loadGeneral(
		organizationId: string,
		signal?: AbortSignal
	): Promise<GeneralSettings> {
		const [{ snapshot }, org, instance, offices, accessEntries, connections] = await Promise.all([
			deps.orgApi.view(organizationId, signal),
			readOrgView(organizationId, signal),
			readInstanceInfo(signal),
			readOffices(organizationId, signal),
			readAccessEntries(organizationId, signal),
			readProviderConnections(organizationId, signal)
		]);

		const current = deps.session();
		const viewerMember = org?.employee_id
			? snapshot.members.find((m) => m.id === org.employee_id)
			: snapshot.members.find((m) => m.owner);
		const isOwner = Boolean(viewerMember?.owner);
		const isAdministrator = Boolean(current?.accountAdmin);

		const hq = offices.find((o) => o.role === 'hq') ?? offices[0];
		const hqMachine = hq?.host || instance?.name || 'This machine';
		const workspace = hq ? `${hq.name} (${hq.host})` : `${org?.name ?? organizationId} · HQ`;

		const adminEntry = accessEntries.find((e) => e.admin);
		const adminMember = adminEntry
			? snapshot.members.find((m) => m.id === adminEntry.member_id)
			: isAdministrator
				? viewerMember
				: undefined;
		const adminUsername =
			adminEntry?.login ?? (isAdministrator ? current?.login : undefined) ?? 'administrator';

		return {
			organization: { id: organizationId, name: org?.name ?? organizationId },
			workspace,
			hqMachine,
			instance: instance?.name ?? 'default',
			runtimes: deriveRuntimes(snapshot, offices, connections),
			viewer: { isOwner, isAdministrator },
			recovery: { state: 'unknown' },
			administrator: {
				username: adminUsername,
				...(adminMember ? { person: adminMember.name } : {}),
				isYou: isAdministrator
			},
			handOverCandidates: buildCandidates(snapshot.members, accessEntries)
		};
	}

	return Object.freeze<SettingsApi>({
		general: loadGeneral,

		async handOverAdministrator(
			organizationId: string,
			memberId: string,
			password: string
		): Promise<GeneralSettings> {
			const entries = await readAccessEntries(organizationId);
			const target = entries.find((e) => e.member_id === memberId && e.account_id);
			if (!target?.account_id) {
				throw {
					source: 'api',
					code: 'not_found',
					message: 'That person does not have an account on this instance.'
				} satisfies AppError;
			}
			let reply: GatewayReply;
			try {
				reply = await deps.http.submit('/api/v1/instance/admin/handover', {
					to_account_id: target.account_id,
					password
				});
			} catch {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Administrator handover could not be completed.'
				} satisfies AppError;
			}
			unwrapGatewayResult(reply, 'Administrator handover was refused.');
			return loadGeneral(organizationId);
		}
	});
}
