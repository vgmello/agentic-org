import type { Member, OrgSnapshot, WorkItemSummary } from '$lib/proto';
import {
	deriveDecisionSpotlight,
	initialsOf,
	relativeAge,
	type SpotlightDecision
} from '$lib/home/model';
import { KIND_WORD, type InboxItem, type InboxScope } from '../../types';

export type TriageTab =
	'all' | 'approvals' | 'decisions' | 'questions' | 'handled' | 'problems' | 'work';

export interface TriageTabDef {
	id: TriageTab;
	label: string;
}

export type TriageOrder = 'priority' | 'date';

export const PRIMARY_TABS: readonly TriageTabDef[] = [
	{ id: 'all', label: 'All' },
	{ id: 'approvals', label: 'Approvals' },
	{ id: 'decisions', label: 'Decisions' }
];

export const OVERFLOW_TABS: readonly TriageTabDef[] = [
	{ id: 'questions', label: 'Questions' },
	{ id: 'problems', label: 'Problems' },
	{ id: 'work', label: 'Work' },
	{ id: 'handled', label: 'Handled' }
];

export const ALL_TRIAGE_TABS: readonly TriageTabDef[] = [...PRIMARY_TABS, ...OVERFLOW_TABS];

export function queueSummaryLabel(tab: TriageTab, order: TriageOrder = 'priority'): string {
	if (isOverflowTab(tab)) {
		if (tab === 'handled') return 'Handled items';
		return OVERFLOW_TABS.find((t) => t.id === tab)?.label ?? 'All';
	}
	return order === 'date' ? 'By date' : 'Priority first';
}

const TAB_ALIASES: Record<string, TriageTab> = {
	all: 'all',
	approval: 'approvals',
	approvals: 'approvals',
	decision: 'decisions',
	decisions: 'decisions',
	question: 'questions',
	questions: 'questions',
	handled: 'handled',
	problem: 'problems',
	problems: 'problems',
	work: 'work'
};

export function parseTriageTab(value: string | null | undefined): TriageTab {
	if (!value) return 'all';
	const key = value.trim().toLowerCase();
	return Object.hasOwn(TAB_ALIASES, key) ? TAB_ALIASES[key] : 'all';
}

export function isOverflowTab(tab: TriageTab): boolean {
	return OVERFLOW_TABS.some((t) => t.id === tab);
}

export type TriageGroupKey = 'urgent' | 'today' | 'earlier' | 'handled';

export interface TriageGroup {
	key: TriageGroupKey;
	label: string;
	items: TriageItem[];
}

export interface HighlightSegment {
	text: string;
	match: boolean;
}

export interface TriageItem {
	id: string;
	state: 'open' | 'handled';
	sourceKind: 'inbox' | 'work';
	category: 'approval' | 'decision' | 'question' | 'problem' | 'work';
	decision?: SpotlightDecision;
	inboxItem?: InboxItem;
	workItem?: WorkItemSummary;
	title: string;
	kindDisplay: string;
	typeLabel: string;
	actorId?: string;
	actorName: string;
	actorInitials: string;
	actorRole: string;
	isAgent: boolean;
	teamId?: string;
	teamName?: string;
	projectId?: string;
	projectName: string;
	hasProject: boolean;
	why: string;
	recommendation?: string;
	recommendationTone: 'ok' | 'warn' | 'bad' | 'neutral';
	urgent: boolean;
	expiresLabel?: string;
	expiresAtIso?: string;
	clearsLabel: string;
	workRef?: string;
	workRefDisplay?: string;
	workHref?: string;
	openHref?: string;
	relatedHref?: string;
	openLabel?: string;
	auditChecks: string[];
	ageLabel: string;
	createdAgeLabel: string;
	timestampMs: number;
	handled?: {
		outcome: string;
		by?: string;
		atIso: string;
		atLabel: string;
	};
}

