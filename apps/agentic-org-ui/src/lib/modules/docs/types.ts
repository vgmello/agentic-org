// What the Project Docs screens read and change (decisions-and-tech-debt-ux §0–§6,
// organization-data 02 & 18). Field names use browser camelCase while keeping canonical stable IDs,
// paths, lifecycle values (`draft | current | deprecated`), issued versions, and attribution.
import type { StaleCopy } from '$lib/modules/projects/types';

export type DocumentLifecycle = 'draft' | 'current' | 'deprecated';

export type BuiltInDocKind = 'brief' | 'decision' | 'design' | 'report' | 'changelog';

export interface DocEmployeeRef {
	id: string;
	name: string;
	kind: 'person' | 'agent';
}

export type DocumentScope =
	| { kind: 'project' }
	| { kind: 'feature'; featureId: string; ref: string; title: string }
	| { kind: 'release'; releaseId: string; ref: string; title: string }
	| {
			kind: 'deployment';
			deploymentId: string;
			releaseId: string;
			releaseRef: string;
			environment: string;
			target: string;
	  };

/** A document kind available in this project: built-in or declared by its development process. */
export interface DocKindMeta {
	key: string;
	title: string;
	versioned: boolean;
	builtIn: boolean;
	templateMarkdown?: string;
	expectedSections?: string[];
}

export interface DocumentBacklink {
	id: string;
	kind: 'task' | 'bug' | 'request' | 'document' | 'feature';
	ref?: string;
	key?: string;
	title: string;
	status: string;
	href: string;
}

export interface DocumentCodeLink {
	id: string;
	repositoryId: string;
	repositoryKey: string;
	path: string;
	symbol?: string;
	note: string;
	stale?: boolean;
}

export interface DerivedFromSource {
	documentId: string;
	title: string;
	key?: string | null;
	version: number;
}

/** An immutable issued version in `docs-history/<document-id>/<version>.md` (organization-data 18). */
export interface DocumentVersion {
	version: number;
	revision: number;
	title: string;
	bodyMarkdown: string;
	note: string;
	/** RFC 3339. */
	issuedAt: string;
	issuedBy: DocEmployeeRef;
	derivedFrom: DerivedFromSource[];
}

/** One Git revision entry of a document's working file. */
export interface DocumentRevisionEntry {
	revision: number;
	/** RFC 3339. */
	at: string;
	by: DocEmployeeRef;
	summary: string;
	title: string;
	bodyMarkdown: string;
}

export type DocumentProblem =
	| { kind: 'invalid_metadata'; field: string; message: string }
	| {
			kind: 'duplicate_key';
			key: string;
			others: { id: string; title: string; scopeLabel: string }[];
	  };

export interface EvaluationExcerpt {
	question: string;
	verdict: string;
}

export interface ProjectDocumentRow {
	id: string;
	projectId: string;
	/** Optional display key such as `ADR-0002`; never a primary key (TPL-8). */
	key: string | null;
	kind: string;
	title: string;
	summary: string;
	status: DocumentLifecycle;
	owner: DocEmployeeRef | null;
	/** Canonical organization-Git path, read-only for scoped docs, movable inside `docs/` for tree docs. */
	path: string;
	/** Section path inside `projects/<project-id>/docs/`, or null for brief / scoped docs. */
	sectionPath: string | null;
	scope: DocumentScope;
	revision: number;
	version: number | null;
	issuedRevision: number | null;
	hasUnissuedChanges: boolean;
	createdAt: string;
	createdBy: DocEmployeeRef;
	updatedAt: string;
	updatedBy: DocEmployeeRef;
	/** Count of locally accepted changes waiting to publish to the state branch (§0.2). */
	pendingPublication?: number;
	evaluationExcerpt?: EvaluationExcerpt;
	/** Short body preview for search matching. */
	bodyPreview?: string;
	problem?: DocumentProblem;
}

export interface ProjectDocumentDetail extends ProjectDocumentRow {
	bodyMarkdown: string;
	links: DocumentBacklink[];
	ledTo: DocumentBacklink[];
	mentionedIn: DocumentBacklink[];
	codeLinks: DocumentCodeLink[];
	versions: DocumentVersion[];
	history: DocumentRevisionEntry[];
	/** Other same-project versioned documents with issued versions, for `derived_from` selection. */
	derivableSources: DerivedFromSource[];
	canEdit: boolean;
	canIssue: boolean;
	stale?: StaleCopy;
}

export interface DocMediaEntry {
	id: string;
	path: string;
	type: 'image' | 'diagram';
	mediaType: string;
	bytes: number;
	title: string;
	summary: string;
}

/** One section of `projects/<project-id>/docs/index.yaml` (organization-data 18). */
export interface DocSection {
	path: string;
	title: string;
	summary: string;
	position: number;
	kinds: string[];
	media: DocMediaEntry[];
}

/** Known limitations stay in `feature.md` and are surfaced on the Docs tab per feature (LMT-1). */
export interface KnownLimitationItem {
	featureId: string;
	featureRef: string;
	featureTitle: string;
	featureStatus: string;
	text: string;
}

export interface ProjectDocsOverview {
	projectId: string;
	projectName: string;
	projectNumber: number;
	stateBranch: string;
	kinds: DocKindMeta[];
	sections: DocSection[];
	documents: ProjectDocumentRow[];
	knownLimitations: KnownLimitationItem[];
	owners: DocEmployeeRef[];
	/** Actual indexed search scope and freshness (WHY-4). */
	indexFreshness: {
		indexedAt: string;
		scope: 'full_body' | 'title_only';
		truncatedBodies?: boolean;
	};
	canEdit: boolean;
	stale?: StaleCopy;
}

export interface DocumentDraft {
	kind: string;
	title: string;
	key: string | null;
	summary: string;
	status: DocumentLifecycle;
	ownerId: string | null;
	sectionPath: string | null;
	fileName: string | null;
	scope: DocumentScope;
	bodyMarkdown: string;
	supersedesId?: string | null;
}

export type DocField =
	'title' | 'key' | 'status' | 'ownerId' | 'sectionPath' | 'fileName' | 'bodyMarkdown';
