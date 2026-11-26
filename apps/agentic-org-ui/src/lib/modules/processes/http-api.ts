import type { OrgApi } from '$lib/modules/organization/api';
import { newStableId } from '$lib/modules/organization/commands';
import {
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { readJsonObject, readResponse } from '$lib/platform/http/wire';
import { readOrganizationsList } from '$lib/platform/organizations/readers';
import type { AppError, OrgSnapshot } from '$lib/proto';
import type { ProcessesApi } from './api';
import type {
	Attribution,
	DevProcessCatalogue,
	DocumentDetail,
	DocumentDraft,
	DocumentKind,
	EditorOptions,
	EffectiveRow,
	EmployeeRef,
	Layer,
	LayerOutcome,
	Level,
	Lifecycle,
	Place,
	PlaceOption,
	PlaceView,
	ProcessDocument,
	RowProblem
} from './types';

type Rec = Record<string, unknown>;

const asRec = (v: unknown): Rec | undefined =>
	v && typeof v === 'object' && !Array.isArray(v) ? (v as Rec) : undefined;
const asStr = (v: unknown): string | undefined => (typeof v === 'string' ? v : undefined);
const asNum = (v: unknown): number | undefined =>
	typeof v === 'number' && Number.isFinite(v) ? v : undefined;

const VALID_KINDS: ReadonlySet<string> = new Set(['process', 'standard']);
const VALID_STATUS: ReadonlySet<string> = new Set(['draft', 'current', 'deprecated']);
const VALID_LEVEL: ReadonlySet<string> = new Set(['organization', 'team', 'project']);
const VALID_OUTCOMES: ReadonlySet<string> = new Set([
	'applies',
	'replaced',
	'refused',
	'invalid',
	'clash'
]);

function decodeAttribution(
	at: unknown,
	byKind: unknown,
	byId: unknown,
	membersById: ReadonlyMap<string, string>
): Attribution {
	const kindRaw = asStr(byKind);
	const kind = kindRaw === 'employee' || kindRaw === 'system' ? kindRaw : 'unknown';
	const id = asStr(byId);
	return {
		at: asStr(at) ?? new Date(0).toISOString(),
		byKind: kind,
		...(id ? { byId: id, byName: membersById.get(id) ?? id } : {})
	};
}

function pickEnum<T extends string>(raw: unknown, valid: ReadonlySet<string>, fallback: T): T {
	const value = asStr(raw);
	return value && valid.has(value) ? (value as T) : fallback;
}

function decodeDocumentOptionalFields(
	r: Rec,
	membersById: ReadonlyMap<string, string>
): Pick<ProcessDocument, 'key' | 'owner' | 'teamId' | 'projectId' | 'scopeName'> {
	const ownerId = asStr(r.owner_employee_id);
	const ownerName = asStr(r.owner_name) ?? (ownerId ? membersById.get(ownerId) : undefined);
	const key = asStr(r.key);
	const teamId = asStr(r.team_id);
	const projectId = asStr(r.project_id);
	const scopeName = asStr(r.scope_name);
	return {
		...(key ? { key } : {}),
		...(ownerId ? { owner: { id: ownerId, name: ownerName ?? ownerId } } : {}),
		...(teamId ? { teamId } : {}),
		...(projectId ? { projectId } : {}),
		...(scopeName ? { scopeName } : {})
	};
}

function decodeDocument(
	raw: unknown,
	membersById: ReadonlyMap<string, string>
): ProcessDocument | undefined {
	const r = asRec(raw);
	const id = asStr(r?.id);
	if (!r || !id) return undefined;
	return {
		id,
		revision: asNum(r.revision) ?? 1,
		kind: pickEnum<DocumentKind>(r.kind, VALID_KINDS, 'process'),
		title: asStr(r.title) ?? id,
		status: pickEnum<Lifecycle>(r.status, VALID_STATUS, 'draft'),
		level: pickEnum<Level>(r.level, VALID_LEVEL, 'organization'),
		...decodeDocumentOptionalFields(r, membersById),
		overridable: r.overridable !== false,
		bodyMarkdown: asStr(r.body_markdown) ?? '',
		path: asStr(r.path) ?? '',
		created: decodeAttribution(r.created_at, r.created_by_kind, r.created_by_id, membersById),
		updated: decodeAttribution(r.updated_at, r.updated_by_kind, r.updated_by_id, membersById)
	};
}

function decodeLayer(raw: unknown, membersById: ReadonlyMap<string, string>): Layer | undefined {
	const r = asRec(raw);
	const doc = decodeDocument(r?.document, membersById);
	if (!r || !doc) return undefined;
	const rawOutcome = asStr(r.outcome) ?? 'applies';
	const outcome: LayerOutcome = VALID_OUTCOMES.has(rawOutcome)
		? (rawOutcome as LayerOutcome)
		: 'applies';
	return { document: doc, outcome };
}

function isDocumentHere(doc: ProcessDocument, place: Place): boolean {
	if (place.kind === 'organization') return doc.level === 'organization';
	if (place.kind === 'team') return doc.level === 'team' && doc.teamId === place.id;
	if (place.kind === 'project') return doc.level === 'project' && doc.projectId === place.id;
	return false;
}

function decodeProblem(
	rawProblem: unknown,
	chain: readonly Layer[],
	doc: ProcessDocument
): RowProblem | undefined {
	const p = asStr(rawProblem);
	if (!p) return undefined;
	if (p.includes('not_overridable') || p.includes('refused')) {
		const blocking = chain.find((l) => l.outcome === 'applies')?.document ?? doc;
		return { kind: 'refused', by: blocking };
	}
	if (p.includes('clash')) {
		return {
			kind: 'clash',
			scopeName: doc.scopeName ?? doc.level,
			others: chain.filter((l) => l.document.id !== doc.id).map((l) => l.document)
		};
	}
	return { kind: 'invalid', field: p };
}

function decodeEffectiveRow(
	raw: unknown,
	place: Place,
	membersById: ReadonlyMap<string, string>
): EffectiveRow | undefined {
	const r = asRec(raw);
	const doc = decodeDocument(r?.document, membersById);
	if (!r || !doc) return undefined;
	const replaces = decodeDocument(r.replaces, membersById);
	const chain = Array.isArray(r.chain)
		? r.chain.map((l) => decodeLayer(l, membersById)).filter((l): l is Layer => l !== undefined)
		: [{ document: doc, outcome: (r.applies === false ? 'refused' : 'applies') as LayerOutcome }];
	const problem = decodeProblem(r.problem, chain, doc);
	return {
		id: asStr(r.key) ?? asStr(r.id) ?? doc.id,
		document: doc,
		applies: r.applies !== false,
		...(replaces ? { replaces } : {}),
		here: isDocumentHere(doc, place),
		...(problem ? { problem } : {}),
		chain
	};
}

function buildPlaceOptions(snapshot: OrgSnapshot, includeAgents: boolean): PlaceOption[] {
	const options: PlaceOption[] = [{ place: { kind: 'organization' }, name: 'Organization' }];
	for (const team of [...snapshot.teams].sort((a, b) => a.name.localeCompare(b.name))) {
		options.push({ place: { kind: 'team', id: team.id }, name: `${team.name} (team)` });
	}
	for (const proj of [...(snapshot.projects ?? [])]
		.filter((p) => !p.archived)
		.sort((a, b) => a.name.localeCompare(b.name))) {
		options.push({ place: { kind: 'project', id: proj.id }, name: `${proj.name} (project)` });
	}
	if (includeAgents) {
		for (const agent of [...snapshot.members]
			.filter((m) => m.kind === 'agent' && m.agent?.status !== 'offboarded')
			.sort((a, b) => a.name.localeCompare(b.name))) {
			options.push({ place: { kind: 'agent', id: agent.id }, name: `${agent.name} (agent)` });
		}
	}
	return options;
}

function placeName(place: Place, snapshot: OrgSnapshot, fallback?: string): string {
	if (place.kind === 'organization') return 'Organization';
	if (place.kind === 'team') {
		return snapshot.teams.find((t) => t.id === place.id)?.name ?? fallback ?? place.id;
	}
	if (place.kind === 'project') {
		return (snapshot.projects ?? []).find((p) => p.id === place.id)?.name ?? fallback ?? place.id;
	}
	return snapshot.members.find((m) => m.id === place.id)?.name ?? fallback ?? place.id;
}

function scopeFromDraft(draft: DocumentDraft) {
	if (draft.projectId) return { level: 'project', project_id: draft.projectId };
	if (draft.teamId) return { level: 'team', team_id: draft.teamId };
	return { level: 'organization' };
}

/**
 * Production HTTP adapter for Processes & Standards backed by `/api/v1/organizations/{org}/processes/*`.
 */
export function createHttpProcessesApi(deps: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
	session: () => { accountAdmin: boolean } | undefined;
}): ProcessesApi {
	async function canViewerEdit(organizationId: string, snapshot: OrgSnapshot): Promise<boolean> {
		if (deps.session()?.accountAdmin) return true;
		try {
			const reply = await deps.http.read('/api/v1/organizations');
			if (reply.status !== 200) return false;
			const env = readResponse(reply.body, readJsonObject);
			if ('error' in env) return false;
			const org = readOrganizationsList(env.result).organizations.find(
				(o) => o.id === organizationId
			);
			const viewer = org?.employee_id
				? snapshot.members.find((m) => m.id === org.employee_id)
				: undefined;
			return Boolean(viewer?.owner);
		} catch {
			return false;
		}
	}

	function membersMap(snapshot: OrgSnapshot): Map<string, string> {
		return new Map(snapshot.members.map((m) => [m.id, m.name]));
	}

	async function writeDocument(
		organizationId: string,
		draft: DocumentDraft,
		documentId?: string,
		expectedRevision?: number
	): Promise<ProcessDocument> {
		const path = documentId
			? `/api/v1/organizations/${encodeURIComponent(organizationId)}/processes/${encodeURIComponent(documentId)}`
			: `/api/v1/organizations/${encodeURIComponent(organizationId)}/processes`;
		let reply: GatewayReply;
		try {
			reply = await deps.http.submit(path, {
				command_id: newStableId(),
				...(documentId ? { document_id: documentId } : {}),
				kind: draft.kind,
				scope: scopeFromDraft(draft),
				...(draft.key?.trim() ? { key: draft.key.trim() } : {}),
				title: draft.title.trim(),
				status: draft.status,
				...(draft.ownerId ? { owner_employee_id: draft.ownerId } : {}),
				overridable: draft.overridable,
				body_markdown: draft.bodyMarkdown,
				...(expectedRevision !== undefined ? { expected_revision: expectedRevision } : {})
			});
		} catch {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'Process document could not be saved.'
			} satisfies AppError;
		}
		const result = unwrapGatewayResult(reply, 'Process document write was refused.');
		const { snapshot } = await deps.orgApi.view(organizationId);
		const doc = decodeDocument(result.document, membersMap(snapshot));
		if (!doc) {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'Saved process document could not be read.'
			} satisfies AppError;
		}
		return doc;
	}

	const notYetSupported = (): Promise<never> =>
		Promise.reject({
			source: 'api',
			code: 'unavailable',
			message: 'Development process packages are not served by this gateway release yet.'
		} satisfies AppError);

	return Object.freeze<ProcessesApi>({
		async view(organizationId: string, place: Place): Promise<PlaceView> {
			const base = `/api/v1/organizations/${encodeURIComponent(organizationId)}/processes`;
			let url: string;
			if (place.kind === 'organization') {
				url = `${base}?level=organization`;
			} else if (place.kind === 'team') {
				url = `${base}?level=team&team_id=${encodeURIComponent(place.id)}`;
			} else if (place.kind === 'project') {
				url = `${base}?level=project&project_id=${encodeURIComponent(place.id)}`;
			} else {
				url = `${base}/effective?employee_id=${encodeURIComponent(place.id)}`;
			}

			const [{ snapshot }, reply] = await Promise.all([
				deps.orgApi.view(organizationId),
				deps.http.read(url).catch(() => undefined)
			]);
			if (!reply) {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Processes could not be reached.'
				} satisfies AppError;
			}
			const result = unwrapGatewayResult(reply, 'Processes could not be loaded.');
			const members = membersMap(snapshot);
			const rawRows = Array.isArray(result.rows)
				? result.rows
				: Array.isArray(result.entries)
					? result.entries
					: [];
			const decodedRows = rawRows
				.map((r) => decodeEffectiveRow(r, place, members))
				.filter((r): r is EffectiveRow => r !== undefined);
			const inactive = Array.isArray(result.inactive)
				? result.inactive
						.map((d) => decodeDocument(d, members))
						.filter((d): d is ProcessDocument => d !== undefined)
				: [];
			const staleSince = asStr(result.stale_since);
			const canEdit = await canViewerEdit(organizationId, snapshot);

			return {
				place,
				name: placeName(place, snapshot, asStr(result.scope_name)),
				options: buildPlaceOptions(snapshot, true),
				processes: decodedRows.filter((r) => r.document.kind === 'process'),
				standards: decodedRows.filter((r) => r.document.kind === 'standard'),
				inactive,
				writable: place.kind !== 'agent',
				...(place.kind === 'agent' ? { agent: {} } : {}),
				...(staleSince ? { stale: { copiedAt: staleSince } } : {}),
				viewer: { canEdit }
			};
		},

		async document(organizationId: string, documentId: string): Promise<DocumentDetail> {
			const [{ snapshot }, reply] = await Promise.all([
				deps.orgApi.view(organizationId),
				deps.http
					.read(
						`/api/v1/organizations/${encodeURIComponent(organizationId)}/processes/${encodeURIComponent(documentId)}`
					)
					.catch(() => undefined)
			]);
			if (!reply) {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'Process document could not be reached.'
				} satisfies AppError;
			}
			const result = unwrapGatewayResult(reply, 'Process document could not be loaded.');
			const members = membersMap(snapshot);
			const document = decodeDocument(result.document, members);
			if (!document) {
				throw {
					source: 'api',
					code: 'not_found',
					message: 'Process document not found.'
				} satisfies AppError;
			}
			const parent = decodeDocument(result.parent, members);
			return { document, ...(parent ? { parent } : {}) };
		},

		async editorOptions(organizationId: string): Promise<EditorOptions> {
			const { snapshot } = await deps.orgApi.view(organizationId);
			const owners: EmployeeRef[] = snapshot.members
				.filter((m) => m.agent?.status !== 'offboarded')
				.map((m) => ({ id: m.id, name: m.name }))
				.sort((a, b) => a.name.localeCompare(b.name));
			const canEdit = await canViewerEdit(organizationId, snapshot);
			return {
				owners,
				places: buildPlaceOptions(snapshot, false),
				viewer: { canEdit }
			};
		},

		create: (organizationId, draft) => writeDocument(organizationId, draft),

		update: (organizationId, documentId, draft, expectedRevision) =>
			writeDocument(organizationId, draft, documentId, expectedRevision),

		async devProcesses(organizationId: string): Promise<DevProcessCatalogue> {
			const { snapshot } = await deps.orgApi.view(organizationId);
			const canEdit = await canViewerEdit(organizationId, snapshot);
			return {
				processes: [],
				viewer: { canEdit }
			};
		},

		async devProcess(): Promise<never> {
			throw {
				source: 'api',
				code: 'not_found',
				message: 'Development process not found.'
			} satisfies AppError;
		},

		createDevProcess: notYetSupported,
		writeDevPart: notYetSupported,
		copyDevProcess: notYetSupported,
		importDevProcess: notYetSupported,
		exportDevProcess: notYetSupported
	});
}
