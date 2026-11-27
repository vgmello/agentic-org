import { badgeInitials as initialsOf, relativeAge } from '$lib/format';
import type { Area, Member, OrgSnapshot, Team, WorkItemSummary } from '$lib/proto';
import { deriveDecisionSpotlight, type SpotlightDecision } from '../../spotlight';
import { KIND_WORD, type InboxItem } from '../../types';
import {
	isUnbuiltWorkRefPath,
	safeWorkListHref,
	sanitizeOpenHref,
	type HighlightSegment,
	type TriageItem
} from './triage-types';

/**
 * Splits `text` into plain-text segments marking case-insensitive literal matches of `query`.
 * Rendered via `<mark>` elements without HTML injection.
 */
export function highlightSegments(text: string, query: string): HighlightSegment[] {
	const trimmed = query.trim();
	if (!trimmed || !text) return [{ text, match: false }];

	const escaped = trimmed.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
	const regex = new RegExp(`(${escaped})`, 'gi');
	const parts = text.split(regex);
	if (parts.length <= 1) return [{ text, match: false }];

	const needle = trimmed.toLowerCase();
	const segments: HighlightSegment[] = [];
	for (const part of parts) {
		if (!part) continue;
		segments.push({ text: part, match: part.toLowerCase() === needle });
	}
	return segments.length > 0 ? segments : [{ text, match: false }];
}