export interface TriageFilterOptions {
	tab: TriageTab;
	urgentOnly: boolean;
	scope: InboxScope;
	search: string;
	order?: TriageOrder;
}

export interface InboxCommentEntry {
	id: string;
	text: string;
	at: number;
}

export const MAX_INBOX_COMMENT_LENGTH = 2000;
export const INBOX_DIVIDERS_STORAGE_KEY = 'agentic-org.inbox.dividers';

export function inboxCommentsStorageKey(organizationId: string, itemId: string): string {
	return `agentic-org.inbox.comments.${organizationId}.${itemId}`;
}

export function parseInboxCommentsJson(raw: string | null): InboxCommentEntry[] {
	if (!raw) return [];
	try {
		const parsed: unknown = JSON.parse(raw);
		if (!Array.isArray(parsed)) return [];
		const valid: InboxCommentEntry[] = [];
		const seenIds = new Set<string>();
		for (let i = 0; i < parsed.length; i++) {
			const entry = parsed[i];
			if (!entry || typeof entry !== 'object') continue;
			const rec = entry as Record<string, unknown>;
			if (typeof rec.text !== 'string') continue;
			const trimmed = rec.text.trim();
			if (!trimmed || rec.text.length > MAX_INBOX_COMMENT_LENGTH) continue;
			if (typeof rec.at !== 'number' || !Number.isFinite(rec.at)) continue;
			if (!Number.isFinite(new Date(rec.at).getTime())) continue;
			const explicitId = typeof rec.id === 'string' ? rec.id.trim() : '';
			const id = explicitId || `c-${rec.at}-${i}`;
			if (seenIds.has(id)) continue;
			seenIds.add(id);
			valid.push({ id, text: trimmed, at: rec.at });
		}
		return valid.sort((a, b) => a.at - b.at);
	} catch {
		return [];
	}
}

const ROUTE_TEMPLATE_BRACKET_RE = /[[\]]/;

export function isInternalAppPath(path: string | undefined): path is string {
	if (!path) return false;
	const trimmed = path.trim();
	return trimmed.startsWith('/') && !trimmed.startsWith('//') && !trimmed.startsWith('/\\');
}

const ROUTE_GROUP_SEGMENT_RE = /^\(.+\)$/;
const PERCENT_ENCODED_RE = /%[0-9a-fA-F]{2}/;

function decodeSegmentFully(rawSegment: string): string | undefined {
	let current = rawSegment;
	for (let i = 0; i < 4; i += 1) {
		try {
			const next = decodeURIComponent(current);
			if (next === current) return current;
			current = next;
		} catch {
			return undefined;
		}
	}
	return PERCENT_ENCODED_RE.test(current) ? undefined : current;
}

function parseNormalizedInternalUrl(
	rawPath: string | undefined
): { pathname: string; segments: string[]; search: string; hash: string } | undefined {
	if (!isInternalAppPath(rawPath)) return undefined;
	const trimmed = rawPath.trim();
	try {
		const parsed = new URL(trimmed, 'https://app.invalid');
		if (parsed.origin !== 'https://app.invalid') return undefined;
		const normPath = parsed.pathname.replace(/\/+/g, '/');
		const rawSegments = normPath.split('/').filter(Boolean);
		const segments: string[] = [];
		for (const rawSeg of rawSegments) {
			const decoded = decodeSegmentFully(rawSeg);
			if (decoded === undefined) return undefined;
			if (
				decoded === '.' ||
				decoded === '..' ||
				decoded.includes('/') ||
				decoded.includes('\\') ||
				ROUTE_TEMPLATE_BRACKET_RE.test(rawSeg) ||
				ROUTE_TEMPLATE_BRACKET_RE.test(decoded) ||
				ROUTE_GROUP_SEGMENT_RE.test(rawSeg) ||
				ROUTE_GROUP_SEGMENT_RE.test(decoded)
			) {
				return undefined;
			}
			segments.push(decoded);
		}
		return { pathname: normPath, segments, search: parsed.search, hash: parsed.hash };
	} catch {
		return undefined;
	}
}

