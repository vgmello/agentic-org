// Pure presentation, template, front-matter synchronization, markdown block parsing, and line-diff helpers
// for the Project Docs screens (decisions-and-tech-debt-ux §0–§6, organization-data 18).
import type {
	DocField,
	DocKindMeta,
	DocumentDraft,
	DocumentLifecycle,
	DocumentScope,
	ProjectDocumentDetail,
	ProjectDocumentRow
} from './types';

export const docsHref = (projectId: string) => `/projects/${encodeURIComponent(projectId)}/docs`;

export const docHref = (projectId: string, docId: string) =>
	`${docsHref(projectId)}/${encodeURIComponent(docId)}`;

export function newDocHref(
	projectId: string,
	kind: string,
	extra: {
		template?: 'evaluation' | 'review';
		section?: string;
		supersedes?: string;
		featureId?: string;
	} = {}
): string {
	const params = new URLSearchParams({ kind });
	if (extra.template) params.set('template', extra.template);
	if (extra.section) params.set('section', extra.section);
	if (extra.supersedes) params.set('supersedes', extra.supersedes);
	if (extra.featureId) params.set('feature', extra.featureId);
	return `${docsHref(projectId)}/new?${params.toString()}`;
}

// ── Labels & tones ────────────────────────────────────────────────────────────────────────────────────────

export const LIFECYCLE_WORD: Record<DocumentLifecycle, string> = {
	draft: 'Draft',
	current: 'Current',
	deprecated: 'Deprecated'
};

export const LIFECYCLE_TONE: Record<DocumentLifecycle, 'info' | 'ok' | 'neutral'> = {
	draft: 'info',
	current: 'ok',
	deprecated: 'neutral'
};

export const BUILT_IN_KIND_TITLE: Record<string, string> = {
	brief: 'Brief',
	decision: 'Decision',
	design: 'Design',
	report: 'Report',
	changelog: 'Changelog'
};

export const BUILT_IN_KIND_PLURAL: Record<string, string> = {
	brief: 'Brief',
	decision: 'Decisions',
	design: 'Designs',
	report: 'Reports',
	changelog: 'Changelogs'
};

export function kindTitle(kind: string, kinds: readonly DocKindMeta[] = []): string {
	return kinds.find((k) => k.key === kind)?.title ?? BUILT_IN_KIND_TITLE[kind] ?? kind;
}

export function kindGroupTitle(kind: string, kinds: readonly DocKindMeta[] = []): string {
	if (BUILT_IN_KIND_PLURAL[kind]) return BUILT_IN_KIND_PLURAL[kind];
	const meta = kinds.find((k) => k.key === kind);
	return meta ? meta.title : kind;
}

export function scopeLabel(scope: DocumentScope, projectName: string): string {
	if (scope.kind === 'project') return `Project ${projectName}`;
	if (scope.kind === 'feature') return `${scope.ref} ${scope.title}`;
	if (scope.kind === 'release') return `${scope.ref} ${scope.title}`;
	return `${scope.releaseRef} · ${scope.environment}/${scope.target}`;
}

export function isTreeDocument(kind: string, scope: DocumentScope): boolean {
	return kind !== 'brief' && kind !== 'changelog' && scope.kind === 'project';
}

export function defaultSectionForKind(kind: string): string | null {
	if (kind === 'decision') return 'decisions';
	if (kind === 'design') return 'designs';
	if (kind === 'report') return 'reports';
	if (kind === 'product-dss') return 'product';
	if (kind === 'technical-dss') return 'technical';
	return null;
}

// ── Templates & expected sections (decisions-and-tech-debt-ux §3, TPL-1) ─────────────────────────────────

export interface DocTemplateSpec {
	kind: string;
	label: string;
	description: string;
	sections: { heading: string; hint: string }[];
	defaultBody: string;
}

