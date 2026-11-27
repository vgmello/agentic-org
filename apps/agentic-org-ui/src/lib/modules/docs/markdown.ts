// YAML front-matter serialization/parsing, safe Markdown block parsing, and line-diff helpers
// for Project Docs (never rendered via @html).
import type { DocumentDraft } from './types';

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