function extractWorkRef(
	openPath: string | undefined,
	openLabel: string | undefined,
	title: string,
	workItem?: WorkItemSummary
): string | undefined {
	if (workItem?.short_id) return workItem.short_id;
	const pathMatch = openPath?.match(/\/work\/([^/?#]+)/);
	if (pathMatch) {
		try {
			return decodeURIComponent(pathMatch[1]);
		} catch {
			return pathMatch[1];
		}
	}
	const labelMatch = openLabel?.match(/\b([A-Z]+-\d+)\b/);
	if (labelMatch) return labelMatch[1];
	return title.match(/\b([A-Z]+-\d+)\b/)?.[1];
}

function resolveMemberRole(
	member: Member | undefined,
	isAgent: boolean,
	teamName: string | undefined
): string {
	const title = member?.agent?.title?.trim();
	const kindWord = isAgent ? 'Agent' : 'Person';
	if (title) return `${title} · ${kindWord}`;
	if (teamName) return `${teamName} · ${kindWord}`;
	return kindWord;
}

function resolveClearsLabel(decision: SpotlightDecision): string {
	const inbox = decision.inboxItem;
	if (!inbox) return decision.impact.sub;
	if (inbox.actions.includes('approve') || inbox.actions.includes('deny')) {
		return 'Only when you approve or deny';
	}
	if (inbox.actions.includes('choose')) return 'When you choose an option';
	if (inbox.actions.includes('reply')) return 'When you send a reply';
	if (inbox.actions.includes('dismiss')) return 'When you dismiss or source resolves';
	return 'Clears when its source is resolved';
}

function resolveRecommendationTone(
	category: TriageItem['category'],
	urgent: boolean
): 'ok' | 'warn' | 'bad' | 'neutral' {
	if (category === 'problem') return 'bad';
	if (category === 'decision') return 'warn';
	if (category === 'approval') return 'ok';
	if (category === 'work' && urgent) return 'warn';
	return 'neutral';
}

function parseTimestamp(iso: string | undefined, fallbackMs: number): number {
	if (!iso) return fallbackMs;
	const parsed = Date.parse(iso);
	return Number.isNaN(parsed) ? fallbackMs : parsed;
}

interface LookupMaps {
	memberMap: ReadonlyMap<string, Member>;
	teamMap: ReadonlyMap<string, Team>;
	areaMap: ReadonlyMap<string, Area>;
	workByRef: ReadonlyMap<string, WorkItemSummary>;
}

function resolveWorkLinks(input: {
	rawOpenPath: string | undefined;
	openLabel: string | undefined;
	title: string;
	explicitWork?: WorkItemSummary;
	explicitProjectId?: string;
	workByRef: ReadonlyMap<string, WorkItemSummary>;
}) {
	const workRef = extractWorkRef(
		input.rawOpenPath,
		input.openLabel,
		input.title,
		input.explicitWork
	);
	const matchedWork =
		input.explicitWork ?? (workRef ? input.workByRef.get(workRef.toUpperCase()) : undefined);
	const workRefDisplay = matchedWork ? `${matchedWork.short_id} · ${matchedWork.title}` : workRef;
	const resolvedProjectId = input.explicitProjectId ?? matchedWork?.project_id;
	const workHref = workRef ? safeWorkListHref(resolvedProjectId) : undefined;
	const openHref = sanitizeOpenHref(input.rawOpenPath, resolvedProjectId);
	const relatedHref = openHref && !isUnbuiltWorkRefPath(input.rawOpenPath) ? openHref : undefined;
	return {
		workRef,
		matchedWork,
		workRefDisplay,
		resolvedProjectId,
		workHref,
		openHref,
		relatedHref
	};
}

function resolveOpenTeam(
	inbox: InboxItem | undefined,
	work: WorkItemSummary | undefined,
	maps: LookupMaps
): { teamId?: string; teamName?: string } {
	if (inbox?.team?.id) return { teamId: inbox.team.id, teamName: inbox.team.name };
	const area = work?.area_id ? maps.areaMap.get(work.area_id) : undefined;
	if (!area?.team_id) return { teamId: inbox?.team?.id, teamName: inbox?.team?.name };
	const team = maps.teamMap.get(area.team_id);
	return { teamId: team?.id ?? area.team_id, teamName: team?.name };
}

const INBOX_CATEGORIES = new Set(['approval', 'decision', 'question', 'problem']);

function resolveDecisionCategory(decision: SpotlightDecision): TriageItem['category'] {
	if (decision.sourceKind === 'work') return 'work';
	return INBOX_CATEGORIES.has(decision.category)
		? (decision.category as TriageItem['category'])
		: 'work';
}

function resolveOpenLabel(decision: SpotlightDecision): string | undefined {
	if (decision.inboxItem?.open?.label) return decision.inboxItem.open.label;
	if (decision.sourceKind !== 'work') return undefined;
	return decision.item ? `Open ${decision.item.short_id}` : 'Open work item';
}

function mapOpenDecision(decision: SpotlightDecision, maps: LookupMaps, nowMs: number): TriageItem {
	const inbox = decision.inboxItem;
	const work = decision.item;
	const actorId = inbox?.who.id ?? work?.assignee_id ?? work?.created_by;
	const member = actorId ? maps.memberMap.get(actorId) : undefined;
	const isAgent = inbox ? inbox.who.kind === 'agent' : member?.kind === 'agent';
	const { teamId, teamName } = resolveOpenTeam(inbox, work, maps);
	const category = resolveDecisionCategory(decision);
	const rawOpenPath =
		inbox?.open?.path ?? (decision.sourceKind === 'work' ? decision.detailHref : undefined);
	const links = resolveWorkLinks({
		rawOpenPath,
		openLabel: inbox?.open?.label,
		title: decision.title,
		explicitWork: work,
		explicitProjectId: inbox?.project?.id ?? decision.projectId,
		workByRef: maps.workByRef
	});
	const createdIso = inbox?.created_at ?? work?.updated_at;
	const openedIso = inbox?.created_at ?? work?.created_at ?? work?.updated_at;
	const recommendation = inbox
		? inbox.recommendation
		: decision.recommendation !== decision.backgroundContext
			? decision.recommendation
			: undefined;
	const expiresLabel =
		inbox?.expires_at || (decision.sourceKind === 'work' && decision.urgent)
			? decision.badgeText
			: undefined;

	return {
		id: decision.id,
		state: 'open',
		sourceKind: decision.sourceKind,
		category,
		decision,
		inboxItem: inbox,
		workItem: links.matchedWork,
		title: decision.title,
		kindDisplay: inbox
			? `${KIND_WORD[inbox.kind]} · ${inbox.source}`
			: `Work · ${decision.kindLabel}`,
		typeLabel: inbox?.source ?? decision.kindLabel,
		actorId,
		actorName: decision.actorName,
		actorInitials: decision.actorInitials,
		actorRole: resolveMemberRole(member, isAgent, teamName),
		isAgent,
		teamId,
		teamName,
		projectId: links.resolvedProjectId,
		projectName: inbox?.project?.name ?? decision.projectName,
		hasProject: Boolean(links.resolvedProjectId),
		why: decision.backgroundContext,
		recommendation,
		recommendationTone: resolveRecommendationTone(category, decision.urgent),
		urgent: decision.urgent,
		expiresLabel,
		expiresAtIso: inbox?.expires_at,
		clearsLabel: resolveClearsLabel(decision),
		workRef: links.workRef,
		workRefDisplay: links.workRefDisplay,
		workHref: links.workHref,
		openHref: links.openHref,
		relatedHref: links.relatedHref,
		openLabel: resolveOpenLabel(decision),
		auditChecks: decision.auditChecks,
		ageLabel: decision.ageLabel,
		createdAgeLabel: openedIso ? relativeAge(openedIso, nowMs) : decision.ageLabel,
		timestampMs: parseTimestamp(createdIso, nowMs)
	};
}

function buildInboxAuditChecks(item: InboxItem, outcome?: string): string[] {
	const checks: string[] = [];
	if (item.source) checks.push(`Source: ${item.source}`);
	if (item.project?.name) checks.push(`Project: ${item.project.name}`);
	if (item.team?.name) checks.push(`Team: ${item.team.name}`);
	if (outcome) checks.push(`Outcome: ${outcome}`);
	return checks;
}

function mapNonOpenInboxItem(
	item: InboxItem,
	state: 'handled' | 'snoozed',
	maps: LookupMaps,
	nowMs: number
): TriageItem {
	const member = maps.memberMap.get(item.who.id);
	const isAgent = item.who.kind === 'agent';
	const links = resolveWorkLinks({
		rawOpenPath: item.open?.path,
		openLabel: item.open?.label,
		title: item.title,
		explicitProjectId: item.project?.id,
		workByRef: maps.workByRef
	});
	const isHandled = state === 'handled';
	const handledIso = item.handled?.at ?? item.created_at;
	const outcome = item.handled?.outcome ?? 'Resolved';

	return {
		id: item.id,
		state,
		sourceKind: 'inbox',
		category: item.kind,
		inboxItem: item,
		workItem: links.matchedWork,
		title: item.title,
		kindDisplay: `${KIND_WORD[item.kind]} · ${item.source}`,
		typeLabel: item.source,
		actorId: item.who.id,
		actorName: item.who.name,
		actorInitials: initialsOf(item.who.name),
		actorRole: resolveMemberRole(member, isAgent, item.team?.name),
		isAgent,
		teamId: item.team?.id,
		teamName: item.team?.name,
		projectId: links.resolvedProjectId,
		projectName: item.project?.name ?? item.team?.name ?? 'Organization',
		hasProject: Boolean(links.resolvedProjectId),
		why: item.why,
		recommendation: item.recommendation,
		recommendationTone: isHandled ? 'ok' : resolveRecommendationTone(item.kind, false),
		urgent: false,
		...(isHandled ? {} : { expiresAtIso: item.expires_at }),
		clearsLabel: isHandled
			? `Handled · ${outcome}`
			: 'Returns to Inbox when due, or un-snooze early',
		workRef: links.workRef,
		workRefDisplay: links.workRefDisplay,
		workHref: links.workHref,
		openHref: links.openHref,
		relatedHref: links.relatedHref,
		openLabel: item.open?.label,
		auditChecks: buildInboxAuditChecks(item, isHandled ? outcome : undefined),
		ageLabel: relativeAge(isHandled ? handledIso : item.created_at, nowMs),
		createdAgeLabel: relativeAge(item.created_at, nowMs),
		timestampMs: parseTimestamp(
			isHandled ? handledIso : (item.snoozed_until ?? item.created_at),
			nowMs
		),
		...(isHandled
			? {
					handled: {
						outcome,
						by: item.handled?.by,
						atIso: handledIso,
						atLabel: relativeAge(handledIso, nowMs)
					}
				}
			: {
					snoozed: {
						untilIso: item.snoozed_until,
						note: item.snooze_note
					}
				})
	};
}

/**
 * Derives the unified list of open, snoozed, and handled triage items from canonical work items and inbox items.
 */
export function deriveTriageItems(
	workItems: readonly WorkItemSummary[],
	snapshot: OrgSnapshot,
	ownerId: string,
	inboxItems: readonly InboxItem[],
	nowMs = Date.now()
): TriageItem[] {
	const maps: LookupMaps = {
		memberMap: new Map(snapshot.members.map((m) => [m.id, m])),
		teamMap: new Map(snapshot.teams.map((t) => [t.id, t])),
		areaMap: new Map((snapshot.areas ?? []).map((a) => [a.id, a])),
		workByRef: new Map(workItems.map((w) => [w.short_id.toUpperCase(), w]))
	};

	const openDecisions = deriveDecisionSpotlight(workItems, snapshot, ownerId, nowMs, inboxItems);
	const openTriage = openDecisions.map((d) => mapOpenDecision(d, maps, nowMs));
	const snoozedTriage = inboxItems
		.filter((item) => item.state === 'snoozed')
		.map((item) => mapNonOpenInboxItem(item, 'snoozed', maps, nowMs));
	const handledTriage = inboxItems
		.filter((item) => item.state === 'handled')
		.map((item) => mapNonOpenInboxItem(item, 'handled', maps, nowMs));

	return [...openTriage, ...snoozedTriage, ...handledTriage];
}
