// Mock for src/lib/modules/processes/api.ts under `bun run dev:mock`. Documents and development processes are JSON in
// e2e/fakes/data/processes; teams, projects and members come from the org mock, so IDs and names match the other
// pages. Resolution follows the processes design (decision 4): the organization's current documents, then the
// accountable team's chain from the top team down, then the project's; a lower document with the same key replaces
// one above unless that one is not overridable. Changes are kept in memory until the page reloads.
// Scenarios (`?mock=` on the page's URL, several joined with commas):
//   slow · fails (reads fail) · empty (no documents) · unavailable · stale (organization repository unreachable: the
//   last copy) · viewer (no edit rights) · conflict (someone saves first, once) · save-fails (writes fail)
//   refuse-status, refuse-scope (writes refused on the lifecycle, or on a field the editor has no place for)
import { orgApi } from '$lib/org/api';
import type { OrgSnapshot } from '$lib/proto';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { ProcessesApi } from '../../../src/lib/modules/processes/api';
import type {
	Attribution,
	DevPart,
	DevProcessDetail,
	DevProcessSummary,
	DocumentDraft,
	EffectiveRow,
	EmployeeRef,
	Layer,
	Lifecycle,
	Place,
	PlaceOption,
	PlaceView,
	ProcessDocument
} from '../../../src/lib/modules/processes/types';
import { mockMode } from '../mock-mode';
import documentsData from '../data/processes/documents.json' with { type: 'json' };
import devData from '../data/processes/development-processes.json' with { type: 'json' };
// Current work comes from the members mock, as the member profile's "Working on" does.
import membersData from '../data/members/members.json' with { type: 'json' };

type StoredDocument = {
	id: string;
	key?: string | null;
	revision: number;
	kind: 'process' | 'standard';
	title: string;
	status: Lifecycle;
	owner_employee_id?: string | null;
	team_id?: string | null;
	project_id?: string | null;
	overridable: boolean;
	body_markdown: string;
	created: Attribution;
	updated: Attribution;
	invalid_field?: string;
};

type StoredDev = Omit<
	DevProcessDetail,
	| 'owner'
	| 'seedRoles'
	| 'projects'
	| 'manifestYaml'
	| 'copiedFrom'
	| 'viewer'
	| 'stale'
	| 'roles'
	| 'skills'
> & {
	owner_employee_id?: string;
	copied_from_process_id?: string;
	roles: DevProcessDetail['roles'];
	skills: DevProcessDetail['skills'];
	/** Set when the manifest was written as text: shown as written. */
	manifestText?: string;
};

let documents = structuredClone(documentsData.documents) as StoredDocument[];
let devProcesses = structuredClone(devData.processes) as unknown as StoredDev[];
const projectProcess = new Map<string, string>(Object.entries(devData.projects));
let conflicted = false;
let counter = 0;

const on = (name: string) => mockMode().split(',').includes(name);
const fail = (code: string, message: string, extra: Record<string, unknown> = {}) =>
	Object.assign(new Error(message), { source: 'api', code, message, ...extra });

async function read() {
	await new Promise((r) => setTimeout(r, on('slow') ? 2500 : 200));
	if (on('unavailable')) await unavailableCall();
	if (on('fails'))
		throw fail('internal', "HQ didn't answer. Check that the office is running, then try again.");
}

async function write() {
	await new Promise((r) => setTimeout(r, on('slow') ? 2500 : 400));
	if (on('unavailable')) await unavailableCall();
	if (on('stale'))
		throw fail(
			'storage_error',
			"Can't reach the organization repository, so nothing was saved. Try again."
		);
	if (on('save-fails'))
		throw fail('storage_error', "HQ couldn't write the file. Your text is kept; try again.");
	if (on('viewer')) throw fail('not_authorized', "You don't have access to change processes.");
	// The office refuses a field the browser can't check, or one with no field of its own in the editor.
	if (on('refuse-status'))
		throw fail(
			'invalid_request',
			'The owner has left the organization. Save it as a draft, or choose another owner.',
			{
				field: 'status'
			}
		);
	if (on('refuse-scope'))
		throw fail('invalid_request', 'Web is archived, so nothing can be written there.', {
			field: 'teamId'
		});
}