export function isUnbuiltWorkRefPath(path: string | undefined): boolean {
	const parsed = parseNormalizedInternalUrl(path);
	return Boolean(
		parsed && parsed.segments[0]?.toLowerCase() === 'work' && parsed.segments.length >= 2
	);
}

export function safeWorkListHref(projectId: string | undefined): string {
	return projectId ? `/projects/${encodeURIComponent(projectId)}/work` : '/work';
}

export function sanitizeOpenHref(
	rawPath: string | undefined,
	projectId: string | undefined
): string | undefined {
	const parsed = parseNormalizedInternalUrl(rawPath);
	if (!parsed) return undefined;
	const head = parsed.segments[0]?.toLowerCase();
	if (head === 'search') return undefined;
	if (head === 'work' && parsed.segments.length >= 2) {
		return safeWorkListHref(projectId);
	}
	return `${parsed.pathname}${parsed.search}${parsed.hash}`;
}

const DAY_MS = 24 * 60 * 60 * 1000;

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
	const titleMatch = title.match(/\b([A-Z]+-\d+)\b/);
	return titleMatch?.[1];
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
	if (inbox) {
		if (inbox.actions.includes('approve') || inbox.actions.includes('deny')) {
			return 'Only when you approve or deny';
		}
		if (inbox.actions.includes('choose')) return 'When you choose an option';
		if (inbox.actions.includes('reply')) return 'When you send a reply';
		if (inbox.actions.includes('dismiss')) return 'When you dismiss or source resolves';
		return 'Clears when its source is resolved';
	}
	return decision.impact.sub;
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

/**
 * Derives the unified list of open and handled triage items from canonical work items and inbox items.
 */
