// Organization processes, standards and development processes (docs/specs/processes-ux.md; the records are
// docs/design/organization-data/01-organization-structure.md `structure_documents` and document 18). The office
// resolves which documents apply; the browser shows what it is told and never computes an effective set itself.

/** What a structure document is: how the organization handles something, or what it uses and how it builds. */
export type DocumentKind = 'process' | 'standard';
export type Lifecycle = 'draft' | 'current' | 'deprecated';
/** Where a document lives: the organization root, a team's folder (any depth) or a project's. */
export type Level = 'organization' | 'team' | 'project';

/** Common attribution (record contracts): who changed a record and when. */
export interface Attribution {
	at: string;
	byKind: 'employee' | 'system' | 'unknown';
	byId?: string;
	/** The employee's name, when the office knows it. */
	byName?: string;
}

export interface EmployeeRef {
	id: string;
	name: string;
}

/** One process or standard file: its front matter, where it lives and its Markdown text. */
export interface ProcessDocument {
	id: string;
	/** Matches a document across levels; a lower level's document with the same key replaces the one above. */
	key?: string;
	revision: number;
	kind: DocumentKind;
	title: string;
	status: Lifecycle;
	/** Accountable owner; required for `current`. */
	owner?: EmployeeRef;
	/** The level and scope from the file's path. */
	level: Level;
	teamId?: string;
	projectId?: string;
	/** The team's or project's name; none at organization level. */
	scopeName?: string;
	overridable: boolean;
	bodyMarkdown: string;
	/** The file in organization Git, e.g. `teams/eng/teams/web/processes/<id>.md`. */
	path: string;
	created: Attribution;
	updated: Attribution;
	/** Front matter the office could not read (PROC-16): the file is listed with this and left out of every set. */
	invalid?: { field: string };
}

/** A place the Processes page can show: the organization, a team, a project or an agent. */
export type Place =
	| { kind: 'organization' }
	| { kind: 'team'; id: string }
	| { kind: 'project'; id: string }
	| { kind: 'agent'; id: string };

/** One Show option (PROC-34): "Organization", "Web (team)", "Atlas (project)", "Noor Haddad (agent)". */
export interface PlaceOption {
	place: Place;
	name: string;
}

/** What happened to one level's document of a key, for "why" (PROC-8, PROC-9). */
export type LayerOutcome =
	| 'applies'
	| 'replaced'
	/** It tried to replace a document marked not overridable. */
	| 'refused'
	/** Its front matter isn't recognized. */
	| 'invalid'
	/** Two documents use its key at one level and scope. */
	| 'clash';

export interface Layer {
	document: ProcessDocument;
	outcome: LayerOutcome;
}

/** A problem that keeps a document out of the effective set, shown on its row. */
export type RowProblem =
	/** `by`: the document above that is not overridable. */
	| { kind: 'refused'; by: ProcessDocument }
	| { kind: 'invalid'; field: string }
	/** `scopeName`: where the two documents clash ("Checkout"); `others`: the other documents with the key. */
	| { kind: 'clash'; scopeName: string; others: ProcessDocument[] };

/**
 * One row of the inheritance view: a key matched across levels (or one document without a key). `document` is the
 * one that applies, or, when none applies, the one the row is about. `chain` lists every level's document for the
 * key, highest first.
 */
export interface EffectiveRow {
	/** The key, or the document's ID when it has none. */
	id: string;
	document: ProcessDocument;
	/** Whether the document is in the effective set for this place. */
	applies: boolean;
	/** The document it replaces, when it replaces one above. */
	replaces?: ProcessDocument;
	/** The document comes from the place being shown ("added here") rather than from above. */
	here: boolean;
	problem?: RowProblem;
	chain: Layer[];
}

/** The organization repository couldn't be reached: the rows are the last copy, from `copiedAt`. */
export interface StaleCopy {
	copiedAt: string;
}

/** What one place follows. */
export interface PlaceView {
	place: Place;
	/** The Show label of the place. */
	name: string;
	options: PlaceOption[];
	processes: EffectiveRow[];
	standards: EffectiveRow[];
	/** Drafts and deprecated documents written at this place: listed, never applied. */
	inactive: ProcessDocument[];
	/** For a team or project: the level documents written here go to. */
	writable: boolean;
	/** The project's development process, for a project or an agent working in one (PROC-35). */
	developmentProcess?: { id: string; name: string };
	/** For an agent: its current work, or none (PROC-10). */
	agent?: { work?: { ref: string; title: string; projectName?: string } };
	stale?: StaleCopy;
	viewer: { canEdit: boolean };
}

/** A new document, or an override of `replacesId` with the same key at a lower level. */
export interface DocumentDraft {
	kind: DocumentKind;
	title: string;
	key?: string;
	status: Lifecycle;
	ownerId?: string;
	overridable: boolean;
	teamId?: string;
	projectId?: string;
	bodyMarkdown: string;
}

export type DocumentField = 'title' | 'key' | 'status' | 'ownerId' | 'overridable' | 'bodyMarkdown';

/** A document and what the editor needs around it. */
export interface DocumentDetail {
	document: ProcessDocument;
	/** The document it replaces (same key, the level above), for "difference from the parent". */
	parent?: ProcessDocument;
}

/** What the editor offers: who may own a document, and the places it may be written at. */
export interface EditorOptions {
	owners: EmployeeRef[];
	places: PlaceOption[];
	viewer: { canEdit: boolean };
}

// ── Development processes (document 18) ──────────────────────────────────────────────────────────────────────

export type DevProcessOrigin = 'built_in' | 'organization';

export interface DevRole {
	key: string;
	name: string;
	hats: string[];
	seed: boolean;
	bodyMarkdown: string;
}

export interface DevSkill {
	key: string;
	title: string;
	description: string;
	/** Role keys; empty means every role of the process. */
	roles: string[];
	bodyMarkdown: string;
}

export interface DevTemplate {
	key: string;
	bodyMarkdown: string;
}

export interface DevStage {
	key: string;
	title: string;
	summary?: string;
	leadRole?: string;
}

export interface DevTab {
	key: string;
	title: string;
	view: 'documents' | 'work' | 'features' | 'releases';
}

export interface DevFolder {
	path: string;
	title: string;
	summary?: string;
}

export interface DevProcessSummary {
	id: string;
	key: string;
	name: string;
	summary: string;
	versionLabel?: string;
	revision: number;
	status: Lifecycle;
	origin: DevProcessOrigin;
	owner?: EmployeeRef;
	seedRoles: { key: string; name: string }[];
	tabs: DevTab[];
	folders: DevFolder[];
	/** Projects that follow it. */
	projects: { id: string; name: string }[];
	invalid?: { field: string };
}

export interface DevProcessCatalogue {
	processes: DevProcessSummary[];
	stale?: StaleCopy;
	viewer: { canEdit: boolean };
}

export interface DevProcessDetail extends DevProcessSummary {
	roles: DevRole[];
	skills: DevSkill[];
	templates: DevTemplate[];
	stages: DevStage[];
	kinds: { key: string; title: string; versioned: boolean }[];
	/** `process.yaml` as written. */
	manifestYaml: string;
	copiedFrom?: { id: string; name: string };
	updated: Attribution;
	stale?: StaleCopy;
	viewer: { canEdit: boolean };
}

/** One file of a development process: the manifest, or a role, skill or template by key. */
export type DevPart =
	| { kind: 'manifest' }
	| { kind: 'role'; key: string }
	| { kind: 'skill'; key: string }
	| { kind: 'template'; key: string };

/** The process file a download gives and an import takes. */
export interface ProcessFile {
	fileName: string;
	blob: Blob;
}