const stale = () =>
	on('stale') ? { stale: { copiedAt: new Date(Date.now() - 42 * 60_000).toISOString() } } : {};
const viewer = () => ({ canEdit: !on('viewer') });
const now = (): Attribution => ({
	at: new Date().toISOString(),
	byKind: 'employee',
	byId: 'you',
	byName: 'Alex Kim'
});
const newUuid = () =>
	`0193a000-0000-7000-8000-${String(900_000_000_000 + ++counter).padStart(12, '0')}`;

// ── The org around the documents ───────────────────────────────────────────────────────────────────────────────

async function snapshot(organizationId: string): Promise<OrgSnapshot> {
	return (await orgApi.view(organizationId)).snapshot;
}

/** The team's chain from the top team down to it. */
function teamChain(s: OrgSnapshot, teamId: string): string[] {
	const chain: string[] = [];
	let id: string | undefined = teamId;
	const seen = new Set<string>();
	while (id && !seen.has(id)) {
		seen.add(id);
		chain.unshift(id);
		id = s.teams.find((t) => t.id === id)?.owner_team_id;
	}
	return chain;
}

/** The project's owning (accountable) team: its first team. */
const owningTeam = (s: OrgSnapshot, projectId: string) =>
	(s.project_teams ?? []).find((pt) => pt.project_id === projectId)?.team_id;

const teamName = (s: OrgSnapshot, id: string) => s.teams.find((t) => t.id === id)?.name ?? id;
const projectName = (s: OrgSnapshot, id: string) =>
	(s.projects ?? []).find((p) => p.id === id)?.name ?? id;
const memberName = (s: OrgSnapshot, id: string) => s.members.find((m) => m.id === id)?.name;

const memberExtras = membersData.members as Record<
	string,
	{ workingOn?: { ref: string; title: string; project?: string } }
>;

/** An agent's current work and its project. */
function workOf(s: OrgSnapshot, agentId: string) {
	const work = memberExtras[agentId]?.workingOn;
	if (!work) return undefined;
	const project = (s.projects ?? []).find((p) => p.name === work.project);
	return { ref: work.ref, title: work.title, projectId: project?.id, projectName: project?.name };
}

function teamPath(s: OrgSnapshot, teamId: string) {
	return teamChain(s, teamId)
		.map((id) => `teams/${id}`)
		.join('/');
}

function toDocument(s: OrgSnapshot, d: StoredDocument): ProcessDocument {
	const level = d.team_id ? 'team' : d.project_id ? 'project' : 'organization';
	const folder = d.kind === 'standard' ? 'standards' : 'processes';
	const base =
		level === 'team'
			? `${teamPath(s, d.team_id!)}/`
			: level === 'project'
				? `projects/${d.project_id}/`
				: '';
	const owner = d.owner_employee_id
		? { id: d.owner_employee_id, name: memberName(s, d.owner_employee_id) ?? d.owner_employee_id }
		: undefined;
	const named = (a: Attribution): Attribution =>
		a.byKind === 'employee' && a.byId ? { ...a, byName: memberName(s, a.byId) ?? a.byName } : a;
	return {
		id: d.id,
		key: d.key ?? undefined,
		revision: d.revision,
		kind: d.kind,
		title: d.title,
		status: d.status,
		owner,
		level,
		teamId: d.team_id ?? undefined,
		projectId: d.project_id ?? undefined,
		scopeName:
			level === 'team'
				? teamName(s, d.team_id!)
				: level === 'project'
					? projectName(s, d.project_id!)
					: undefined,
		overridable: d.overridable,
		bodyMarkdown: d.body_markdown,
		path: `${base}${folder}/${d.id}.md`,
		created: named(d.created),
		updated: named(d.updated),
		...(d.invalid_field ? { invalid: { field: d.invalid_field } } : {})
	};
}

