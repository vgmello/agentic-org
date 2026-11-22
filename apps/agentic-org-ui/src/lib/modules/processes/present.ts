// Words, addresses and checks for the Processes screens (processes-ux §2, decision 7). Pure, so they are unit
// tested; the screens only arrange them.
import type {
	DocumentDraft,
	DocumentField,
	DocumentKind,
	EffectiveRow,
	Level,
	Lifecycle,
	Place,
	PlaceOption,
	ProcessDocument
} from './types';

export const PROCESSES_URL = '/settings/processes';
export const DEV_PROCESSES_URL = '/settings/processes/development';

// ── The three things, named apart (decision 7, PROC-37) ──────────────────────────────────────────────────────

export const PAGE_SUBTITLE = 'Organization processes and standards';
export const KIND_TAB: Record<DocumentKind, string> = {
	process: 'Processes',
	standard: 'Standards'
};
export const KIND_DEFINITION: Record<DocumentKind, string> = {
	process: 'How the organization handles something, whatever is being built',
	standard: 'What the organization uses, and how it builds and runs things'
};
/** "process" / "standard", inside a sentence. */
export const KIND_WORD: Record<DocumentKind, string> = { process: 'process', standard: 'standard' };
export const KIND_PLURAL: Record<DocumentKind, string> = {
	process: 'processes',
	standard: 'standards'
};
export const DEV_HEADING = 'Development processes';
export const DEV_SUBTITLE = 'How a project builds software';

export const LEVEL_WORD: Record<Level, string> = {
	organization: 'Organization',
	team: 'Team',
	project: 'Project'
};
export const LIFECYCLE_WORD: Record<Lifecycle, string> = {
	draft: 'Draft',
	current: 'Current',
	deprecated: 'Deprecated'
};

// ── Places and addresses (PROC-31, PROC-34) ───────────────────────────────────────────────────────────────────

const PLACE_KINDS = ['team', 'project', 'agent'] as const;

/** `?place=team:web` → a place; anything else is the organization. */
export function parsePlace(value: string | null | undefined): Place {
	if (!value) return { kind: 'organization' };
	const at = value.indexOf(':');
	const kind = value.slice(0, at) as (typeof PLACE_KINDS)[number];
	const id = value.slice(at + 1);
	if (at < 1 || !id || !PLACE_KINDS.includes(kind)) return { kind: 'organization' };
	return { kind, id };
}

/** The URL value of a place; none for the organization. */
export const placeParam = (place: Place): string | undefined =>
	place.kind === 'organization' ? undefined : `${place.kind}:${place.id}`;

/** Where a document is written: its team, its project or the organization. */
export const placeOfDocument = (doc: Pick<ProcessDocument, 'teamId' | 'projectId'>): Place =>
	doc.teamId
		? { kind: 'team', id: doc.teamId }
		: doc.projectId
			? { kind: 'project', id: doc.projectId }
			: { kind: 'organization' };

export const samePlace = (a: Place, b: Place) => placeParam(a) === placeParam(b);

/** "Organization", "Web (team)", "Atlas (project)", "Noor Haddad (agent)" (PROC-34). */
export const placeLabel = (option: PlaceOption): string =>
	option.place.kind === 'organization' ? 'Organization' : `${option.name} (${option.place.kind})`;

export function parseKind(value: string | null | undefined): DocumentKind {
	return value === 'standard' ? 'standard' : 'process';
}

function query(params: Record<string, string | undefined>): string {
	const search = new URLSearchParams();
	for (const [name, value] of Object.entries(params)) if (value) search.set(name, value);
	const text = search.toString();
	return text ? `?${text}` : '';
}

/** The Processes page for a place and tab. */
export const listHref = (place: Place, kind: DocumentKind = 'process') =>
	`${PROCESSES_URL}${query({ place: placeParam(place), kind: kind === 'standard' ? kind : undefined })}`;

/** The editor for a new document: blank, from a template, or overriding `from` (same key, new ID). */
export const newDocumentHref = (
	place: Place,
	kind: DocumentKind,
	extra: { from?: string; template?: string } = {}
) =>
	`${PROCESSES_URL}/documents/new${query({ place: placeParam(place), kind, from: extra.from, template: extra.template })}`;

/** The editor for an existing document; `place` and `kind` say where to go back to. */
export const documentHref = (documentId: string, place: Place, kind: DocumentKind) =>
	`${PROCESSES_URL}/documents/${encodeURIComponent(documentId)}${query({ place: placeParam(place), kind: kind === 'standard' ? kind : undefined })}`;