export const BUILT_IN_TEMPLATES: Record<string, DocTemplateSpec> = {
	brief: {
		kind: 'brief',
		label: 'Brief',
		description:
			'The living overview of what the project is, whom it serves, and how it works now.',
		sections: [
			{ heading: 'What it is', hint: 'No project overview written yet.' },
			{ heading: 'Who it serves', hint: 'No intended users listed yet.' },
			{ heading: 'How it works now', hint: 'No current behavior described yet.' },
			{
				heading: 'Current known version and availability',
				hint: 'No version or availability evidence recorded yet.'
			}
		],
		defaultBody:
			'## What it is\n\n## Who it serves\n\n## How it works now\n\n## Current known version and availability\n'
	},
	decision: {
		kind: 'decision',
		label: 'Decision',
		description:
			'A recorded architecture decision: context, options, decision, consequences, and evidence.',
		sections: [
			{ heading: 'Context', hint: 'No context written yet.' },
			{ heading: 'Options considered', hint: 'No options listed yet.' },
			{ heading: 'Decision', hint: 'No decision statement written yet.' },
			{ heading: 'Consequences', hint: 'No consequences written yet.' },
			{ heading: 'Evidence', hint: 'No evidence linked yet.' },
			{ heading: 'Tech debt accepted', hint: 'No accepted tech debt noted.' },
			{ heading: 'Not covered here', hint: 'No boundaries noted yet.' }
		],
		defaultBody:
			'## Context\n\n## Options considered\n\n## Decision\n\n## Consequences\n\n## Evidence\n\n## Tech debt accepted\n\n## Not covered here\n'
	},
	design: {
		kind: 'design',
		label: 'Design',
		description:
			'How a capability or system component will be implemented, with flow, states, and risks.',
		sections: [
			{ heading: 'Goal', hint: 'No goal written yet.' },
			{ heading: 'Users and jobs', hint: 'No users or jobs listed yet.' },
			{ heading: 'Flow', hint: 'No flow described yet.' },
			{ heading: 'States', hint: 'No states listed yet.' },
			{ heading: 'Risks', hint: 'No risks noted yet.' },
			{ heading: 'Acceptance criteria', hint: 'No acceptance criteria written yet.' },
			{ heading: 'Open questions', hint: 'No open questions listed.' }
		],
		defaultBody:
			'## Goal\n\n## Users and jobs\n\n## Flow\n\n## States\n\n## Risks\n\n## Acceptance criteria\n\n## Open questions\n'
	},
	report: {
		kind: 'report',
		label: 'Report',
		description: 'Findings, reviews, or evaluations backed by evidence.',
		sections: [
			{ heading: 'Summary', hint: 'No summary written yet.' },
			{ heading: 'Findings', hint: 'No findings listed yet.' },
			{ heading: 'Evidence', hint: 'No evidence linked yet.' },
			{ heading: 'Open decisions', hint: 'No open decisions noted.' }
		],
		defaultBody: '## Summary\n\n## Findings\n\n## Evidence\n\n## Open decisions\n'
	},
	evaluation: {
		kind: 'report',
		label: 'Evaluation (report)',
		description:
			'A comparison or spike that answers a question with criteria, evidence, and a verdict.',
		sections: [
			{ heading: 'Question', hint: 'No evaluation question written yet.' },
			{ heading: 'Options', hint: 'No options listed yet.' },
			{ heading: 'Criteria', hint: 'No evaluation criteria listed yet.' },
			{ heading: 'Evidence', hint: 'No measured evidence recorded yet.' },
			{ heading: 'Verdict', hint: 'No verdict written yet.' },
			{ heading: 'Left open', hint: 'No open questions recorded.' }
		],
		defaultBody:
			'## Question\n\n## Options\n\n## Criteria\n\n## Evidence\n\n## Verdict\n\n## Left open\n'
	},
	review: {
		kind: 'report',
		label: 'Review (report)',
		description: 'A review report with subject, verdict, severity-tagged findings, and validation.',
		sections: [
			{ heading: 'Subject and commit', hint: 'No subject or commit recorded yet.' },
			{ heading: 'Summary and verdict', hint: 'No verdict written yet.' },
			{ heading: 'Findings', hint: 'No findings recorded yet.' },
			{ heading: 'Validation', hint: 'No validation notes recorded yet.' },
			{ heading: 'Open decisions', hint: 'No open decisions noted.' }
		],
		defaultBody:
			'## Subject and commit\n\n## Summary and verdict\n\n## Findings\n\n## Validation\n\n## Open decisions\n'
	}
};