/** The levels a place reads, highest first: each a filter over stored documents and the name of its scope. */
function levelsFor(s: OrgSnapshot, place: Place) {
	type Scope = { match: (d: StoredDocument) => boolean; name: string; here: boolean };
	const org: Scope = {
		match: (d) => !d.team_id && !d.project_id,
		name: 'the organization',
		here: place.kind === 'organization'
	};
	const team = (id: string, here: boolean): Scope => ({
		match: (d) => d.team_id === id,
		name: teamName(s, id),
		here
	});
	const project = (id: string, here: boolean): Scope => ({
		match: (d) => d.project_id === id,
		name: projectName(s, id),
		here
	});
	const forProject = (projectId: string, here: boolean): Scope[] => {
		const owner = owningTeam(s, projectId);
		return [
			org,
			...(owner ? teamChain(s, owner).map((id) => team(id, false)) : []),
			project(projectId, here)
		];
	};
	if (place.kind === 'team')
		return [org, ...teamChain(s, place.id).map((id) => team(id, id === place.id))];
	if (place.kind === 'project') return forProject(place.id, true);
	if (place.kind === 'agent') {
		const work = workOf(s, place.id);
		return work?.projectId ? forProject(work.projectId, false) : [org];
	}
	return [org];
}

/** The effective rows of one kind for a place (processes design decision 4). */
function resolve(s: OrgSnapshot, place: Place, kind: 'process' | 'standard'): EffectiveRow[] {
	const rows: EffectiveRow[] = [];
	const byKey = new Map<string, EffectiveRow>();
	for (const level of levelsFor(s, place)) {
		const here = documents.filter(
			(d) => d.kind === kind && level.match(d) && d.status === 'current'
		);
		const groups = new Map<string, StoredDocument[]>();
		for (const d of here) {
			const k = !d.invalid_field && d.key ? `key:${d.key}` : `id:${d.id}`;
			groups.set(k, [...(groups.get(k) ?? []), d]);
		}
		for (const [k, group] of groups) {
			const docs = group.map((d) => toDocument(s, d));
			if (docs.length > 1) {
				// Two documents with one key at one level and scope: neither applies until one is renamed.
				for (const doc of docs)
					rows.push({
						id: `clash:${doc.id}`,
						document: doc,
						applies: false,
						here: level.here,
						problem: {
							kind: 'clash',
							scopeName: level.name === 'the organization' ? 'the organization' : level.name,
							others: docs.filter((o) => o.id !== doc.id)
						},
						chain: [{ document: doc, outcome: 'clash' }]
					});
				continue;
			}
			const doc = docs[0];
			if (doc.invalid) {
				rows.push({
					id: `invalid:${doc.id}`,
					document: doc,
					applies: false,
					here: level.here,
					problem: { kind: 'invalid', field: doc.invalid.field },
					chain: [{ document: doc, outcome: 'invalid' }]
				});
				continue;
			}
			const above = k.startsWith('key:') ? byKey.get(k) : undefined;
			if (!above) {
				const row: EffectiveRow = {
					id: doc.key ?? doc.id,
					document: doc,
					applies: true,
					here: level.here,
					chain: [{ document: doc, outcome: 'applies' }]
				};
				rows.push(row);
				if (doc.key) byKey.set(k, row);
				continue;
			}
			if (!above.document.overridable) {
				rows.push({
					id: `refused:${doc.id}`,
					document: doc,
					applies: false,
					here: level.here,
					problem: { kind: 'refused', by: above.document },
					chain: [
						{ document: above.document, outcome: 'applies' },
						{ document: doc, outcome: 'refused' }
					]
				});
				continue;
			}
			const chain: Layer[] = [
				...above.chain.map((l) =>
					l.outcome === 'applies' ? { ...l, outcome: 'replaced' as const } : l
				),
				{ document: doc, outcome: 'applies' }
			];
			Object.assign(above, {
				document: doc,
				replaces: above.document,
				here: level.here,
				chain
			});
		}
	}
	return rows;
}

function optionsFor(s: OrgSnapshot, agents = true): PlaceOption[] {
	const teams = [...s.teams].sort((a, b) =>
		teamChain(s, a.id).join('/').localeCompare(teamChain(s, b.id).join('/'))
	);
	return [
		{ place: { kind: 'organization' }, name: 'Organization' },
		...teams.map((t) => ({ place: { kind: 'team' as const, id: t.id }, name: t.name })),
		...(s.projects ?? []).map((p) => ({
			place: { kind: 'project' as const, id: p.id },
			name: p.name
		})),
		...(agents
			? s.members
					.filter((m) => m.kind === 'agent')
					.map((m) => ({ place: { kind: 'agent' as const, id: m.id }, name: m.name }))
			: [])
	];
}