export function deriveTriageItems(
	workItems: readonly WorkItemSummary[],
	snapshot: OrgSnapshot,
	ownerId: string,
	inboxItems: readonly InboxItem[],
	nowMs = Date.now()
): TriageItem[] {
	const memberMap = new Map(snapshot.members.map((m) => [m.id, m]));
	const teamMap = new Map(snapshot.teams.map((t) => [t.id, t]));
	const areaMap = new Map((snapshot.areas ?? []).map((a) => [a.id, a]));
	const workByRef = new Map(workItems.map((w) => [w.short_id.toUpperCase(), w]));

	const openDecisions = deriveDecisionSpotlight(workItems, snapshot, ownerId, nowMs, inboxItems);

	const openTriage: TriageItem[] = openDecisions.map((decision) => {
		const inbox = decision.inboxItem;
		const work = decision.item;
		const actorId = inbox?.who.id ?? work?.assignee_id ?? work?.created_by;
		const member = actorId ? memberMap.get(actorId) : undefined;
		const isAgent = inbox ? inbox.who.kind === 'agent' : member?.kind === 'agent';

		let teamId = inbox?.team?.id;
		let teamName = inbox?.team?.name;
		if (!teamId && work?.area_id) {
			const area = areaMap.get(work.area_id);
			if (area?.team_id) {
				const team = teamMap.get(area.team_id);
				teamId = team?.id ?? area.team_id;
				teamName = team?.name;
			}
		}

		const category: TriageItem['category'] =
			decision.sourceKind === 'work'
				? 'work'
				: decision.category === 'approval' ||
					  decision.category === 'decision' ||
					  decision.category === 'question' ||
					  decision.category === 'problem'
					? decision.category
					: 'work';

		const workRef = extractWorkRef(inbox?.open?.path, inbox?.open?.label, decision.title, work);
		const matchedWork = work ?? (workRef ? workByRef.get(workRef.toUpperCase()) : undefined);
		const workRefDisplay = matchedWork ? `${matchedWork.short_id} · ${matchedWork.title}` : workRef;
		const resolvedProjectId = inbox?.project?.id ?? decision.projectId ?? matchedWork?.project_id;
		const rawOpenPath =
			inbox?.open?.path ?? (decision.sourceKind === 'work' ? decision.detailHref : undefined);
		const workHref = workRef ? safeWorkListHref(resolvedProjectId) : undefined;
		const openHref = sanitizeOpenHref(rawOpenPath, resolvedProjectId);
		const relatedHref = openHref && !isUnbuiltWorkRefPath(rawOpenPath) ? openHref : undefined;
		const openLabel =
			inbox?.open?.label ??
			(decision.sourceKind === 'work'
				? work
					? `Open ${work.short_id}`
					: 'Open work item'
				: undefined);

		const createdIso = inbox?.created_at ?? work?.updated_at;
		const recommendation = inbox
			? inbox.recommendation
			: decision.recommendation !== decision.backgroundContext
				? decision.recommendation
				: undefined;

		const expiresLabel = inbox?.expires_at
			? decision.badgeText
			: decision.sourceKind === 'work' && decision.urgent
				? decision.badgeText
				: undefined;

		return {
			id: decision.id,
			state: 'open',
			sourceKind: decision.sourceKind,
			category,
			decision,
			inboxItem: inbox,
			workItem: matchedWork,
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
			projectId: resolvedProjectId,
			projectName: inbox?.project?.name ?? decision.projectName,
			hasProject: Boolean(resolvedProjectId),
			why: decision.backgroundContext,
			recommendation,
			recommendationTone: resolveRecommendationTone(category, decision.urgent),
			urgent: decision.urgent,
			expiresLabel,
			expiresAtIso: inbox?.expires_at,
			clearsLabel: resolveClearsLabel(decision),
			workRef,
			workRefDisplay,
			workHref,
			openHref,
			relatedHref,
			openLabel,
			auditChecks: decision.auditChecks,
			ageLabel: decision.ageLabel,
			createdAgeLabel: decision.ageLabel,
			timestampMs: parseTimestamp(createdIso, nowMs)
		};
	});

	const handledTriage: TriageItem[] = inboxItems
		.filter((item) => item.state === 'handled')
		.map((item): TriageItem => {
			const member = memberMap.get(item.who.id);
			const isAgent = item.who.kind === 'agent';
			const workRef = extractWorkRef(item.open?.path, item.open?.label, item.title);
			const matchedWork = workRef ? workByRef.get(workRef.toUpperCase()) : undefined;
			const workRefDisplay = matchedWork
				? `${matchedWork.short_id} · ${matchedWork.title}`
				: workRef;
			const resolvedProjectId = item.project?.id ?? matchedWork?.project_id;
			const rawOpenPath = item.open?.path;
			const workHref = workRef ? safeWorkListHref(resolvedProjectId) : undefined;
			const openHref = sanitizeOpenHref(rawOpenPath, resolvedProjectId);
			const relatedHref = openHref && !isUnbuiltWorkRefPath(rawOpenPath) ? openHref : undefined;
			const handledIso = item.handled?.at ?? item.created_at;
			const outcome = item.handled?.outcome ?? 'Resolved';

			const auditChecks: string[] = [];
			if (item.source) auditChecks.push(`Source: ${item.source}`);
			if (item.project?.name) auditChecks.push(`Project: ${item.project.name}`);
			if (item.team?.name) auditChecks.push(`Team: ${item.team.name}`);
			auditChecks.push(`Outcome: ${outcome}`);

			return {
				id: item.id,
				state: 'handled',
				sourceKind: 'inbox',
				category: item.kind,
				inboxItem: item,
				workItem: matchedWork,
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
				projectId: resolvedProjectId,
				projectName: item.project?.name ?? item.team?.name ?? 'Organization',
				hasProject: Boolean(resolvedProjectId),
				why: item.why,
				recommendation: item.recommendation,
				recommendationTone: 'ok',
				urgent: false,
				clearsLabel: `Handled · ${outcome}`,
				workRef,
				workRefDisplay,
				workHref,
				openHref,
				relatedHref,
				openLabel: item.open?.label,
				auditChecks,
				ageLabel: relativeAge(handledIso, nowMs),
				createdAgeLabel: relativeAge(item.created_at, nowMs),
				timestampMs: parseTimestamp(handledIso, nowMs),
				handled: {
					outcome,
					by: item.handled?.by,
					atIso: handledIso,
					atLabel: relativeAge(handledIso, nowMs)
				}
			};
		});

	return [...openTriage, ...handledTriage];
}