export function expectedSectionsFor(
	kind: string,
	bodyMarkdown: string,
	kinds: readonly DocKindMeta[] = []
): { heading: string; hint: string; present: boolean; empty: boolean }[] {
	// Detect evaluation report by headings when kind is report
	const parsed = extractSections(bodyMarkdown);
	const parsedMap = new Map(parsed.map((s) => [s.heading.toLowerCase(), s]));
	const isEvaluation = kind === 'report' && (parsedMap.has('question') || parsedMap.has('verdict'));
	const tplKey = isEvaluation ? 'evaluation' : kind;
	const builtin = BUILT_IN_TEMPLATES[tplKey];
	const custom = kinds.find((k) => k.key === kind);
	const expected: { heading: string; hint: string }[] = builtin
		? builtin.sections
		: (custom?.expectedSections ?? []).map((h) => ({
				heading: h,
				hint: `No ${h.toLowerCase()} written yet.`
			}));

	const seen = new Set<string>();
	const result: { heading: string; hint: string; present: boolean; empty: boolean }[] = [];
	for (const exp of expected) {
		const hit = parsedMap.get(exp.heading.toLowerCase());
		seen.add(exp.heading.toLowerCase());
		result.push({
			heading: hit?.heading ?? exp.heading,
			hint: exp.hint,
			present: !!hit,
			empty: !hit || hit.content.trim().length === 0
		});
	}
	for (const sec of parsed) {
		if (!seen.has(sec.heading.toLowerCase())) {
			result.push({
				heading: sec.heading,
				hint: '',
				present: true,
				empty: sec.content.trim().length === 0
			});
		}
	}
	return result;
}

export function templateBodyFor(
	kind: string,
	templateVariant?: 'evaluation' | 'review',
	kinds: readonly DocKindMeta[] = []
): string {
	if (templateVariant && BUILT_IN_TEMPLATES[templateVariant]) {
		return BUILT_IN_TEMPLATES[templateVariant].defaultBody;
	}
	if (BUILT_IN_TEMPLATES[kind]) return BUILT_IN_TEMPLATES[kind].defaultBody;
	const custom = kinds.find((k) => k.key === kind);
	if (custom?.templateMarkdown) return custom.templateMarkdown;
	return '## Purpose\n\n## Details\n';
}

// ── Path & slug derivation ───────────────────────────────────────────────────────────────────────────────

export function slugifyFileName(title: string): string {
	const base = title
		.normalize('NFKD')
		.replace(/[\u0300-\u036f]/g, '')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '')
		.slice(0, 64)
		.replace(/-+$/g, '');
	return base ? `${base}.md` : 'untitled.md';
}

export function deriveCanonicalPath(
	projectId: string,
	docId: string,
	kind: string,
	scope: DocumentScope,
	sectionPath: string | null,
	fileName: string | null
): string {
	if (kind === 'brief') return `projects/${projectId}/brief.md`;
	if (scope.kind === 'feature') {
		const folder = kind === 'decision' ? 'decisions' : kind === 'report' ? 'reports' : 'designs';
		return `projects/${projectId}/features/${scope.featureId}/${folder}/${docId}.md`;
	}
	if (scope.kind === 'release') {
		if (kind === 'changelog')
			return `projects/${projectId}/releases/${scope.releaseId}/changelog.md`;
		return `projects/${projectId}/releases/${scope.releaseId}/documents/${docId}.md`;
	}
	if (scope.kind === 'deployment') {
		if (kind === 'changelog') {
			return `projects/${projectId}/releases/${scope.releaseId}/deployments/${scope.deploymentId}/changelog.md`;
		}
		return `projects/${projectId}/releases/${scope.releaseId}/deployments/${scope.deploymentId}/documents/${docId}.md`;
	}
	const sec = (sectionPath ?? defaultSectionForKind(kind) ?? 'general').replace(/^\/+|\/+$/g, '');
	const name = (fileName?.trim() || 'document.md').replace(/^\/+/, '');
	const normalized = name.endsWith('.md') ? name : `${name}.md`;
	return `projects/${projectId}/docs/${sec}/${normalized}`;
}

export function fileNameFromPath(path: string): string {
	const idx = path.lastIndexOf('/');
	return idx >= 0 ? path.slice(idx + 1) : path;
}

// ── Validation ────────────────────────────────────────────────────────────────────────────────────────────

