import type { OrgApi } from '$lib/modules/organization/api';
import type { SearchHit } from '$lib/modules/shell/types';
import type { WorkItemsApi } from '$lib/modules/workitems/api';
import { STATUS_WORD, taskHref } from '$lib/modules/workitems/present';
import type { TaskRow } from '$lib/modules/workitems/types';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import type { Member, OrgSnapshot } from '$lib/proto';
import type { SearchPageApi } from './api';
import type { KnowledgeHit, SearchPageResults } from './types';

import { asRec, asStr } from '$lib/platform/http/decode';

function includesNeedle(value: string | undefined, needle: string): boolean {
	return Boolean(value && value.toLowerCase().includes(needle));
}

function searchProjects(snapshot: OrgSnapshot, needle: string): SearchHit[] {
	return (snapshot.projects ?? [])
		.filter(
			(p) =>
				!p.archived &&
				(includesNeedle(p.name, needle) ||
					includesNeedle(p.key, needle) ||
					includesNeedle(p.description, needle))
		)
		.map((p) => ({
			id: p.id,
			title: p.name,
			...(p.key ? { meta: p.key } : {}),
			href: `/projects/${encodeURIComponent(p.id)}`
		}));
}

function searchWork(rows: readonly TaskRow[], needle: string): SearchHit[] {
	return rows
		.filter(
			(row) =>
				includesNeedle(row.ref, needle) ||
				includesNeedle(row.title, needle) ||
				includesNeedle(row.project?.name, needle) ||
				includesNeedle(row.assignee?.name, needle)
		)
		.slice(0, 40)
		.map((row) => ({
			id: row.id,
			title: `${row.ref} · ${row.title}`,
			meta: [row.project?.name, STATUS_WORD[row.status]].filter(Boolean).join(' · '),
			href: taskHref(row.ref)
		}));
}

function memberMatches(m: Member, needle: string): boolean {
	if (m.agent?.status === 'offboarded') return false;
	return (
		includesNeedle(m.name, needle) ||
		includesNeedle(m.handle, needle) ||
		includesNeedle(m.agent?.role, needle) ||
		includesNeedle(m.agent?.title, needle)
	);
}

function searchMembers(snapshot: OrgSnapshot, needle: string): SearchHit[] {
	return snapshot.members
		.filter((m) => memberMatches(m, needle))
		.map((m) => ({
			id: m.id,
			title: m.name,
			meta: m.agent?.title || m.agent?.role || (m.kind === 'agent' ? 'Agent' : 'Person'),
			href: `/members/${encodeURIComponent(m.id)}`
		}));
}

function toKnowledgeHit(
	item: unknown,
	needle: string,
	indexedAt: string
): KnowledgeHit | undefined {
	const doc = asRec(asRec(item)?.document);
	const id = asStr(doc?.id);
	const title = asStr(doc?.title);
	if (!doc || !id || !title) return undefined;
	const body = asStr(doc.body_markdown) ?? '';
	const key = asStr(doc.key) ?? null;
	if (
		!includesNeedle(title, needle) &&
		!includesNeedle(key ?? undefined, needle) &&
		!includesNeedle(body, needle)
	) {
		return undefined;
	}
	return {
		id,
		recordType: asStr(doc.kind) ?? 'process',
		kindLabel: asStr(doc.kind) === 'standard' ? 'Standard' : 'Process',
		key,
		title,
		scopeLabel: asStr(doc.scope_name) ?? 'Organization',
		status: asStr(doc.status) ?? 'current',
		updatedAt: asStr(doc.updated_at) ?? indexedAt,
		excerpt: body.trim().slice(0, 180) || null,
		excerptLabel: 'Excerpt',
		href: `/settings/processes/documents/${encodeURIComponent(id)}`
	};
}

function searchProcessDocs(
	procReply: GatewayReply | undefined,
	needle: string,
	indexedAt: string
): KnowledgeHit[] {
	if (!procReply || procReply.status !== 200) return [];
	try {
		const procRes = unwrapGatewayResult(procReply, 'Processes search failed.');
		const rows = Array.isArray(procRes.rows) ? procRes.rows : [];
		return rows
			.map((item) => toKnowledgeHit(item, needle, indexedAt))
			.filter((hit): hit is KnowledgeHit => hit !== undefined);
	} catch {
		return [];
	}
}

/**
 * Production HTTP adapter for `/search` combining live org snapshot (projects, members),
 * live work queue (`workItemsApi.queue`), and live organization processes/standards.
 */
export function createHttpSearchApi(deps: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
	workItemsApi: Pick<WorkItemsApi, 'queue'>;
	now?: () => string;
}): SearchPageApi {
	const now = deps.now ?? (() => new Date().toISOString());

	return Object.freeze<SearchPageApi>({
		async results(organizationId: string, query: string): Promise<SearchPageResults> {
			const needle = query.trim().toLowerCase();
			const indexedAt = now();
			if (!needle) {
				return {
					projects: [],
					work: [],
					members: [],
					knowledge: [],
					coverage: { scope: 'title_only', indexedAt }
				};
			}

			const [{ snapshot }, workRes, procReply] = await Promise.all([
				deps.orgApi.view(organizationId),
				deps.workItemsApi
					.queue(organizationId, { kind: 'organization' })
					.catch(() => ({ rows: [] })),
				deps.http
					.read(
						`/api/v1/organizations/${encodeURIComponent(organizationId)}/processes?level=organization`
					)
					.catch(() => undefined)
			]);

			return {
				projects: searchProjects(snapshot, needle),
				work: searchWork(workRes.rows, needle),
				members: searchMembers(snapshot, needle),
				knowledge: searchProcessDocs(procReply, needle, indexedAt),
				coverage: {
					scope: 'full_body',
					indexedAt,
					note: 'Searches live projects, work items, members, and organization processes.'
				}
			};
		}
	});
}