export const devProcessHref = (processId: string) =>
	`${DEV_PROCESSES_URL}/${encodeURIComponent(processId)}`;

// ── Rows (PROC-9, PROC-16, PROC-18) ───────────────────────────────────────────────────────────────────────────

/** "Organization", "Team Web", "Project Atlas": where a document comes from. */
export const sourceText = (doc: ProcessDocument) =>
	doc.level === 'organization'
		? 'Organization'
		: `${LEVEL_WORD[doc.level]}${doc.scopeName ? ` ${doc.scopeName}` : ''}`;

/** How a row relates to the level above: "replaces Organization", "added here" or "inherited". */
export function relationText(row: EffectiveRow): string {
	if (row.replaces) return `replaces ${sourceText(row.replaces)}`;
	if (row.here) return 'added here';
	return 'inherited';
}

/** "the organization", "team Platform": who marked a document not overridable. */
const markedBy = (doc: ProcessDocument) =>
	doc.level === 'organization' ? 'the organization' : `${doc.level} ${doc.scopeName ?? ''}`.trim();

/** The spec's words for a row's problem (processes-ux §2 States). */
export function problemText(row: EffectiveRow): string | undefined {
	const doc = row.document;
	const problem = row.problem;
	if (!problem) return undefined;
	if (problem.kind === 'refused') {
		const whose = doc.scopeName ?? LEVEL_WORD[doc.level];
		return `${whose}'s version of ${problem.by.title} isn't applied: ${markedBy(problem.by)} marked it not overridable. You can add a ${KIND_WORD[doc.kind]} instead.`;
	}
	if (problem.kind === 'invalid') return invalidText(doc.title, problem.field);
	return clashText(doc.kind, doc.key ?? '', problem.scopeName);
}

/** "Design handoff: details not recognized (overridable)." */
export const invalidText = (title: string, field: string) =>
	`${title}: details not recognized (${field}).`;

/** The parts of the clash sentence, so the key can be shown as code without literal backticks. */
export function clashParts(kind: DocumentKind, scopeName: string) {
	return {
		before: `Two ${KIND_PLURAL[kind]} use the key`,
		after: `in ${scopeName}. Neither applies until one is renamed.`
	};
}

export const clashText = (kind: DocumentKind, key: string, scopeName: string) => {
	const parts = clashParts(kind, scopeName);
	return `${parts.before} ${key} ${parts.after}`;
};

/** The row's trailing state: "Applies" or "Not applied". */
export const appliesText = (row: EffectiveRow) => (row.applies ? 'Applies' : 'Not applied');

/** Override is offered for an overridable, applying document inherited from above a team or project place. */
export function canOverride(row: EffectiveRow, place: Place): boolean {
	if (place.kind !== 'team' && place.kind !== 'project') return false;
	return !row.here && row.applies && row.document.overridable && !!row.document.key && !row.problem;
}

/** The agent line (PROC-10). */
export const NO_CURRENT_WORK =
	"No current work. Showing the organization's processes. Team and project processes apply when it starts a task.";

export const agentWorkText = (name: string, ref: string, title: string) =>
	`What ${name} follows for ${ref} ${title}, and where each comes from.`;

export const EMPTY_TITLE: Record<DocumentKind, string> = {
	process: 'No processes yet',
	standard: 'No standards yet'
};
export const EMPTY_TEXT: Record<DocumentKind, string> = {
	process: 'Write down how your agents work. Start with how changes get reviewed and merged.',
	standard: 'Write down what the organization uses: its technologies, tools and how it deploys.'
};

export const staleText = (shown: string) =>
	`Can't reach the organization repository. Showing the last copy from ${shown}.`;
export const CHANGES_PAUSED = 'Changes wait until the organization repository can be reached.';

// ── The editor (PROC-1, PROC-3, PROC-36) ──────────────────────────────────────────────────────────────────────

/** The usual headings of a new document (processes-ux §1). They are prose, not enforced fields. */
export const DOCUMENT_TEMPLATE = '## Purpose\n\n## Procedure\n\n## Review and completion\n';

export const REVIEW_AND_MERGE = {
	key: 'review-and-merge',
	title: 'Review and merge',
	bodyMarkdown: `## Purpose
How a change gets from a task branch into the integration branch.

## Procedure
1. Keep the change to one task. Say what it does and why in its description.
2. Run the project's checks before asking for review.
3. Ask a teammate who did not write the change to review it.
4. Answer every comment: change the code, or say why not.

## Review and completion
The change is merged when its checks pass and the reviewer has approved it. Close the task once it is merged.
`
} as const;