const devName = (id: string) => devProcesses.find((p) => p.id === id)?.name;

function placeExists(s: OrgSnapshot, place: Place) {
	if (place.kind === 'organization') return true;
	if (place.kind === 'team') return s.teams.some((t) => t.id === place.id);
	if (place.kind === 'project') return (s.projects ?? []).some((p) => p.id === place.id);
	return s.members.some((m) => m.id === place.id && m.kind === 'agent');
}

/** The scope a place's documents are written to. */
function scopeOf(d: Pick<DocumentDraft, 'teamId' | 'projectId'>) {
	return { team_id: d.teamId ?? null, project_id: d.projectId ?? null };
}

/** The document a draft at this scope would replace: the nearest one above with the same key that applies. */
function aboveFor(s: OrgSnapshot, draft: DocumentDraft): ProcessDocument | undefined {
	if (!draft.key || (!draft.teamId && !draft.projectId)) return undefined;
	const place: Place = draft.teamId
		? { kind: 'team', id: draft.teamId }
		: { kind: 'project', id: draft.projectId! };
	const levels = levelsFor(s, place).slice(0, -1);
	let found: ProcessDocument | undefined;
	for (const level of levels) {
		const d = documents.find(
			(x) =>
				level.match(x) &&
				x.kind === draft.kind &&
				x.key === draft.key &&
				x.status === 'current' &&
				!x.invalid_field
		);
		if (!d) continue;
		const doc = toDocument(s, d);
		if (found && !found.overridable) continue; // a not-overridable document above still wins
		found = doc;
	}
	return found;
}

function validate(s: OrgSnapshot, draft: DocumentDraft, ignoreId?: string) {
	if (!draft.title.trim()) throw fail('invalid_request', 'Give it a title.', { field: 'title' });
	if (draft.key && !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(draft.key))
		throw fail('invalid_request', 'Use lowercase letters, digits and single hyphens.', {
			field: 'key'
		});
	if (draft.status === 'current' && !draft.ownerId)
		throw fail('invalid_request', 'A current document needs an owner.', { field: 'ownerId' });
	const scope = scopeOf(draft);
	if (
		draft.key &&
		documents.some(
			(d) =>
				d.id !== ignoreId &&
				d.key === draft.key &&
				d.kind === draft.kind &&
				(d.team_id ?? null) === scope.team_id &&
				(d.project_id ?? null) === scope.project_id
		)
	)
		throw fail(
			'already_exists',
			`Another ${draft.kind} here already uses the key ${draft.key}. Choose another key.`,
			{ field: 'key' }
		);
	const above = aboveFor(s, draft);
	if (above && !above.overridable)
		throw fail(
			'invalid_request',
			`${above.title} is marked not overridable by ${above.level === 'organization' ? 'the organization' : `team ${above.scopeName}`}, so it can't be replaced here. Use another key to add a ${draft.kind} instead.`,
			{ field: 'key', rule: 'not_overridable' }
		);
}

// ── Development processes ──────────────────────────────────────────────────────────────────────────────────────

const MANIFEST_FIELDS = new Set([
	'schema_version',
	'id',
	'key',
	'revision',
	'name',
	'summary',
	'version_label',
	'status',
	'owner_employee_id',
	'copied_from_process_id',
	'roles',
	'kinds',
	'folders',
	'stages',
	'tabs'
]);
const ITEM_FIELDS = new Set([
	'key',
	'name',
	'hats',
	'seed',
	'title',
	'versioned',
	'path',
	'summary',
	'kinds',
	'lead_role',
	'view',
	'section',
	'types',
	'layout',
	'lanes'
]);
const SKILL_FIELDS = new Set(['schema_version', 'key', 'title', 'description', 'roles']);

const yamlText = (value: string) => (/^[\w ./()-]*$/.test(value) ? value : JSON.stringify(value));

