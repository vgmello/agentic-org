// Dev-mock fake for src/lib/modules/docs/api.ts under `bun run dev:mock`.
// Scenarios (`?mock=`): slow · fails · empty · unavailable · unsupported · viewer · stale · conflict · save-fails · title-only
import { orgApi } from '$lib/modules/organization/api';
import { newId } from '$lib/modules/organization/commands';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { DocsApi } from '../../../src/lib/modules/docs/api';
import type {
	DerivedFromSource,
	DocKindMeta,
	DocSection,
	KnownLimitationItem,
	ProjectDocsOverview,
	ProjectDocumentDetail,
	ProjectDocumentRow
} from '../../../src/lib/modules/docs/types';
import {
	deriveCanonicalPath,
	extractSections,
	isTreeDocument
} from '../../../src/lib/modules/docs/present';
import { mockMode } from '../mock-mode';
import docsSeed from '../data/docs/docs.json' with { type: 'json' };
import featuresSeed from '../data/features/features.json' with { type: 'json' };
import devData from '../data/processes/development-processes.json' with { type: 'json' };
import projectsSeed from '../data/projects/projects.json' with { type: 'json' };

interface ProjectDocsStore {
	stateBranch: string;
	sections: DocSection[];
	documents: ProjectDocumentDetail[];
}

const stores = new Map<string, ProjectDocsStore>();
let conflictedOnce = false;

const on = (flag: string) => mockMode().split(',').includes(flag);
const fail = (code: string, message: string, extra: Record<string, unknown> = {}) =>
	Object.assign(new Error(message), { source: 'api', code, message, ...extra });

const BUILT_IN_KINDS: DocKindMeta[] = [
	{ key: 'brief', title: 'Brief', versioned: false, builtIn: true },
	{ key: 'decision', title: 'Decision', versioned: false, builtIn: true },
	{ key: 'design', title: 'Design', versioned: false, builtIn: true },
	{ key: 'report', title: 'Report', versioned: false, builtIn: true },
	{ key: 'changelog', title: 'Changelog', versioned: false, builtIn: true }
];

const DEFAULT_SECTIONS: DocSection[] = [
	{
		path: 'decisions',
		title: 'Decisions',
		summary: 'Architecture decisions.',
		position: 0,
		kinds: ['decision'],
		media: []
	},
	{
		path: 'designs',
		title: 'Designs',
		summary: 'Implementation designs and specs.',
		position: 1,
		kinds: ['design'],
		media: []
	},
	{
		path: 'reports',
		title: 'Reports',
		summary: 'Evaluations and reviews.',
		position: 2,
		kinds: ['report'],
		media: []
	}
];

async function readDelay() {
	await new Promise((r) => setTimeout(r, on('slow') ? 2200 : 120));
	if (on('unavailable')) await unavailableCall();
	if (on('unsupported'))
		throw fail('unsupported', "This version of the office doesn't support project documentation.");
	if (on('fails'))
		throw fail('internal', "HQ didn't answer while loading project docs. Try again.");
}

async function writeDelay() {
	await new Promise((r) => setTimeout(r, on('slow') ? 2200 : 180));
	if (on('unavailable')) await unavailableCall();
	if (on('unsupported'))
		throw fail('unsupported', "This version of the office doesn't support editing documentation.");
	if (on('stale'))
		throw fail('storage_error', "HQ can't be reached, so nothing was saved. Try again.");
	if (on('viewer'))
		throw fail('not_authorized', "You don't have access to edit documents in this project.");
	if (on('save-fails'))
		throw fail('storage_error', "HQ couldn't save this document. Your draft is kept; try again.");
}