const FILE_NAME_RE = /^[a-z0-9][a-z0-9._-]*\.md$/;
const SECTION_PATH_RE = /^[a-z0-9][a-z0-9/_-]*[a-z0-9]$|^[a-z0-9]$/;

export function checkDocumentDraft(draft: DocumentDraft): Partial<Record<DocField, string>> {
	const errors: Partial<Record<DocField, string>> = {};
	if (!draft.title.trim()) errors.title = 'Enter a title for this document.';
	if (draft.key && draft.key.trim().length > 64) {
		errors.key = 'Use at most 64 characters for the display key.';
	}
	if (draft.status === 'current' && !draft.ownerId) {
		errors.ownerId = 'A current document needs an owner. Choose one, or keep it as Draft.';
	}
	if (isTreeDocument(draft.kind, draft.scope)) {
		const sec = draft.sectionPath?.trim() ?? '';
		if (!sec || !SECTION_PATH_RE.test(sec) || sec.includes('//')) {
			errors.sectionPath =
				'Choose a valid section path using lowercase letters, digits, hyphens, underscores and slashes.';
		}
		const rawName = draft.fileName?.trim() ?? '';
		const withExt = rawName.endsWith('.md') ? rawName : `${rawName}.md`;
		if (!rawName || !FILE_NAME_RE.test(withExt) || withExt.includes('..')) {
			errors.fileName =
				'Use lowercase letters, digits, hyphens, underscores or single dots ending in .md.';
		}
	}
	return errors;
}

export function checkSectionPath(path: string): string | undefined {
	const clean = path.trim().replace(/^\/+|\/+$/g, '');
	if (!clean) return 'Enter a folder path such as architecture or product/checkout.';
	if (clean.length > 256 || !SECTION_PATH_RE.test(clean) || clean.includes('//')) {
		return 'Use lowercase letters, digits, hyphens, underscores and single slashes.';
	}
	return undefined;
}

// ── Front matter <-> Details form synchronization (TPL-2) ─────────────────────────────────────────────────

function yamlScalar(value: string): string {
	const plain =
		/^[A-Za-z0-9][\w ./()-]*$/.test(value) &&
		!/\s$/.test(value) &&
		!/^(?:y|yes|n|no|true|false|on|off|null|~)$/i.test(value);
	return plain ? value : JSON.stringify(value);
}

/** Formats a draft with its immutable identity metadata as synchronized YAML front matter + Markdown body. */
export function serializeDocumentSource(
	draft: DocumentDraft,
	meta: { id: string; revision: number; projectId: string; path: string }
): string {
	const lines = [
		'---',
		'schema_version: 1',
		`id: ${meta.id}`,
		`revision: ${meta.revision}`,
		`key: ${draft.key?.trim() ? yamlScalar(draft.key.trim()) : 'null'}`,
		`project_id: ${meta.projectId}`,
		`kind: ${draft.kind}`,
		`status: ${draft.status}`,
		`owner_employee_id: ${draft.ownerId ?? 'null'}`,
		`title: ${yamlScalar(draft.title.trim())}`,
		`summary: ${yamlScalar(draft.summary.trim())}`,
		`path: ${meta.path}`,
		'---'
	];
	return `${lines.join('\n')}\n${draft.bodyMarkdown}`;
}

function unquoteYaml(raw: string): string | null {
	const trimmed = raw.trim();
	if (!trimmed || trimmed === 'null' || trimmed === '~') return null;
	if (
		(trimmed.startsWith('"') && trimmed.endsWith('"')) ||
		(trimmed.startsWith("'") && trimmed.endsWith("'"))
	) {
		try {
			if (trimmed.startsWith('"')) return JSON.parse(trimmed) as string;
			return trimmed.slice(1, -1).replaceAll("''", "'");
		} catch {
			return trimmed.slice(1, -1);
		}
	}
	return trimmed;
}

/**
 * Parses Source mode back into editable draft fields, keeping immutable identity (`id`, `project_id`, `kind`)
 * intact from `current`. Returns `{ ok: true, draft }` or `{ ok: false, error }`.
 */