function manifestOf(p: StoredDev): string {
	if (p.manifestText) return p.manifestText;
	const lines = [
		'schema_version: 1',
		`id: ${p.id}`,
		`key: ${p.key}`,
		`revision: ${p.revision}`,
		`name: ${yamlText(p.name)}`,
		`summary: ${yamlText(p.summary)}`,
		...(p.versionLabel ? [`version_label: "${p.versionLabel}"`] : []),
		`status: ${p.status}`,
		...(p.owner_employee_id ? [`owner_employee_id: ${p.owner_employee_id}`] : []),
		...(p.copied_from_process_id ? [`copied_from_process_id: ${p.copied_from_process_id}`] : []),
		'roles:',
		...p.roles.flatMap((r) => [
			`  - key: ${r.key}`,
			`    name: ${yamlText(r.name)}`,
			...(r.hats.length ? [`    hats: [${r.hats.join(', ')}]`] : []),
			`    seed: ${r.seed}`
		]),
		...(p.kinds.length
			? [
					'kinds:',
					...p.kinds.flatMap((k) => [
						`  - key: ${k.key}`,
						`    title: ${yamlText(k.title)}`,
						`    versioned: ${k.versioned}`
					])
				]
			: []),
		'folders:',
		...p.folders.flatMap((f) => [
			`  - path: ${f.path}`,
			`    title: ${yamlText(f.title)}`,
			...(f.summary ? [`    summary: ${yamlText(f.summary)}`] : [])
		]),
		'stages:',
		...p.stages.flatMap((st) => [
			`  - key: ${st.key}`,
			`    title: ${yamlText(st.title)}`,
			...(st.summary ? [`    summary: ${yamlText(st.summary)}`] : []),
			...(st.leadRole ? [`    lead_role: ${st.leadRole}`] : [])
		]),
		'tabs:',
		...p.tabs.flatMap((t) => [
			`  - key: ${t.key}`,
			`    title: ${yamlText(t.title)}`,
			`    view: ${t.view}`
		])
	];
	return lines.join('\n') + '\n';
}

const unquote = (v: string) => {
	const t = v.trim();
	try {
		return t.startsWith('"') ? (JSON.parse(t) as string) : t;
	} catch {
		return t;
	}
};

/** A deliberately small reading of the manifest: every field must be known (PKG-2); the top-level text fields are applied. */
function applyManifest(p: StoredDev, text: string) {
	for (const line of text.split('\n')) {
		const top = /^([a-z_]+):(.*)$/.exec(line);
		const item = /^\s+-?\s*([a-z_]+):/.exec(line);
		const name = top?.[1] ?? item?.[1];
		if (!name) continue;
		if (top ? !MANIFEST_FIELDS.has(name) : !ITEM_FIELDS.has(name))
			throw fail(
				'invalid_request',
				`process.yaml: ${name} isn't a field a development process may have.`,
				{
					field: name
				}
			);
	}
	const field = (name: string) => {
		const match = new RegExp(`^${name}:(.*)$`, 'm').exec(text);
		return match ? unquote(match[1]) : undefined;
	};
	const status = field('status');
	if (status && !['draft', 'current', 'deprecated'].includes(status))
		throw fail('invalid_request', 'process.yaml: status is draft, current or deprecated.', {
			field: 'status'
		});
	if (status === 'current' && !field('owner_employee_id'))
		throw fail(
			'invalid_request',
			'process.yaml: a current development process needs owner_employee_id.',
			{
				field: 'owner_employee_id'
			}
		);
	if (status === 'current' && p.skills.length === 0)
		throw fail(
			'invalid_request',
			'process.yaml: a current development process needs at least one skill.',
			{
				field: 'status'
			}
		);
	p.name = field('name') || p.name;
	p.summary = field('summary') ?? p.summary;
	p.versionLabel = field('version_label') || undefined;
	p.status = (status as Lifecycle) ?? p.status;
	p.owner_employee_id = field('owner_employee_id') || undefined;
	p.manifestText = text.endsWith('\n') ? text : text + '\n';
}

