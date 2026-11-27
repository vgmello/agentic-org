import { replaceState } from '$app/navigation';
import { resolve } from '$app/paths';
import {
	defaultSectionForKind,
	isTreeDocument,
	slugifyFileName,
	templateBodyFor
} from '../../present';
import type {
	DocumentDraft,
	DocumentScope,
	ProjectDocsOverview,
	ProjectDocumentDetail
} from '../../types';

export interface CompareSnapshot {
	key: string;
	label: string;
	body: string;
}

export function clearEditQueryParamFromUrl(url: URL, state: App.PageState): void {
	if (!url.searchParams.has('edit')) return;
	const kept = [...url.searchParams.entries()]
		.filter(([k]) => k !== 'edit')
		.map(([k, v]) =>
			v ? `${encodeURIComponent(k)}=${encodeURIComponent(v)}` : encodeURIComponent(k)
		);
	const next = kept.length > 0 ? `${url.pathname}?${kept.join('&')}` : url.pathname;
	replaceState(resolve(next as '/'), state);
}

function resolveFeatureScope(
	featureParam: string | null,
	overview: ProjectDocsOverview
): DocumentScope {
	if (!featureParam) return { kind: 'project' };
	const matchedDocScope = overview.documents.find(
		(d) => d.scope.kind === 'feature' && d.scope.featureId === featureParam
	)?.scope;
	if (matchedDocScope?.kind === 'feature') return matchedDocScope;
	const matchedLim = overview.knownLimitations.find((l) => l.featureId === featureParam);
	return {
		kind: 'feature',
		featureId: featureParam,
		ref: matchedLim?.featureRef ?? featureParam,
		title: matchedLim?.featureTitle ?? featureParam
	};
}

export function buildNewDocumentDraft(
	searchParams: URLSearchParams,
	overview: ProjectDocsOverview
): DocumentDraft {
	const kind = searchParams.get('kind') ?? 'decision';
	const templateParam = searchParams.get('template');
	const templateVariant =
		templateParam === 'evaluation' || templateParam === 'review' ? templateParam : undefined;
	const supersedesId = searchParams.get('supersedes');
	const superseded = supersedesId
		? overview.documents.find((d) => d.id === supersedesId)
		: undefined;
	const scope = resolveFeatureScope(searchParams.get('feature'), overview);
	const sectionParam =
		searchParams.get('section') ??
		defaultSectionForKind(kind) ??
		overview.sections[0]?.path ??
		'decisions';
	const treeDoc = isTreeDocument(kind, scope);

	let body = templateBodyFor(kind, templateVariant, overview.kinds);
	let initialTitle = '';
	if (superseded) {
		const label = superseded.key ? `${superseded.key} (${superseded.title})` : superseded.title;
		initialTitle = `Supersedes ${superseded.key ?? superseded.title}`;
		body = `## Context\nSupersedes ${label}.\n\n## Options considered\n\n## Decision\n\n## Consequences\n\n## Evidence\n\n## Tech debt accepted\n\n## Not covered here\n`;
	}

	return {
		kind,
		title: initialTitle,
		key: null,
		summary: '',
		status: 'draft',
		ownerId: overview.owners[0]?.id ?? null,
		sectionPath: treeDoc ? sectionParam : null,
		fileName: treeDoc ? slugifyFileName(initialTitle || `new-${kind}`) : null,
		scope,
		bodyMarkdown: body,
		supersedesId: supersedesId ?? null
	};
}

export function buildCompareSnapshots(doc: ProjectDocumentDetail | undefined): CompareSnapshot[] {
	if (!doc) return [];
	const list: CompareSnapshot[] = [
		{
			key: 'working',
			label: `Working copy (rev ${doc.revision})`,
			body: doc.bodyMarkdown
		}
	];
	for (const v of [...doc.versions].toReversed()) {
		list.push({
			key: `v:${v.version}`,
			label: `Issued v${v.version} (rev ${v.revision})`,
			body: v.bodyMarkdown
		});
	}
	for (const h of [...doc.history].toReversed()) {
		if (h.revision === doc.revision) continue;
		list.push({
			key: `rev:${h.revision}`,
			label: `Revision ${h.revision} · ${h.summary}`,
			body: h.bodyMarkdown
		});
	}
	return list;
}
