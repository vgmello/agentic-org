import { parseNormalizedInternalUrl } from '$lib/platform/http/safe-url';
import type { WorkItemSummary } from '$lib/proto';
import type { SpotlightDecision } from '../../spotlight';
import type { InboxItem, InboxScope } from '../../types';

export type TriageTab =
	'all' | 'approvals' | 'decisions' | 'questions' | 'snoozed' | 'handled' | 'problems' | 'work';

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
	{ id: 'snoozed', label: 'Snoozed' },
	{ id: 'handled', label: 'Handled' }
];

export const ALL_TRIAGE_TABS: readonly TriageTabDef[] = [...PRIMARY_TABS, ...OVERFLOW_TABS];

export function isOverflowTab(tab: TriageTab): boolean {
	return OVERFLOW_TABS.some((t) => t.id === tab);
}

export function queueSummaryLabel(tab: TriageTab, order: TriageOrder = 'priority'): string {
	if (isOverflowTab(tab)) {
		if (tab === 'handled') return 'Handled items';
		if (tab === 'snoozed') return 'Snoozed items';
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
	snooze: 'snoozed',
	snoozed: 'snoozed',
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

export type TriageGroupKey = 'urgent' | 'today' | 'earlier' | 'snoozed' | 'handled';

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
	state: 'open' | 'snoozed' | 'handled';
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
	snoozed?: {
		untilIso?: string;
		note?: string;
	};
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

function parseCommentEntry(entry: unknown, index: number): InboxCommentEntry | undefined {
	if (!entry || typeof entry !== 'object') return undefined;
	const rec = entry as Record<string, unknown>;
	if (typeof rec.text !== 'string') return undefined;
	const trimmed = rec.text.trim();
	if (!trimmed || rec.text.length > MAX_INBOX_COMMENT_LENGTH) return undefined;
	if (typeof rec.at !== 'number' || !Number.isFinite(rec.at)) return undefined;
	if (!Number.isFinite(new Date(rec.at).getTime())) return undefined;
	const explicitId = typeof rec.id === 'string' ? rec.id.trim() : '';
	return { id: explicitId || `c-${rec.at}-${index}`, text: trimmed, at: rec.at };
}

export function parseInboxCommentsJson(raw: string | null): InboxCommentEntry[] {
	if (!raw) return [];
	try {
		const parsed: unknown = JSON.parse(raw);
		if (!Array.isArray(parsed)) return [];
		const valid: InboxCommentEntry[] = [];
		const seenIds = new Set<string>();
		for (let i = 0; i < parsed.length; i++) {
			const comment = parseCommentEntry(parsed[i], i);
			if (!comment || seenIds.has(comment.id)) continue;
			seenIds.add(comment.id);
			valid.push(comment);
		}
		return valid.sort((a, b) => a.at - b.at);
	} catch {
		return [];
	}
}

export function isInternalAppPath(path: string | undefined): path is string {
	if (!path) return false;
	const trimmed = path.trim();
	return trimmed.startsWith('/') && !trimmed.startsWith('//') && !trimmed.startsWith('/\\');
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
