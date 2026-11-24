// Full-page search and "Why is..." knowledge search contracts (decisions-and-tech-debt-ux §5, WHY-4).
import type { SearchResults } from '$lib/modules/shell/types';

export interface KnowledgeHit {
	id: string;
	recordType:
		'decision' | 'evaluation' | 'design' | 'report' | 'brief' | 'feature' | 'changelog' | string;
	kindLabel: string;
	key: string | null;
	title: string;
	scopeLabel: string;
	status: string;
	/** RFC 3339. */
	updatedAt: string;
	/** Body or evaluation Question -> Verdict match, always labelled as an excerpt (WHY-3, WHY-4). */
	excerpt: string | null;
	excerptLabel?: 'Excerpt' | 'Evaluation excerpt';
	href: string;
}

export interface SearchCoverage {
	scope: 'full_body' | 'title_only' | 'unavailable';
	/** RFC 3339. */
	indexedAt: string;
	truncatedBodies?: boolean;
	note?: string;
}

export interface SearchPageResults extends SearchResults {
	knowledge: KnowledgeHit[];
	coverage: SearchCoverage;
}