function getStore(projectId: string): ProjectDocsStore {
	let store = stores.get(projectId);
	if (store) return store;
	const raw = (docsSeed.projects as Record<string, unknown>)[projectId] as
		{ stateBranch: string; sections: DocSection[]; documents: ProjectDocumentDetail[] } | undefined;
	store = raw
		? {
				stateBranch: raw.stateBranch,
				sections: structuredClone(raw.sections),
				documents: structuredClone(raw.documents).map((d) => ({
					...d,
					derivableSources: [],
					canEdit: true,
					canIssue: true
				}))
			}
		: {
				stateBranch: 'org/state',
				sections: structuredClone(DEFAULT_SECTIONS),
				documents: []
			};
	stores.set(projectId, store);
	return store;
}

function kindsForProject(projectId: string): DocKindMeta[] {
	const processId = (devData.projects as Record<string, string>)[projectId];
	const proc = processId ? devData.processes.find((p) => p.id === processId) : undefined;
	const custom: DocKindMeta[] = (proc?.kinds ?? []).map((k) => {
		const tpl = proc?.templates?.find((t) => t.key === k.key);
		return {
			key: k.key,
			title: k.title,
			versioned: k.versioned,
			builtIn: false,
			templateMarkdown: tpl?.bodyMarkdown,
			expectedSections: tpl ? extractSections(tpl.bodyMarkdown).map((s) => s.heading) : []
		};
	});
	return [...BUILT_IN_KINDS, ...custom];
}

function knownLimitationsFor(projectId: string): KnownLimitationItem[] {
	const proj = (
		featuresSeed.projects as Record<
			string,
			{
				features: {
					id: string;
					number: number | null;
					title: string;
					status: string;
					bodyMarkdown: string;
				}[];
			}
		>
	)[projectId];
	if (!proj) return [];
	const items: KnownLimitationItem[] = [];
	for (const f of proj.features) {
		const sec = extractSections(f.bodyMarkdown).find(
			(s) => s.heading.toLowerCase() === 'known limitations'
		);
		if (sec && sec.content.trim()) {
			items.push({
				featureId: f.id,
				featureRef: f.number === null ? 'Pending number' : `F-${f.number}`,
				featureTitle: f.title,
				featureStatus:
					f.status === 'in_progress'
						? 'In progress'
						: f.status[0].toUpperCase() + f.status.slice(1),
				text: sec.content.trim()
			});
		}
	}
	return items;
}

function toRow(d: ProjectDocumentDetail): ProjectDocumentRow {
	return {
		id: d.id,
		projectId: d.projectId,
		key: d.key,
		kind: d.kind,
		title: d.title,
		summary: d.summary,
		status: d.status,
		owner: d.owner,
		path: d.path,
		sectionPath: d.sectionPath,
		scope: d.scope,
		revision: d.revision,
		version: d.version,
		issuedRevision: d.issuedRevision,
		hasUnissuedChanges: d.hasUnissuedChanges,
		createdAt: d.createdAt,
		createdBy: d.createdBy,
		updatedAt: d.updatedAt,
		updatedBy: d.updatedBy,
		pendingPublication: d.pendingPublication,
		evaluationExcerpt: d.evaluationExcerpt,
		bodyPreview: d.bodyMarkdown.slice(0, 400),
		problem: d.problem
	};
}

function deriveEvaluationExcerpt(kind: string, body: string) {
	if (kind !== 'report') return undefined;
	const secs = extractSections(body);
	const q = secs
		.find((s) => s.heading.toLowerCase() === 'question')
		?.content.split('\n')[0]
		?.trim();
	const v = secs
		.find((s) => s.heading.toLowerCase() === 'verdict')
		?.content.split('\n')[0]
		?.trim();
	return q && v ? { question: q, verdict: v } : undefined;
}

function derivableSourcesFor(store: ProjectDocsStore, excludeDocId: string): DerivedFromSource[] {
	const sources: DerivedFromSource[] = [];
	for (const doc of store.documents) {
		if (doc.id === excludeDocId) continue;
		const latest = doc.versions.at(-1);
		if (latest) {
			sources.push({
				documentId: doc.id,
				title: doc.title,
				key: doc.key,
				version: latest.version
			});
		}
	}
	return sources;
}