export const TEMPLATES: Record<string, { key: string; title: string; bodyMarkdown: string }> = {
	[REVIEW_AND_MERGE.key]: REVIEW_AND_MERGE
};

const KEY = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/** Trims the typed text and drops an empty key. */
export function normalizeDraft(draft: DocumentDraft): DocumentDraft {
	const key = draft.key?.trim();
	return {
		...draft,
		title: draft.title.trim(),
		key: key ? key : undefined,
		bodyMarkdown: draft.bodyMarkdown.replace(/\s+$/, '') + '\n'
	};
}

/** Checks the browser can make before sending (the office decides): the field and what to do. */
export function checkDraft(draft: DocumentDraft): Partial<Record<DocumentField, string>> {
	const errors: Partial<Record<DocumentField, string>> = {};
	if (!draft.title) errors.title = 'Give it a title.';
	if (draft.key && (draft.key.length > 64 || !KEY.test(draft.key)))
		errors.key = 'Use 1 to 64 lowercase letters, digits and single hyphens, like review-and-merge.';
	if (draft.status === 'current' && !draft.ownerId)
		errors.ownerId = 'A current document needs an owner. Choose one, or save it as a draft.';
	return errors;
}

export const sameDraft = (a: DocumentDraft, b: DocumentDraft) =>
	JSON.stringify(normalizeDraft(a)) === JSON.stringify(normalizeDraft(b));

export const draftOf = (doc: ProcessDocument): DocumentDraft => ({
	kind: doc.kind,
	title: doc.title,
	key: doc.key,
	status: doc.status,
	ownerId: doc.owner?.id,
	overridable: doc.overridable,
	teamId: doc.teamId,
	projectId: doc.projectId,
	bodyMarkdown: doc.bodyMarkdown
});

const FIELDS: DocumentField[] = [
	'title',
	'key',
	'status',
	'ownerId',
	'overridable',
	'bodyMarkdown'
];

/** The editor field a refusal is about, when the office named one. */
export function refusedField(e: unknown): DocumentField | undefined {
	const field = (e as { field?: unknown } | null)?.field;
	return FIELDS.includes(field as DocumentField) ? (field as DocumentField) : undefined;
}

/** The document as it is now, from a revision conflict. */
export function conflictLatest(e: unknown): ProcessDocument | undefined {
	if (!e || typeof e !== 'object' || (e as { code?: unknown }).code !== 'conflict')
		return undefined;
	const latest = (e as { latest?: unknown }).latest;
	return latest && typeof latest === 'object' && 'revision' in latest
		? (latest as ProcessDocument)
		: undefined;
}

// ── Difference from the parent (§2 "one click away") ──────────────────────────────────────────────────────────

export type DiffLine = { kind: 'same' | 'added' | 'removed'; text: string };

/** A line difference from `before` to `after` (longest common subsequence). */
export function diffLines(before: string, after: string): DiffLine[] {
	const a = before.replace(/\n$/, '').split('\n');
	const b = after.replace(/\n$/, '').split('\n');
	const n = a.length;
	const m = b.length;
	// lcs[i][j]: the common length of a[i..] and b[j..].
	const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
	for (let i = n - 1; i >= 0; i--)
		for (let j = m - 1; j >= 0; j--)
			lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
	const out: DiffLine[] = [];
	let i = 0;
	let j = 0;
	while (i < n && j < m) {
		if (a[i] === b[j]) {
			out.push({ kind: 'same', text: a[i] });
			i++;
			j++;
		} else if (lcs[i + 1][j] >= lcs[i][j + 1]) out.push({ kind: 'removed', text: a[i++] });
		else out.push({ kind: 'added', text: b[j++] });
	}
	while (i < n) out.push({ kind: 'removed', text: a[i++] });
	while (j < m) out.push({ kind: 'added', text: b[j++] });
	return out;
}

/** "3 lines added, 1 removed", or "No differences". */
export function diffSummary(lines: DiffLine[]): string {
	const added = lines.filter((l) => l.kind === 'added').length;
	const removed = lines.filter((l) => l.kind === 'removed').length;
	if (!added && !removed) return 'No differences';
	const plural = (count: number, word: string) =>
		`${count} ${count === 1 ? 'line' : 'lines'} ${word}`;
	return [added && plural(added, 'added'), removed && plural(removed, 'removed')]
		.filter(Boolean)
		.join(', ');
}

// ── Development processes (document 18) ───────────────────────────────────────────────────────────────────────

export const VIEW_WORD = {
	documents: 'Documents',
	work: 'Work',
	features: 'Features',
	releases: 'Releases'
} as const;

