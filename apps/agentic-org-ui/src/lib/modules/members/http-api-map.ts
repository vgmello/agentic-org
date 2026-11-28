import type { AppError, Member, OrgSnapshot, Team } from '$lib/proto';
import { unwrapGatewayResult, type GatewayReply } from '$lib/platform/http/gateway-errors';
import type { OrganizationView } from '$lib/platform/organizations/readers';
import type {
	EmployeeStatus,
	MemberRow,
	Placement,
	SignInLink,
	TeamRef,
	Turn,
	TurnState
} from './types';

export interface GatewayAccessEntry {
	member_id: string;
	account_id?: string | null;
	login?: string | null;
	admin: boolean;
	owner: boolean;
	pending_invite: boolean;
	pending_password_reset: boolean;
}

export interface MembersSessionInfo {
	accountAdmin: boolean;
}

export function teamRef(team: Team | undefined): TeamRef | undefined {
	if (!team) return undefined;
	return { id: team.id, name: team.name, ref: '' };
}

export function memberHandle(m: Member): string {
	return m.handle ?? (m.kind === 'agent' ? (m.agent?.messaging_name ?? m.id) : m.name);
}

export function memberStatus(m: Member): EmployeeStatus {
	if (m.kind === 'agent' && m.agent?.status === 'offboarded') return 'inactive';
	if (m.kind === 'agent' && m.agent?.status === 'paused') return 'paused';
	return 'active';
}

export function agentPlacement(
	m: Member,
	org: OrganizationView | undefined,
	officeNamesById?: ReadonlyMap<string, string>
): Placement | null | undefined {
	if (m.kind !== 'agent') return undefined;
	if (!m.office_id) return null;
	const name =
		officeNamesById?.get(m.office_id) ??
		org?.offices.find((o) => o.id === m.office_id)?.name ??
		m.office_id;
	return { id: m.office_id, name };
}

export function parseLinkResult(reply: GatewayReply, fallback: string): SignInLink {
	const result = unwrapGatewayResult(reply, fallback);
	if (typeof result.url !== 'string' || typeof result.expires_at_ms !== 'number') {
		throw { source: 'api', code: 'unavailable', message: fallback } satisfies AppError;
	}
	return { url: result.url, expiresAt: new Date(result.expires_at_ms).toISOString() };
}

function resolveCanSignIn(
	m: Member,
	status: MemberRow['status'],
	access: GatewayAccessEntry | undefined,
	org: OrganizationView | undefined
): boolean {
	if (status === 'inactive' || m.kind !== 'person') return false;
	return access ? Boolean(access.account_id) : Boolean(m.owner && org?.employee_id === m.id);
}

function countDirectReports(snapshot: OrgSnapshot, memberId: string): number {
	return snapshot.members.filter(
		(other) => memberStatus(other) !== 'inactive' && other.reports_to_member_id === memberId
	).length;
}

export function mapMemberRow(
	m: Member,
	snapshot: OrgSnapshot,
	teamsById: ReadonlyMap<string, Team>,
	membersById: ReadonlyMap<string, Member>,
	accessById: ReadonlyMap<string, GatewayAccessEntry> | undefined,
	org: OrganizationView | undefined,
	officeNamesById?: ReadonlyMap<string, string>
): MemberRow {
	const status = memberStatus(m);
	const homeTeam = m.home_team_id ? teamRef(teamsById.get(m.home_team_id)) : undefined;
	const manager = m.reports_to_member_id ? membersById.get(m.reports_to_member_id) : undefined;
	const row: MemberRow = {
		id: m.id,
		kind: m.kind === 'agent' ? 'agent' : 'person',
		name: m.name,
		handle: memberHandle(m),
		status,
		owner: Boolean(m.owner),
		canSignIn: resolveCanSignIn(m, status, accessById?.get(m.id), org),
		directReports: countDirectReports(snapshot, m.id)
	};
	if (m.agent?.role) row.role = m.agent.role;
	if (m.builtin_key) row.builtinKey = m.builtin_key;
	if (homeTeam) row.homeTeam = homeTeam;
	if (m.kind === 'agent') row.office = agentPlacement(m, org, officeNamesById);
	if (manager) row.reportsTo = { id: manager.id, name: manager.name };
	if (m.contact) row.contact = m.contact;
	return row;
}

function decodeTurnState(raw: unknown): TurnState {
	switch (typeof raw === 'string' ? raw.toLowerCase() : '') {
		case 'running':
		case 'active':
			return 'running';
		case 'succeeded':
		case 'completed':
		case 'idle':
			return 'succeeded';
		case 'failed':
		case 'error':
			return 'failed';
		case 'cancelled':
		case 'stopped':
			return 'cancelled';
		default:
			return 'accepted';
	}
}

function readOptString(value: unknown): string | undefined {
	return typeof value === 'string' ? value : undefined;
}

export function decodeSessionTurn(item: unknown): Turn | undefined {
	if (!item || typeof item !== 'object') return undefined;
	const r = item as Record<string, unknown>;
	const id = readOptString(r.session_id) ?? readOptString(r.id);
	if (!id) return undefined;
	const startedAt =
		readOptString(r.started_at) ?? readOptString(r.created_at) ?? new Date(0).toISOString();
	const endedAt = readOptString(r.ended_at);
	const summary = readOptString(r.summary);
	const failure = readOptString(r.error);
	return {
		id,
		startedAt,
		...(endedAt ? { endedAt } : {}),
		startedBy: { kind: 'run' },
		state: decodeTurnState(r.state ?? r.status),
		...(summary ? { summary } : {}),
		...(failure ? { failure } : {}),
		attempts: [],
		hasLog: false
	};
}