export function matchesScope(item: TriageItem, scope: InboxScope): boolean {
	if (scope.team && item.teamId !== scope.team) return false;
	if (scope.project && item.projectId !== scope.project) return false;
	return true;
}

export function matchesSearch(item: TriageItem, query: string): boolean {
	const needle = query.trim().toLowerCase();
	if (!needle) return true;
	const haystack = [
		item.title,
		item.actorName,
		item.actorRole,
		item.projectName,
		item.teamName ?? '',
		item.typeLabel,
		item.kindDisplay,
		item.workRef ?? '',
		item.workRefDisplay ?? '',
		item.why,
		item.recommendation ?? '',
		item.handled?.outcome ?? ''
	]
		.join(' ')
		.toLowerCase();
	return haystack.includes(needle);
}

function matchesTab(item: TriageItem, tab: TriageTab): boolean {
	if (tab === 'handled') return item.state === 'handled';
	if (item.state !== 'open') return false;
	switch (tab) {
		case 'all':
			return true;
		case 'approvals':
			return item.category === 'approval';
		case 'decisions':
			return item.category === 'decision';
		case 'questions':
			return item.category === 'question';
		case 'problems':
			return item.category === 'problem';
		case 'work':
			return item.sourceKind === 'work';
	}
}

export function sortTriageItems(
	items: readonly TriageItem[],
	tab: TriageTab,
	order: TriageOrder = 'priority'
): TriageItem[] {
	if (tab === 'handled' || order === 'date') {
		return [...items].sort((a, b) => b.timestampMs - a.timestampMs);
	}
	return [...items].sort((a, b) => {
		const aExp = a.expiresAtIso ? Date.parse(a.expiresAtIso) : Number.POSITIVE_INFINITY;
		const bExp = b.expiresAtIso ? Date.parse(b.expiresAtIso) : Number.POSITIVE_INFINITY;
		if (aExp !== bExp) return aExp - bExp;
		if (a.urgent !== b.urgent) return a.urgent ? -1 : 1;
		return b.timestampMs - a.timestampMs;
	});
}

export function filterTriageItems(
	items: readonly TriageItem[],
	options: TriageFilterOptions
): TriageItem[] {
	const matched = items.filter((item) => {
		if (!matchesTab(item, options.tab)) return false;
		if (options.tab !== 'handled' && options.urgentOnly && !item.urgent) return false;
		if (!matchesScope(item, options.scope)) return false;
		if (!matchesSearch(item, options.search)) return false;
		return true;
	});
	return sortTriageItems(matched, options.tab, options.order ?? 'priority');
}

export function triageTabCounts(
	items: readonly TriageItem[],
	scope: InboxScope = {}
): Record<TriageTab, number> {
	const counts: Record<TriageTab, number> = {
		all: 0,
		approvals: 0,
		decisions: 0,
		questions: 0,
		handled: 0,
		problems: 0,
		work: 0
	};
	for (const item of items) {
		if (!matchesScope(item, scope)) continue;
		if (item.state === 'handled') {
			counts.handled += 1;
			continue;
		}
		counts.all += 1;
		if (item.category === 'approval') counts.approvals += 1;
		else if (item.category === 'decision') counts.decisions += 1;
		else if (item.category === 'question') counts.questions += 1;
		else if (item.category === 'problem') counts.problems += 1;
		if (item.sourceKind === 'work') counts.work += 1;
	}
	return counts;
}