/** "Product, Technical, Releases". */
export const listText = (items: { title?: string; name?: string }[]) =>
	items.map((i) => i.title ?? i.name ?? '').join(', ');

export const projectsText = (count: number) =>
	count === 0 ? 'No project follows it' : count === 1 ? '1 project' : `${count} projects`;

export const skillRolesText = (roles: string[], names: Map<string, string>) =>
	roles.length === 0 ? 'every role' : roles.map((key) => names.get(key) ?? key).join(', ');

/** A development process key, as document 18 allows. */
export const validKey = (key: string) => key.length <= 64 && KEY.test(key);
export const KEY_HELP =
	'1 to 64 lowercase letters, digits and single hyphens. Unique in the organization.';

// ── Reading a document (rendered as elements, never as HTML) ──────────────────────────────────────────────────

export type Block =
	| { kind: 'heading'; text: string }
	| { kind: 'list'; ordered: boolean; items: string[] }
	| { kind: 'paragraph'; text: string }
	/** A fenced code block, its lines kept as written. */
	| { kind: 'code'; text: string };

/** Headings, lists and paragraphs of a Markdown body; everything else reads as text. */
export function markdownBlocks(body: string): Block[] {
	const blocks: Block[] = [];
	let paragraph: string[] = [];
	const flush = () => {
		if (paragraph.length) blocks.push({ kind: 'paragraph', text: paragraph.join(' ') });
		paragraph = [];
	};
	let code: string[] | undefined;
	for (const raw of body.split(/\r?\n/)) {
		const line = raw.trim();
		if (code) {
			if (/^(```|~~~)/.test(line)) {
				blocks.push({ kind: 'code', text: code.join('\n') });
				code = undefined;
			} else code.push(raw);
			continue;
		}
		if (/^(```|~~~)/.test(line)) {
			flush();
			code = [];
			continue;
		}
		const heading = /^#{1,6}(?:\s+(.*?))?\s*#*$/.exec(line);
		const bullet = /^[-*+]\s+(.*)$/.exec(line);
		const numbered = /^\d+[.)]\s+(.*)$/.exec(line);
		if (!line) flush();
		else if (heading) {
			flush();
			if (heading[1]) blocks.push({ kind: 'heading', text: heading[1] });
		} else if (bullet || numbered) {
			flush();
			const ordered = !!numbered;
			const text = (bullet ?? numbered)![1];
			const last = blocks.at(-1);
			if (last?.kind === 'list' && last.ordered === ordered) last.items.push(text);
			else blocks.push({ kind: 'list', ordered, items: [text] });
		} else paragraph.push(line);
	}
	flush();
	if (code) blocks.push({ kind: 'code', text: code.join('\n') });
	return blocks;
}

/** A YAML scalar as written: plain only when it can't be read as anything but this text. */
export function yamlScalar(value: string): string {
	const plain =
		/^[A-Za-z][\w ./()-]*$/.test(value) &&
		!/\s$/.test(value) &&
		!/^(?:y|yes|n|no|true|false|on|off|null|~)$/i.test(value);
	return plain ? value : JSON.stringify(value);
}

/** A skill file: its front matter and body (document 18). Without a key, the key line is added when it is saved. */
export function skillFileText(skill: {
	key?: string;
	title: string;
	description: string;
	roles: string[];
	bodyMarkdown: string;
}): string {
	const key = skill.key ? `key: ${skill.key}\n` : '';
	return `---\nschema_version: 1\n${key}title: ${yamlScalar(skill.title)}\ndescription: ${yamlScalar(skill.description)}\nroles: [${skill.roles.join(', ')}]\n---\n${skill.bodyMarkdown}`;
}

/** The skill file with `key` set in its front matter to the key it is saved under. */
export function withSkillKey(text: string, key: string): string {
	const match = /^---\n([\s\S]*?)\n---/.exec(text);
	if (!match) return text;
	const lines = match[1].split('\n').filter((line) => !/^key:/.test(line));
	const at = lines.findIndex((line) => /^schema_version:/.test(line));
	lines.splice(at + 1, 0, `key: ${key}`);
	return `---\n${lines.join('\n')}\n---${text.slice(match[0].length)}`;
}

/** "roles/product-lead.md", "process.yaml". */
export function partPath(part: { kind: string; key?: string }): string {
	return part.kind === 'manifest' ? 'process.yaml' : `${part.kind}s/${part.key}.md`;
}

export const NEW_SKILL_BODY = '## When\n\n## Steps\n\n## Done when\n';
export const NEW_ROLE_BODY = '## Mission\n\n## Responsibilities\n\n## Working style\n';