async function ownersOf(organizationId: string) {
	const { snapshot } = await orgApi.view(organizationId);
	return snapshot.members.map((m) => ({
		id: m.id,
		name: m.name,
		kind: m.kind as 'person' | 'agent'
	}));
}

export function allMockDocuments(): ProjectDocumentDetail[] {
	const all: ProjectDocumentDetail[] = [];
	for (const pid of Object.keys(docsSeed.projects)) {
		all.push(...getStore(pid).documents);
	}
	for (const [pid, store] of stores.entries()) {
		if (!(pid in docsSeed.projects)) all.push(...store.documents);
	}
	return all;
}

export const docsApi: DocsApi = {
	async list(organizationId, projectId): Promise<ProjectDocsOverview> {
		await readDelay();
		const { snapshot } = await orgApi.view(organizationId);
		const proj = snapshot.projects?.find((p) => p.id === projectId);
		const seedProj = (projectsSeed.projects as Record<string, { number: number }>)[projectId];
		const store = getStore(projectId);
		const owners = await ownersOf(organizationId);
		return {
			projectId,
			projectName: proj?.name ?? projectId,
			projectNumber: seedProj?.number ?? 1,
			stateBranch: store.stateBranch,
			kinds: kindsForProject(projectId),
			sections: structuredClone(store.sections).toSorted((a, b) => a.position - b.position),
			documents: on('empty') ? [] : store.documents.map(toRow),
			knownLimitations: on('empty') ? [] : knownLimitationsFor(projectId),
			owners,
			indexFreshness: {
				indexedAt: new Date(Date.now() - 2 * 60_000).toISOString(),
				scope: on('title-only') ? 'title_only' : 'full_body'
			},
			canEdit: !on('viewer'),
			...(on('stale')
				? { stale: { copiedAt: new Date(Date.now() - 25 * 60_000).toISOString() } }
				: {})
		};
	},

	async get(organizationId, projectId, docId): Promise<ProjectDocumentDetail> {
		await readDelay();
		const store = getStore(projectId);
		const doc = store.documents.find((d) => d.id === docId);
		if (!doc) {
			throw fail(
				'not_found',
				"This document isn't in the project. It may have been moved or the link is wrong."
			);
		}
		const kinds = kindsForProject(projectId);
		const km = kinds.find((k) => k.key === doc.kind);
		return {
			...structuredClone(doc),
			derivableSources: derivableSourcesFor(store, doc.id),
			canEdit: !on('viewer'),
			canIssue: !on('viewer') && !!km?.versioned,
			...(on('stale')
				? { stale: { copiedAt: new Date(Date.now() - 25 * 60_000).toISOString() } }
				: {})
		};
	},

	async create(organizationId, projectId, draft) {
		await writeDelay();
		const store = getStore(projectId);
		const owners = await ownersOf(organizationId);
		const id = newId();
		const now = new Date().toISOString();
		const actor = owners.find((o) => o.id === 'you') ??
			owners[0] ?? { id: 'you', name: 'Alex Kim', kind: 'person' as const };
		const owner = draft.ownerId ? (owners.find((o) => o.id === draft.ownerId) ?? null) : null;
		const path = deriveCanonicalPath(
			projectId,
			id,
			draft.kind,
			draft.scope,
			draft.sectionPath,
			draft.fileName
		);

		const created: ProjectDocumentDetail = {
			id,
			projectId,
			key: draft.key?.trim() || null,
			kind: draft.kind,
			title: draft.title.trim(),
			summary: draft.summary.trim(),
			status: draft.status,
			owner,
			path,
			sectionPath: isTreeDocument(draft.kind, draft.scope) ? draft.sectionPath : null,
			scope: draft.scope,
			revision: 1,
			version: null,
			issuedRevision: null,
			hasUnissuedChanges: false,
			createdAt: now,
			createdBy: actor,
			updatedAt: now,
			updatedBy: actor,
			bodyMarkdown: draft.bodyMarkdown,
			evaluationExcerpt: deriveEvaluationExcerpt(draft.kind, draft.bodyMarkdown),
			links: [],
			ledTo: [],
			mentionedIn: [],
			codeLinks: [],
			versions: [],
			history: [
				{
					revision: 1,
					at: now,
					by: actor,
					summary: `Created ${draft.title.trim()}`,
					title: draft.title.trim(),
					bodyMarkdown: draft.bodyMarkdown
				}
			],
			derivableSources: [],
			canEdit: true,
			canIssue: !!kindsForProject(projectId).find((k) => k.key === draft.kind)?.versioned
		};

		// Superseding links both records without changing either record's status (DEC-6).
		if (draft.supersedesId) {
			const oldDoc = store.documents.find((d) => d.id === draft.supersedesId);
			if (oldDoc) {
				created.mentionedIn.push({
					id: oldDoc.id,
					kind: 'document',
					key: oldDoc.key ?? undefined,
					title: oldDoc.title,
					status: oldDoc.status[0].toUpperCase() + oldDoc.status.slice(1),
					href: `/projects/${encodeURIComponent(projectId)}/docs/${encodeURIComponent(oldDoc.id)}`
				});
				oldDoc.ledTo.push({
					id: created.id,
					kind: 'document',
					key: created.key ?? undefined,
					title: created.title,
					status: created.status[0].toUpperCase() + created.status.slice(1),
					href: `/projects/${encodeURIComponent(projectId)}/docs/${encodeURIComponent(created.id)}`
				});
			}
		}

		store.documents.unshift(created);
		return { id };
	},

	async save(organizationId, projectId, docId, change) {
		await writeDelay();
		const store = getStore(projectId);
		const doc = store.documents.find((d) => d.id === docId);
		if (!doc) throw fail('not_found', "This document isn't in the project.");

		if (on('conflict') && !conflictedOnce) {
			conflictedOnce = true;
			doc.revision += 1;
			doc.bodyMarkdown = `${doc.bodyMarkdown.trimEnd()}\n\n## Concurrent update\nAnother teammate saved a clarification while you were editing.\n`;
			doc.history.push({
				revision: doc.revision,
				at: new Date().toISOString(),
				by: { id: 'sam', name: 'Sam Okafor', kind: 'agent' },
				summary: 'Concurrent clarification',
				title: doc.title,
				bodyMarkdown: doc.bodyMarkdown
			});
			throw fail('conflict', `${doc.title} changed while you were editing.`, {
				latest: structuredClone(doc)
			});
		}

		if (change.expectedRevision !== doc.revision) {
			throw fail('conflict', `${doc.title} changed while you were editing.`, {
				latest: structuredClone(doc)
			});
		}

		const owners = await ownersOf(organizationId);
		const actor = owners.find((o) => o.id === 'you') ??
			owners[0] ?? { id: 'you', name: 'Alex Kim', kind: 'person' as const };
		const now = new Date().toISOString();
		const nextOwner = change.draft.ownerId
			? (owners.find((o) => o.id === change.draft.ownerId) ?? null)
			: null;

		doc.title = change.draft.title.trim();
		doc.key = change.draft.key?.trim() || null;
		doc.summary = change.draft.summary.trim();
		doc.status = change.draft.status;
		doc.owner = nextOwner;
		doc.bodyMarkdown = change.draft.bodyMarkdown;
		doc.evaluationExcerpt = deriveEvaluationExcerpt(doc.kind, doc.bodyMarkdown);
		if (isTreeDocument(doc.kind, doc.scope)) {
			doc.sectionPath = change.draft.sectionPath;
			doc.path = deriveCanonicalPath(
				projectId,
				doc.id,
				doc.kind,
				doc.scope,
				change.draft.sectionPath,
				change.draft.fileName
			);
		}
		doc.revision += 1;
		doc.updatedAt = now;
		doc.updatedBy = actor;
		doc.problem = undefined;

		const latestIssued = doc.versions.at(-1);
		if (latestIssued) {
			doc.hasUnissuedChanges =
				doc.title !== latestIssued.title || doc.bodyMarkdown !== latestIssued.bodyMarkdown;
		}

		doc.history.push({
			revision: doc.revision,
			at: now,
			by: actor,
			summary: change.summary?.trim() || `Updated ${doc.title}`,
			title: doc.title,
			bodyMarkdown: doc.bodyMarkdown
		});

		return structuredClone(doc);
	},

	async issueVersion(organizationId, projectId, docId, change) {
		await writeDelay();
		const store = getStore(projectId);
		const doc = store.documents.find((d) => d.id === docId);
		if (!doc) throw fail('not_found', "This document isn't in the project.");
		if (change.expectedRevision !== doc.revision) {
			throw fail('conflict', `${doc.title} changed before issuing.`);
		}
		const owners = await ownersOf(organizationId);
		const actor = owners.find((o) => o.id === 'you') ??
			owners[0] ?? { id: 'you', name: 'Alex Kim', kind: 'person' as const };
		const now = new Date().toISOString();
		const nextVersion = (doc.version ?? 0) + 1;
		const issuedFromRev = doc.revision;
		const resolvedDerived: DerivedFromSource[] = change.derivedFrom.map((d) => {
			const src = store.documents.find((x) => x.id === d.documentId);
			return {
				documentId: d.documentId,
				title: src?.title ?? d.documentId,
				key: src?.key ?? null,
				version: d.version
			};
		});

		doc.versions.push({
			version: nextVersion,
			revision: issuedFromRev,
			title: doc.title,
			bodyMarkdown: doc.bodyMarkdown,
			note: change.note,
			issuedAt: now,
			issuedBy: actor,
			derivedFrom: resolvedDerived
		});
		doc.version = nextVersion;
		doc.issuedRevision = issuedFromRev;
		doc.revision += 1;
		doc.status = 'current';
		doc.hasUnissuedChanges = false;
		doc.updatedAt = now;
		doc.updatedBy = actor;
		doc.history.push({
			revision: doc.revision,
			at: now,
			by: actor,
			summary: `Issued version v${nextVersion}${change.note ? `: ${change.note}` : ''}`,
			title: doc.title,
			bodyMarkdown: doc.bodyMarkdown
		});

		return structuredClone(doc);
	},

	async addSection(_organizationId, projectId, section) {
		await writeDelay();
		const store = getStore(projectId);
		if (store.sections.some((s) => s.path === section.path)) {
			throw fail('already_exists', `Section docs/${section.path} already exists.`);
		}
		store.sections.push({
			path: section.path,
			title: section.title,
			summary: section.summary,
			position: store.sections.length,
			kinds: [],
			media: []
		});
	},

	async renameSection(_organizationId, projectId, sectionPath, change) {
		await writeDelay();
		const store = getStore(projectId);
		const sec = store.sections.find((s) => s.path === sectionPath);
		if (!sec) throw fail('not_found', "That section isn't in the index any more.");
		sec.title = change.title;
		sec.summary = change.summary;
	},

	async reorderSections(_organizationId, projectId, orderedPaths) {
		await writeDelay();
		const store = getStore(projectId);
		const byPath = new Map(store.sections.map((s) => [s.path, s]));
		store.sections = orderedPaths
			.map((p, position) => {
				const s = byPath.get(p);
				return s ? { ...s, position } : null;
			})
			.filter((s): s is DocSection => s !== null);
	},

	async moveDocument(_organizationId, projectId, docId, change) {
		await writeDelay();
		const store = getStore(projectId);
		const doc = store.documents.find((d) => d.id === docId);
		if (!doc) throw fail('not_found', "This document isn't in the project.");
		if (!store.sections.some((s) => s.path === change.sectionPath)) {
			throw fail('not_found', "Destination section isn't in the index.");
		}
		const name = change.fileName ?? doc.path.slice(doc.path.lastIndexOf('/') + 1);
		doc.sectionPath = change.sectionPath;
		doc.path = `projects/${projectId}/docs/${change.sectionPath}/${name}`;
		doc.revision += 1;
	}
};