function skillText(skill: DevProcessDetail['skills'][number]) {
	return `---\nschema_version: 1\nkey: ${skill.key}\ntitle: ${yamlText(skill.title)}\ndescription: ${yamlText(skill.description)}\nroles: [${skill.roles.join(', ')}]\n---\n${skill.bodyMarkdown}`;
}

function applySkill(p: StoredDev, key: string, text: string) {
	const match = /^---\n([\s\S]*?)\n---\n?([\s\S]*)$/.exec(text);
	if (!match)
		throw fail(
			'invalid_request',
			`skills/${key}.md: it starts with front matter between --- lines.`,
			{
				field: 'front matter'
			}
		);
	const fields = new Map<string, string>();
	for (const line of match[1].split('\n')) {
		const f = /^([a-z_]+):(.*)$/.exec(line);
		if (!f) continue;
		if (!SKILL_FIELDS.has(f[1]))
			throw fail('invalid_request', `skills/${key}.md: ${f[1]} isn't a field a skill may have.`, {
				field: f[1]
			});
		fields.set(f[1], unquote(f[2]));
	}
	const roles = (fields.get('roles') ?? '[]')
		.replace(/^\[|\]$/g, '')
		.split(',')
		.map((r) => r.trim())
		.filter(Boolean);
	const unknown = roles.find((r) => !p.roles.some((x) => x.key === r));
	if (unknown)
		throw fail(
			'invalid_request',
			`skills/${key}.md: ${unknown} isn't a role of this development process.`,
			{
				field: 'roles'
			}
		);
	const skill = {
		key,
		title: fields.get('title') || key,
		description: fields.get('description') ?? '',
		roles,
		bodyMarkdown: match[2]
	};
	const at = p.skills.findIndex((x) => x.key === key);
	if (at >= 0) p.skills[at] = skill;
	else p.skills.push(skill);
}

async function summaryOf(organizationId: string, p: StoredDev): Promise<DevProcessSummary> {
	const s = await snapshot(organizationId);
	const owner: EmployeeRef | undefined = p.owner_employee_id
		? { id: p.owner_employee_id, name: memberName(s, p.owner_employee_id) ?? p.owner_employee_id }
		: undefined;
	return {
		id: p.id,
		key: p.key,
		name: p.name,
		summary: p.summary,
		versionLabel: p.versionLabel,
		revision: p.revision,
		status: p.status,
		origin: p.origin,
		owner,
		seedRoles: p.roles.filter((r) => r.seed).map((r) => ({ key: r.key, name: r.name })),
		tabs: p.tabs,
		folders: p.folders,
		projects: [...projectProcess]
			.filter(([, id]) => id === p.id)
			.map(([projectId]) => ({ id: projectId, name: projectName(s, projectId) }))
	};
}

function devFor(processId: string) {
	const p = devProcesses.find((x) => x.id === processId);
	if (!p) throw fail('not_found', "That development process isn't here any more.");
	return p;
}

function checkRevision(current: number, expected: number, latest: () => unknown) {
	if (on('conflict') && !conflicted) {
		conflicted = true;
		throw fail('conflict', 'Someone else saved a newer version while you were editing.', {
			latest: latest()
		});
	}
	if (current !== expected)
		throw fail('conflict', 'Someone else saved a newer version while you were editing.', {
			latest: latest()
		});
}

