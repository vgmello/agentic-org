// Mock for src/lib/modules/search/api.ts: searches Projects, Work, Members, and permitted project
// Knowledge (decisions, evaluations, designs, reports, briefs, and feature definitions) for "Why is..."
// queries (decisions-and-tech-debt-ux §5, WHY-3, WHY-4). Excludes private task notes and protected chat.
import { orgApi } from '$lib/modules/organization/api';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { SearchPageApi } from '../../../src/lib/modules/search/api';
import type { KnowledgeHit, SearchPageResults } from '../../../src/lib/modules/search/types';
import { extractSearchTerms } from '../../../src/lib/modules/search/present';
import type { SearchHit } from '../../../src/lib/modules/shell/types';
import { mockLatency, mockMode } from '../mock-mode';
import work from '../data/shell/work.json' with { type: 'json' };
import { allMockDocuments } from './docs';
import { allMockFeatures } from './features';

const on = (flag: string) => mockMode().split(',').includes(flag);
const fail = (code: string, message: string) =>
	Object.assign(new Error(message), { source: 'api', code, message });

const has = (q: string, ...t: (string | undefined)[]) =>
	t.some((x) => x?.toLowerCase().includes(q.toLowerCase()));

function matchesTerms(terms: string[], ...fields: (string | null | undefined)[]): boolean {
	if (terms.length === 0) return false;
	const combined = fields
		.filter((f): f is string => !!f)
		.join(' ')
		.toLowerCase();
	return terms.some((term) => combined.includes(term));
}

function excerptFromBody(body: string, terms: string[]): string | null {
	const lines = body
		.split(/\r?\n/)
		.map((l) => l.trim())
		.filter((l) => l && !l.startsWith('#'));
	for (const line of lines) {
		const lower = line.toLowerCase();
		if (terms.some((t) => lower.includes(t))) {
			return line.length > 180 ? `${line.slice(0, 177)}…` : line;
		}
	}
	return lines[0] ? (lines[0].length > 160 ? `${lines[0].slice(0, 157)}…` : lines[0]) : null;
}

const CAP = (s: string) => (s ? s[0].toUpperCase() + s.slice(1).replaceAll('_', ' ') : s);

export const searchApi: SearchPageApi = {
	async results(organizationId, query): Promise<SearchPageResults> {
		const { snapshot } = await orgApi.view(organizationId);
		await mockLatency(250);
		if (on('unavailable')) await unavailableCall();
		if (on('unsupported'))
			throw fail('unsupported', "This version of the office doesn't support knowledge search.");
		if (on('error') || on('fails')) throw fail('internal', 'The office did not answer in time.');

		const terms = extractSearchTerms(query);
		const titleOnly = on('title-only');
		const projectNames = new Map((snapshot.projects ?? []).map((p) => [p.id, p.name]));

		const projects: SearchHit[] = (snapshot.projects ?? [])
			.filter((p) => !p.archived && has(query, p.name, p.key))
			.map((p) => ({
				id: p.id,
				title: p.name,
				meta: p.key || undefined,
				href: `/projects/${p.id}`
			}));

		const items: SearchHit[] = work
			.filter((w) => has(query, w.ref, w.title))
			.map((w) => ({
				id: w.ref,
				title: w.title,
				meta: `${w.ref} · ${w.state}`,
				href: `/work/${w.ref}`
			}));

		const members: SearchHit[] = snapshot.members
			.filter((m) => has(query, m.name, m.agent?.title))
			.map((m) => ({
				id: m.id,
				title: m.name,
				meta: m.kind === 'agent' ? `Agent${m.agent?.title ? ` · ${m.agent.title}` : ''}` : 'Person',
				href: `/members/${m.id}`
			}));

		const knowledge: KnowledgeHit[] = [];

		for (const doc of allMockDocuments()) {
			const matched = titleOnly
				? matchesTerms(terms, doc.title, doc.key, doc.id, doc.summary)
				: matchesTerms(
						terms,
						doc.title,
						doc.key,
						doc.id,
						doc.summary,
						doc.bodyMarkdown,
						doc.evaluationExcerpt?.question,
						doc.evaluationExcerpt?.verdict
					);
			if (!matched) continue;
			const isEval = !!doc.evaluationExcerpt;
			const pName = projectNames.get(doc.projectId) ?? doc.projectId;
			const scopeText =
				doc.scope.kind === 'feature'
					? `${pName} › ${doc.scope.ref}`
					: doc.scope.kind === 'release'
						? `${pName} › ${doc.scope.ref}`
						: doc.scope.kind === 'deployment'
							? `${pName} › ${doc.scope.releaseRef} (${doc.scope.environment}/${doc.scope.target})`
							: pName;
			knowledge.push({
				id: doc.id,
				recordType: isEval ? 'evaluation' : doc.kind,
				kindLabel: isEval ? 'Evaluation (report)' : CAP(doc.kind),
				key: doc.key,
				title: doc.title,
				scopeLabel: scopeText,
				status: CAP(doc.status),
				updatedAt: doc.updatedAt,
				excerpt: isEval
					? `${doc.evaluationExcerpt!.question} → ${doc.evaluationExcerpt!.verdict}`
					: titleOnly
						? null
						: excerptFromBody(doc.bodyMarkdown, terms),
				excerptLabel: isEval ? 'Evaluation excerpt' : 'Excerpt',
				href: `/projects/${encodeURIComponent(doc.projectId)}/docs/${encodeURIComponent(doc.id)}`
			});
		}

		for (const feat of allMockFeatures()) {
			const ref = feat.number === null ? 'Pending number' : `F-${feat.number}`;
			const matched = titleOnly
				? matchesTerms(terms, feat.title, ref, feat.localSlug, feat.id)
				: matchesTerms(terms, feat.title, ref, feat.localSlug, feat.id, feat.bodyMarkdown);
			if (!matched) continue;
			const pName = projectNames.get(feat.projectId) ?? feat.projectId;
			knowledge.push({
				id: feat.id,
				recordType: 'feature',
				kindLabel: 'Feature',
				key: ref,
				title: feat.title,
				scopeLabel: pName,
				status: CAP(feat.status),
				updatedAt: feat.updatedAt,
				excerpt: titleOnly ? null : excerptFromBody(feat.bodyMarkdown, terms),
				excerptLabel: 'Excerpt',
				href: `/projects/${encodeURIComponent(feat.projectId)}/features/${encodeURIComponent(feat.id)}`
			});
		}

		return {
			projects,
			work: items,
			members,
			knowledge,
			coverage: {
				scope: titleOnly ? 'title_only' : 'full_body',
				indexedAt: new Date(Date.now() - 2 * 60_000).toISOString()
			}
		};
	}
};