export function parseDocumentSource(
	source: string,
	current: DocumentDraft
): { ok: true; draft: DocumentDraft } | { ok: false; error: string } {
	const match = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/.exec(source);
	if (!match) {
		return {
			ok: false,
			error: 'Source must start with YAML front matter enclosed by --- lines.'
		};
	}
	const [, yamlBlock, body] = match;
	const fields = new Map<string, string>();
	for (const line of yamlBlock.split(/\r?\n/)) {
		const trimmed = line.trim();
		if (!trimmed || trimmed.startsWith('#')) continue;
		const colon = trimmed.indexOf(':');
		if (colon < 1) continue;
		fields.set(trimmed.slice(0, colon).trim(), trimmed.slice(colon + 1).trim());
	}

	const nextStatusRaw = unquoteYaml(fields.get('status') ?? current.status);
	if (nextStatusRaw !== 'draft' && nextStatusRaw !== 'current' && nextStatusRaw !== 'deprecated') {
		return {
			ok: false,
			error: `Status must be draft, current, or deprecated (got "${nextStatusRaw ?? ''}").`
		};
	}
	const title = unquoteYaml(fields.get('title') ?? current.title) ?? '';
	const summary = unquoteYaml(fields.get('summary') ?? current.summary) ?? '';
	const key = fields.has('key') ? unquoteYaml(fields.get('key')!) : current.key;
	const ownerId = fields.has('owner_employee_id')
		? unquoteYaml(fields.get('owner_employee_id')!)
		: current.ownerId;

	return {
		ok: true,
		draft: {
			...current,
			title,
			summary,
			key,
			status: nextStatusRaw,
			ownerId,
			bodyMarkdown: body
		}
	};
}

// ── Markdown parsing (rendered strictly as safe elements, never @html) ───────────────────────────────────

export type DocMarkdownBlock =
	| { kind: 'heading'; level: number; text: string; slug: string }
	| { kind: 'list'; ordered: boolean; items: string[] }
	| { kind: 'paragraph'; text: string }
	| { kind: 'code'; text: string };

export function headingSlug(text: string): string {
	return (
		text
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '-')
			.replace(/^-+|-+$/g, '') || 'section'
	);
}

export function extractSections(
	body: string
): { heading: string; slug: string; content: string }[] {
	const sections: { heading: string; slug: string; content: string }[] = [];
	let current: { heading: string; slug: string; lines: string[] } | null = null;
	for (const raw of body.split(/\r?\n/)) {
		const h2 = /^##\s+(.*?)\s*#*$/.exec(raw.trim());
		if (h2 && h2[1]) {
			if (current) {
				sections.push({
					heading: current.heading,
					slug: current.slug,
					content: current.lines.join('\n').trim()
				});
			}
			current = { heading: h2[1], slug: headingSlug(h2[1]), lines: [] };
		} else if (current) {
			current.lines.push(raw);
		}
	}
	if (current) {
		sections.push({
			heading: current.heading,
			slug: current.slug,
			content: current.lines.join('\n').trim()
		});
	}
	return sections;
}

