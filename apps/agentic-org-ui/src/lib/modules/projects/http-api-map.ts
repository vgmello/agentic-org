import type { AppError, Member, Team } from '$lib/proto';
import { formatUuidV7 } from '$lib/platform/http/clock';
import { readOperationId } from '$lib/platform/http/wire';
import type { TeamRef } from '$lib/modules/members';
import { emptyBugCounts, emptyTaskCounts, type ProjectWorkTotals } from './live-hydration';
import type { ProjectField, ProjectRow, ProjectTeamEntry } from './types';

export interface GatewayProjectRecord {
	id: string;
	number: number;
	key: string;
	slug: string;
	name: string;
	owner_team_id: string;
	contributing_team_ids: string[];
	archived: boolean;
	revision: number;
}

export const ZERO_FEATURES = Object.freeze({ planned: 0, inProgress: 0, live: 0 });

export function teamRef(team: Team | undefined, fallbackId: string): TeamRef {
	if (!team) return { id: fallbackId, name: fallbackId, ref: '' };
	return { id: team.id, name: team.name, ref: '' };
}

export function teamPath(team: Team, byId: ReadonlyMap<string, Team>): string {
	const names: string[] = [];
	const seen = new Set<string>();
	let current: Team | undefined = team;
	while (current && !seen.has(current.id)) {
		seen.add(current.id);
		names.unshift(current.name);
		current = current.owner_team_id ? byId.get(current.owner_team_id) : undefined;
	}
	return names.join(' › ');
}

export function isProjectRecord(value: unknown): value is GatewayProjectRecord {
	if (!value || typeof value !== 'object') return false;
	const r = value as Partial<GatewayProjectRecord>;
	return (
		typeof r.id === 'string' &&
		typeof r.number === 'number' &&
		typeof r.slug === 'string' &&
		typeof r.name === 'string' &&
		typeof r.owner_team_id === 'string' &&
		Array.isArray(r.contributing_team_ids) &&
		typeof r.archived === 'boolean' &&
		typeof r.revision === 'number'
	);
}

export function toProjectRow(
	record: GatewayProjectRecord,
	teamsById: ReadonlyMap<string, Team>,
	totals?: ProjectWorkTotals
): ProjectRow {
	return {
		id: record.id,
		number: record.number,
		slug: record.slug,
		name: record.name,
		ownerTeam: teamRef(teamsById.get(record.owner_team_id), record.owner_team_id),
		contributors: record.contributing_team_ids.map((id) => teamRef(teamsById.get(id), id)),
		archived: record.archived,
		tasks: totals ? { ...totals.tasks } : emptyTaskCounts(),
		bugs: totals ? { ...totals.bugs } : emptyBugCounts(),
		developmentProcess: null
	};
}

export function isMemberActive(m: Member): boolean {
	return m.agent?.status !== 'offboarded';
}

export function toProjectTeamEntry(
	teamId: string,
	teamsById: ReadonlyMap<string, Team>,
	membersById: ReadonlyMap<string, Member>
): ProjectTeamEntry {
	const t = teamsById.get(teamId);
	const lead = t?.lead_member_id ? membersById.get(t.lead_member_id) : undefined;
	return {
		...teamRef(t, teamId),
		...(lead && isMemberActive(lead) ? { lead: { id: lead.id, name: lead.name } } : {})
	};
}

export function inferProjectField(error: AppError): ProjectField | undefined {
	const msg = error.message.toLowerCase();
	if (msg.includes('slug')) return 'slug';
	if (msg.includes('team')) return 'owner';
	if (error.code === 'invalid_name' || msg.includes('name')) return 'name';
	return undefined;
}

export function defaultUuidV7(): string {
	return formatUuidV7(Date.now(), globalThis.crypto.getRandomValues(new Uint8Array(10)));
}

export function normalizeUuidV7(candidate: string, fallback: () => string): string {
	try {
		return readOperationId(candidate);
	} catch {
		return readOperationId(fallback());
	}
}