export const processesApi: ProcessesApi = {
	async view(organizationId, place) {
		await read();
		const s = await snapshot(organizationId);
		if (!placeExists(s, place))
			throw fail('not_found', "That team, project or agent isn't in this organization.");
		const all = optionsFor(s);
		const name =
			all.find(
				(o) =>
					o.place.kind === place.kind &&
					('id' in place ? 'id' in o.place && o.place.id === place.id : true)
			)?.name ?? 'Organization';
		const empty = on('empty');
		const was = documents;
		if (empty) documents = [];
		try {
			const own = levelsFor(s, place).find((l) => l.here);
			const work = place.kind === 'agent' ? workOf(s, place.id) : undefined;
			const projectId = place.kind === 'project' ? place.id : work?.projectId;
			const devId = projectId ? projectProcess.get(projectId) : undefined;
			const view: PlaceView = {
				place,
				name,
				options: all,
				processes: resolve(s, place, 'process'),
				standards: resolve(s, place, 'standard'),
				inactive: own
					? documents
							.filter((d) => own.match(d) && d.status !== 'current')
							.map((d) => toDocument(s, d))
					: [],
				writable: place.kind !== 'agent',
				...(devId ? { developmentProcess: { id: devId, name: devName(devId) ?? devId } } : {}),
				...(place.kind === 'agent'
					? {
							agent: work
								? { work: { ref: work.ref, title: work.title, projectName: work.projectName } }
								: {}
						}
					: {}),
				...stale(),
				viewer: viewer()
			};
			return structuredClone(view);
		} finally {
			documents = was;
		}
	},
	async document(organizationId, documentId) {
		await read();
		const s = await snapshot(organizationId);
		const d = documents.find((x) => x.id === documentId);
		if (!d) throw fail('not_found', "That document isn't here any more.");
		const document = toDocument(s, d);
		const parent = aboveFor(s, {
			kind: d.kind,
			key: d.key ?? undefined,
			teamId: d.team_id ?? undefined,
			projectId: d.project_id ?? undefined,
			title: d.title,
			status: d.status,
			overridable: d.overridable,
			bodyMarkdown: d.body_markdown
		});
		return structuredClone({ document, ...(parent ? { parent } : {}) });
	},
	async editorOptions(organizationId) {
		await read();
		const s = await snapshot(organizationId);
		return {
			owners: s.members
				.map((m) => ({ id: m.id, name: m.name }))
				.sort((a, b) => a.name.localeCompare(b.name)),
			places: optionsFor(s, false),
			viewer: viewer()
		};
	},
	async create(organizationId, draft) {
		await write();
		const s = await snapshot(organizationId);
		validate(s, draft);
		const stored: StoredDocument = {
			id: newUuid(),
			key: draft.key ?? null,
			revision: 1,
			kind: draft.kind,
			title: draft.title.trim(),
			status: draft.status,
			owner_employee_id: draft.ownerId ?? null,
			...scopeOf(draft),
			overridable: draft.overridable,
			body_markdown: draft.bodyMarkdown,
			created: now(),
			updated: now()
		};
		documents = [...documents, stored];
		return toDocument(s, stored);
	},
	async update(organizationId, documentId, draft, expectedRevision) {
		await write();
		const s = await snapshot(organizationId);
		const d = documents.find((x) => x.id === documentId);
		if (!d) throw fail('not_found', "That document isn't here any more.");
		checkRevision(d.revision, expectedRevision, () => toDocument(s, d));
		validate(
			s,
			{ ...draft, teamId: d.team_id ?? undefined, projectId: d.project_id ?? undefined },
			d.id
		);
		Object.assign(d, {
			key: draft.key ?? null,
			title: draft.title.trim(),
			status: draft.status,
			owner_employee_id: draft.ownerId ?? null,
			overridable: draft.overridable,
			body_markdown: draft.bodyMarkdown,
			revision: d.revision + 1,
			updated: now()
		});
		delete d.invalid_field;
		return toDocument(s, d);
	},

	async devProcesses(organizationId) {
		await read();
		const list = on('empty') ? [] : devProcesses;
		const order = (p: StoredDev) => (p.origin === 'built_in' ? 0 : 1);
		const sorted = [...list].sort((a, b) => order(a) - order(b) || a.name.localeCompare(b.name));
		return {
			processes: await Promise.all(sorted.map((p) => summaryOf(organizationId, p))),
			...stale(),
			viewer: viewer()
		};
	},
	async devProcess(organizationId, processId) {
		await read();
		const p = devFor(processId);
		const from = p.copied_from_process_id;
		const s = await snapshot(organizationId);
		const updated =
			p.updated.byKind === 'employee' && p.updated.byId
				? { ...p.updated, byName: memberName(s, p.updated.byId) }
				: p.updated;
		return structuredClone({
			...(await summaryOf(organizationId, p)),
			roles: p.roles,
			skills: p.skills,
			templates: p.templates,
			stages: p.stages,
			kinds: p.kinds,
			manifestYaml: manifestOf(p),
			...(from ? { copiedFrom: { id: from, name: devName(from) ?? from } } : {}),
			updated,
			...stale(),
			viewer: viewer()
		});
	},
	async createDevProcess(organizationId, key, name) {
		await write();
		if (devProcesses.some((p) => p.key === key))
			throw fail('already_exists', `The key ${key} is taken. Choose another.`, { field: 'key' });
		const p: StoredDev = {
			id: newUuid(),
			key,
			name: name.trim(),
			summary: '',
			revision: 1,
			status: 'draft',
			origin: 'organization',
			owner_employee_id: 'you',
			roles: [],
			skills: [],
			templates: [],
			stages: [],
			kinds: [],
			folders: [],
			tabs: [],
			updated: now()
		};
		devProcesses = [...devProcesses, p];
		return { processId: p.id };
	},
	async writeDevPart(organizationId, processId, part: DevPart, content, expectedRevision) {
		await write();
		const p = devFor(processId);
		if (p.origin === 'built_in')
			throw fail(
				'not_authorized',
				'A built-in development process can’t be edited. Copy it to change it.'
			);
		checkRevision(p.revision, expectedRevision, () => null);
		const next = structuredClone(p);
		if (part.kind === 'manifest') applyManifest(next, content);
		else if (part.kind === 'skill') applySkill(next, part.key, content);
		else if (part.kind === 'role') {
			const role = next.roles.find((r) => r.key === part.key);
			if (role) role.bodyMarkdown = content;
			else
				next.roles.push({
					key: part.key,
					name: part.key,
					hats: [],
					seed: false,
					bodyMarkdown: content
				});
			next.manifestText = undefined;
		} else {
			const template = next.templates.find((t) => t.key === part.key);
			if (template) template.bodyMarkdown = content;
			else next.templates.push({ key: part.key, bodyMarkdown: content });
		}
		next.revision = p.revision + 1;
		next.updated = now();
		devProcesses = devProcesses.map((x) => (x.id === p.id ? next : x));
		return { processId, revision: next.revision };
	},
	async copyDevProcess(organizationId, processId, key, name) {
		await write();
		const p = devFor(processId);
		if (devProcesses.some((x) => x.key === key))
			throw fail('already_exists', `The key ${key} is taken. Choose another.`, { field: 'key' });
		const copy: StoredDev = {
			...structuredClone(p),
			id: newUuid(),
			key,
			name: name.trim(),
			revision: 1,
			status: 'draft',
			origin: 'organization',
			owner_employee_id: 'you',
			copied_from_process_id: p.id,
			manifestText: undefined,
			updated: now()
		};
		devProcesses = [...devProcesses, copy];
		return { processId: copy.id };
	},
	async importDevProcess(organizationId, file, key) {
		await write();
		if (file.size > 5 * 1024 * 1024) throw fail('too_large', 'The file is over 5 MiB.');
		let parsed: StoredDev;
		try {
			parsed = JSON.parse(await file.text()) as StoredDev;
			if (!parsed.key || !parsed.name || !Array.isArray(parsed.roles)) throw new Error();
		} catch {
			throw fail('invalid_request', `${file.name} isn't a development process file.`, {
				field: 'file'
			});
		}
		const finalKey = key || parsed.key;
		if (devProcesses.some((x) => x.key === finalKey))
			throw fail('already_exists', `The key ${finalKey} is taken. Choose another key for it.`, {
				field: 'key'
			});
		const imported: StoredDev = {
			...parsed,
			id: newUuid(),
			key: finalKey,
			revision: 1,
			status: 'draft',
			origin: 'organization',
			copied_from_process_id: undefined,
			manifestText: undefined,
			updated: now()
		};
		devProcesses = [...devProcesses, imported];
		return { processId: imported.id };
	},
	async exportDevProcess(organizationId, processId) {
		await read();
		const p = devFor(processId);
		// The mock's process file is JSON; the office's is an archive of the folder (document 18).
		const body = JSON.stringify({ ...p, skillFiles: p.skills.map(skillText) }, null, 2);
		return {
			fileName: `${p.key}.process.zip`,
			blob: new Blob([body], { type: 'application/zip' })
		};
	}
};