export function openWaitingCount(items: readonly TriageItem[]): number {
	return items.filter((i) => i.state === 'open').length;
}

export function triageScopeOptions(items: readonly TriageItem[]): {
	teams: { id: string; name: string }[];
	projects: { id: string; name: string }[];
} {
	const teams = new Map<string, string>();
	const projects = new Map<string, string>();
	for (const item of items) {
		if (item.teamId && item.teamName) teams.set(item.teamId, item.teamName);
		if (item.projectId && item.projectName) projects.set(item.projectId, item.projectName);
	}
	const toSorted = (map: Map<string, string>) =>
		[...map].map(([id, name]) => ({ id, name })).sort((a, b) => a.name.localeCompare(b.name));
	return { teams: toSorted(teams), projects: toSorted(projects) };
}

export function groupTriageItems(
	items: readonly TriageItem[],
	tab: TriageTab,
	nowMs = Date.now(),
	order: TriageOrder = 'priority'
): TriageGroup[] {
	if (items.length === 0) return [];
	if (tab === 'handled') {
		return [{ key: 'handled', label: 'Handled', items: [...items] }];
	}
	const urgent: TriageItem[] = [];
	const today: TriageItem[] = [];
	const earlier: TriageItem[] = [];
	for (const item of items) {
		if (order === 'priority' && item.urgent) urgent.push(item);
		else if (nowMs - item.timestampMs < DAY_MS) today.push(item);
		else earlier.push(item);
	}
	const groups: TriageGroup[] = [];
	if (urgent.length > 0) groups.push({ key: 'urgent', label: 'Urgent', items: urgent });
	if (today.length > 0) groups.push({ key: 'today', label: 'Today', items: today });
	if (earlier.length > 0) groups.push({ key: 'earlier', label: 'Earlier', items: earlier });
	return groups;
}

export function flattenGroups(groups: readonly TriageGroup[]): TriageItem[] {
	return groups.flatMap((g) => g.items);
}

/**
 * Returns ordered candidate IDs to select after `resolvedId` leaves `visibleItems`:
 * items after `resolvedId` first (in order), then items before `resolvedId` (nearest first).
 */
export function neighborCandidatesAfterResolve(
	visibleItems: readonly TriageItem[],
	resolvedId: string
): string[] {
	const index = visibleItems.findIndex((i) => i.id === resolvedId);
	if (index < 0) return visibleItems.map((i) => i.id);
	const after = visibleItems.slice(index + 1).map((i) => i.id);
	const before = visibleItems
		.slice(0, index)
		.reverse()
		.map((i) => i.id);
	return [...after, ...before];
}

/**
 * Returns the next item ID to select after `resolvedId` leaves the visible queue:
 * next neighbour first, otherwise previous neighbour, otherwise undefined.
 */
export function nextSelectionAfterResolve(
	visibleItems: readonly TriageItem[],
	resolvedId: string
): string | undefined {
	return neighborCandidatesAfterResolve(visibleItems, resolvedId)[0];
}

export function isDeniedOutcome(outcome: string | undefined): boolean {
	if (!outcome) return false;
	const lower = outcome.trim().toLowerCase();
	return lower.startsWith('denied') || lower.startsWith('declined');
}

/**
 * Splits a relative route href into its pathname and `?query`/`#hash` suffix so only the pathname
 * is passed to SvelteKit's `resolve()` (preventing bracket-syntax throws on dynamic fragments).
 */
export function splitHref(raw: string): { pathname: string; suffix: string } {
	const qIdx = raw.indexOf('?');
	const hIdx = raw.indexOf('#');
	let cut = -1;
	if (qIdx >= 0 && hIdx >= 0) cut = Math.min(qIdx, hIdx);
	else if (qIdx >= 0) cut = qIdx;
	else if (hIdx >= 0) cut = hIdx;
	if (cut < 0) return { pathname: raw || '/', suffix: '' };
	return { pathname: raw.slice(0, cut) || '/', suffix: raw.slice(cut) };
}