export function markdownBlocks(body: string): DocMarkdownBlock[] {
	const blocks: DocMarkdownBlock[] = [];
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
		const heading = /^(#{1,6})\s+(.*?)\s*#*$/.exec(line);
		const bullet = /^[-*+]\s+(.*)$/.exec(line);
		const numbered = /^\d+[.)]\s+(.*)$/.exec(line);
		if (!line) flush();
		else if (heading && heading[2]) {
			flush();
			blocks.push({
				kind: 'heading',
				level: heading[1].length,
				text: heading[2],
				slug: headingSlug(heading[2])
			});
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

// ── Line diff for history & conflict comparison ──────────────────────────────────────────────────────────

export interface DiffLine {
	kind: 'same' | 'added' | 'removed';
	text: string;
}

export function diffLines(before: string, after: string): DiffLine[] {
	const a = before.replace(/\n$/, '').split('\n');
	const b = after.replace(/\n$/, '').split('\n');
	const n = a.length;
	const m = b.length;
	const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
	for (let i = n - 1; i >= 0; i--) {
		for (let j = m - 1; j >= 0; j--) {
			lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
		}
	}
	const out: DiffLine[] = [];
	let i = 0;
	let j = 0;
	while (i < n && j < m) {
		if (a[i] === b[j]) {
			out.push({ kind: 'same', text: a[i] });
			i++;
			j++;
		} else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
			out.push({ kind: 'removed', text: a[i++] });
		} else {
			out.push({ kind: 'added', text: b[j++] });
		}
	}
	while (i < n) out.push({ kind: 'removed', text: a[i++] });
	while (j < m) out.push({ kind: 'added', text: b[j++] });
	return out;
}

export function diffSummary(lines: readonly DiffLine[]): string {
	const added = lines.filter((l) => l.kind === 'added').length;
	const removed = lines.filter((l) => l.kind === 'removed').length;
	if (!added && !removed) return 'No differences';
	const part = (n: number, word: string) => `${n} ${n === 1 ? 'line' : 'lines'} ${word}`;
	return [added && part(added, 'added'), removed && part(removed, 'removed')]
		.filter(Boolean)
		.join(', ');
}

// ── Filtering & grouping (decisions-and-tech-debt-ux §2, DEC-1, DEC-8) ───────────────────────────────────

export interface DocsFilter {
	status: 'all' | DocumentLifecycle;
	ownerId: string;
	query: string;
}

export function matchesDocFilter(doc: ProjectDocumentRow, filter: DocsFilter): boolean {
	if (filter.status !== 'all' && doc.status !== filter.status) return false;
	if (filter.ownerId && doc.owner?.id !== filter.ownerId) return false;
	const q = filter.query.trim().toLowerCase();
	if (!q) return true;
	return [
		doc.title,
		doc.id,
		doc.key ?? '',
		doc.summary,
		doc.path,
		doc.bodyPreview ?? '',
		doc.evaluationExcerpt?.question ?? '',
		doc.evaluationExcerpt?.verdict ?? ''
	].some((text) => text.toLowerCase().includes(q));
}

export interface DocKindGroup {
	kind: string;
	label: string;
	docs: ProjectDocumentRow[];
}

const KIND_ORDER = ['decision', 'design', 'report'];

export function groupDocsByKind(
	docs: readonly ProjectDocumentRow[],
	kinds: readonly DocKindMeta[]
): {
	brief: ProjectDocumentRow | null;
	groups: DocKindGroup[];
	deprecated: ProjectDocumentRow[];
} {
	let brief: ProjectDocumentRow | null = null;
	const deprecated: ProjectDocumentRow[] = [];
	const buckets = new Map<string, ProjectDocumentRow[]>();

	for (const doc of docs) {
		if (doc.status === 'deprecated') {
			deprecated.push(doc);
			continue;
		}
		if (doc.kind === 'brief' && !brief) {
			brief = doc;
			continue;
		}
		const list = buckets.get(doc.kind) ?? [];
		list.push(doc);
		buckets.set(doc.kind, list);
	}

	const customOrder = kinds.filter((k) => !k.builtIn).map((k) => k.key);
	const orderedKeys = [
		...KIND_ORDER,
		...customOrder,
		'changelog',
		...[...buckets.keys()].filter(
			(k) => !KIND_ORDER.includes(k) && !customOrder.includes(k) && k !== 'changelog'
		)
	];

	const groups: DocKindGroup[] = [];
	for (const key of orderedKeys) {
		const list = buckets.get(key);
		if (list && list.length > 0) {
			groups.push({ kind: key, label: kindGroupTitle(key, kinds), docs: list });
		}
	}

	return { brief, groups, deprecated };
}

export function draftFromDetail(doc: ProjectDocumentDetail): DocumentDraft {
	return {
		kind: doc.kind,
		title: doc.title,
		key: doc.key,
		summary: doc.summary,
		status: doc.status,
		ownerId: doc.owner?.id ?? null,
		sectionPath: doc.sectionPath,
		fileName: isTreeDocument(doc.kind, doc.scope) ? fileNameFromPath(doc.path) : null,
		scope: doc.scope,
		bodyMarkdown: doc.bodyMarkdown
	};
}

export function conflictLatestDoc(e: unknown): ProjectDocumentDetail | undefined {
	if (!e || typeof e !== 'object' || (e as { code?: unknown }).code !== 'conflict') {
		return undefined;
	}
	const latest = (e as { latest?: unknown }).latest;
	return latest && typeof latest === 'object' && 'revision' in latest
		? (latest as ProjectDocumentDetail)
		: undefined;
}
