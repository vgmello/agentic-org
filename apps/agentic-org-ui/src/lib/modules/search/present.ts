// Pure helpers for knowledge search ("Why is…") and coverage disclosure (decisions-and-tech-debt-ux §5, WHY-4).
import type { KnowledgeHit, SearchCoverage } from './types';

const WHY_PREFIX_RE =
	/^(?:why\s+(?:is|are|did|do|does|was|were|not|we|have|has|can't|cannot)\b|why[:?]\s*)/i;

const STOP_WORDS = new Set([
	'why',
	'is',
	'are',
	'did',
	'do',
	'does',
	'was',
	'were',
	'we',
	'the',
	'a',
	'an',
	'for',
	'to',
	'of',
	'in',
	'on',
	'it',
	'this',
	'that',
	'used',
	'use',
	'have',
	'has',
	'be',
	'into',
	'with',
	'and',
	'or'
]);

/** Detects a "Why is…" knowledge question (decisions-and-tech-debt-ux §5). */
export function isWhyQuery(query: string): boolean {
	return WHY_PREFIX_RE.test(query.trim());
}

/** Extracts meaningful search terms, stripping question scaffolding for "Why is…" queries. */
export function extractSearchTerms(query: string): string[] {
	const cleaned = query
		.trim()
		.toLowerCase()
		.replace(/[?!.,;:"'()[\]]+/g, ' ');
	const raw = cleaned.split(/\s+/).filter(Boolean);
	if (!isWhyQuery(query)) return raw;
	const filtered = raw.filter((w) => !STOP_WORDS.has(w) && w.length >= 2);
	return filtered.length > 0 ? filtered : raw;
}

const KNOWLEDGE_ORDER = [
	'decision',
	'evaluation',
	'design',
	'feature',
	'report',
	'brief',
	'changelog'
];

export interface KnowledgeGroup {
	recordType: string;
	label: string;
	hits: KnowledgeHit[];
}

const GROUP_LABELS: Record<string, string> = {
	decision: 'Decisions',
	evaluation: 'Evaluations',
	design: 'Designs',
	feature: 'Features',
	report: 'Reports',
	brief: 'Briefs',
	changelog: 'Changelogs'
};

export function groupKnowledgeHits(hits: readonly KnowledgeHit[]): KnowledgeGroup[] {
	const buckets = new Map<string, KnowledgeHit[]>();
	for (const hit of hits) {
		const list = buckets.get(hit.recordType) ?? [];
		list.push(hit);
		buckets.set(hit.recordType, list);
	}
	const ordered = [
		...KNOWLEDGE_ORDER,
		...[...buckets.keys()].filter((k) => !KNOWLEDGE_ORDER.includes(k))
	];
	const groups: KnowledgeGroup[] = [];
	for (const key of ordered) {
		const list = buckets.get(key);
		if (list && list.length > 0) {
			groups.push({
				recordType: key,
				label: GROUP_LABELS[key] ?? list[0].kindLabel,
				hits: list
			});
		}
	}
	return groups;
}

/** Honest coverage statement (WHY-4): states scope, partial/title-only limits, and exclusions. */
export function coverageStatement(coverage: SearchCoverage): string {
	if (coverage.scope === 'unavailable') {
		return (
			coverage.note ??
			'Knowledge search index is currently unavailable. No matches does not prove absence. Private task notes, protected chat and inaccessible records are always excluded.'
		);
	}
	if (coverage.scope === 'title_only') {
		return (
			coverage.note ??
			'Title-only search: matching permitted titles, display keys and stable IDs only (body text is not indexed yet). No matches does not rule out body mentions. Private task notes, protected chat and inaccessible records are excluded.'
		);
	}
	const trunc = coverage.truncatedBodies ? ' Long document bodies are truncated in search.' : '';
	return `Searching permitted decisions, evaluations, designs, reports, briefs and feature definitions (titles, stable IDs, display keys and body text).${trunc} Private task notes, protected chat and inaccessible records are excluded.`;
}

export const WHY_EXAMPLES = [
	'Why is SQLite used for office storage?',
	'Why is checkout split into two services?',
	'Why is Turso not chosen?'
] as const;
