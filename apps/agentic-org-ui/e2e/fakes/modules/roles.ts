// Mock for src/lib/modules/roles/api.ts under `bun run dev:mock`. Roles are JSON in e2e/fakes/data/roles; the
// employees that reference them come from the org mock, so IDs and names match the member profiles. Role IDs match
// the Hire mock's role choices. Changes are kept in memory until the page reloads.
// Scenarios (`?mock=` on the page's URL, several joined with commas):
//   slow · fails (reads fail) · empty · unavailable · stale (organization data unreachable: the last copy)
//   viewer (no edit rights) · hr-off (the HR lead can't run) · conflict (someone saves first, once)
//   invalid (one role file's key is not recognized) · save-fails (writes fail)
import { orgApi } from '$lib/org/api';
import { newId } from '$lib/org/commands';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { RolesApi } from '../../../src/lib/modules/roles/api';
import { HR_CANT_RUN } from '../../../src/lib/modules/roles/present';
import type {
	Attribution,
	Role,
	RoleDetail,
	RoleDraft,
	RoleEmployee,
	RoleRevision,
	RoleSummary,
	RolesOverview
} from '../../../src/lib/modules/roles/types';
import { mockMode } from '../mock-mode';
import data from '../data/roles/roles.json' with { type: 'json' };

type Stored = {
	id: string;
	key?: string;
	keyNamedBy?: string[];
	created: Attribution;
	/** Oldest first. */
	revisions: { name: string; bodyMarkdown: string; updated: Attribution }[];
};

let roles: Stored[] = structuredClone(data.roles) as Stored[];
const references = new Map(Object.entries(data.employees));
const fixed = new Set<string>();
let conflicted = false;

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
		throw fail('storage_error', "Can't reach organization data, so nothing was saved. Try again.");
	if (on('save-fails'))
		throw fail('storage_error', "HQ couldn't write the role file. Your text is kept; try again.");
	if (on('viewer')) throw fail('not_authorized', "You don't have access to change roles.");
}

const stale = () =>
	on('stale') ? { stale: { copiedAt: new Date(Date.now() - 42 * 60_000).toISOString() } } : {};
const viewer = () => ({ canEdit: !on('viewer') });

async function members(organizationId: string) {
	return (await orgApi.view(organizationId)).snapshot.members;
}

async function named(organizationId: string, a: Attribution): Promise<Attribution> {
	if (a.byKind !== 'employee' || !a.byId) return a;
	const m = (await members(organizationId)).find((x) => x.id === a.byId);
	return m ? { ...a, byName: m.name } : a;
}

const invalidOf = (r: Stored) =>
	on('invalid') && r.id === 'role-qa' && !fixed.has(r.id) ? { field: 'key' } : undefined;

async function toRole(organizationId: string, r: Stored): Promise<Role> {
	const current = r.revisions.at(-1)!;
	const invalid = invalidOf(r);
	return {
		id: r.id,
		// A key the office could not read is not shown as if it were one.
		...(r.key && !invalid ? { key: r.key } : {}),
		schemaVersion: 1,
		revision: r.revisions.length,
		name: current.name,
		bodyMarkdown: current.bodyMarkdown,
		created: await named(organizationId, r.created),
		updated: await named(organizationId, current.updated),
		...(invalid ? { invalid } : {}),
		...(r.keyNamedBy?.length ? { keyNamedBy: [...r.keyNamedBy] } : {})
	};
}

const countOf = (roleId: string) => [...references.values()].filter((id) => id === roleId).length;

function find(roleId: string) {
	const r = roles.find((x) => x.id === roleId);
	if (!r) throw fail('not_found', "There's no role with this ID in this organization.");
	return r;
}

function keyTaken(key: string | undefined, except?: string) {
	if (key && roles.some((x) => x.key === key && x.id !== except))
		throw fail('already_exists', `Another role already uses the key ${key}. Choose another key.`, {
			field: 'key'
		});
}

async function you(organizationId: string): Promise<Attribution> {
	return named(organizationId, {
		at: new Date().toISOString(),
		byKind: 'employee',
		byId: 'you'
	});
}

export const rolesApi: RolesApi = {
	async list() {
		await read();
		const summaries: RoleSummary[] = on('empty')
			? []
			: roles.map((r) => {
					const invalid = invalidOf(r);
					return {
						id: r.id,
						...(r.key && !invalid ? { key: r.key } : {}),
						name: r.revisions.at(-1)!.name,
						revision: r.revisions.length,
						employeeCount: countOf(r.id),
						...(invalid ? { invalid } : {})
					};
				});
		const overview: RolesOverview = {
			roles: summaries,
			viewer: viewer(),
			hr: { canRun: !on('hr-off') },
			...stale()
		};
		return structuredClone(overview);
	},

	async role(organizationId, roleId) {
		await read();
		const r = find(roleId);
		const all = await members(organizationId);
		const employees: RoleEmployee[] = all
			.filter((m) => references.get(m.id) === roleId && (m.kind === 'agent' || m.kind === 'person'))
			.map((m) => ({
				id: m.id,
				name: m.name,
				kind: m.kind as 'agent' | 'person',
				...(m.agent?.title ? { title: m.agent.title } : {})
			}))
			.toSorted((a, b) => a.name.localeCompare(b.name));
		const history: RoleRevision[] = [];
		for (const [i, rev] of r.revisions.entries())
			history.unshift({
				revision: i + 1,
				name: rev.name,
				bodyMarkdown: rev.bodyMarkdown,
				updated: await named(organizationId, rev.updated)
			});
		const detail: RoleDetail = {
			role: await toRole(organizationId, r),
			employees,
			history,
			viewer: viewer(),
			...stale()
		};
		return structuredClone(detail);
	},

	async create(organizationId, draft: RoleDraft) {
		await write();
		keyTaken(draft.key);
		const by = await you(organizationId);
		const stored: Stored = {
			id: newId(),
			...(draft.key ? { key: draft.key } : {}),
			created: by,
			revisions: [{ name: draft.name, bodyMarkdown: draft.bodyMarkdown, updated: by }]
		};
		roles = [...roles, stored];
		return toRole(organizationId, stored);
	},

	async update(organizationId, roleId, edit) {
		await write();
		const r = find(roleId);
		if (on('conflict') && !conflicted) {
			// Someone else saves first: the person's save is refused with the newer version.
			conflicted = true;
			const current = r.revisions.at(-1)!;
			r.revisions.push({
				name: current.name,
				bodyMarkdown: current.bodyMarkdown.replace(
					/\s*$/,
					'\n\nUpdated by Avery while you were editing.\n'
				),
				updated: { at: new Date().toISOString(), byKind: 'employee', byId: 'avery' }
			});
		}
		if (edit.expectedRevision !== r.revisions.length)
			throw fail(
				'conflict',
				"This role changed while you were editing. Here's the newer version; your changes are kept.",
				{ latest: await toRole(organizationId, r) }
			);
		if (r.keyNamedBy?.length && (edit.key ?? '') !== (r.key ?? ''))
			throw fail(
				'invalid_request',
				`${r.keyNamedBy.join(', ')} uses this key, so it can't be changed or removed.`,
				{ field: 'key' }
			);
		keyTaken(edit.key, r.id);
		if (edit.key) r.key = edit.key;
		else delete r.key;
		r.revisions.push({
			name: edit.name,
			bodyMarkdown: edit.bodyMarkdown,
			updated: await you(organizationId)
		});
		fixed.add(r.id);
		return toRole(organizationId, r);
	},

	async askHrToDraft(_organizationId, description) {
		await new Promise((r) => setTimeout(r, on('slow') ? 4000 : 900));
		if (on('unavailable')) await unavailableCall();
		if (on('hr-off')) throw fail('not_ready', HR_CANT_RUN);
		if (on('save-fails'))
			throw fail('internal', 'The HR lead stopped before finishing the draft. Try again.');
		const words = description.trim().split(/\s+/).slice(0, 3);
		const name = words.map((w) => w.charAt(0).toUpperCase() + w.slice(1).toLowerCase()).join(' ');
		return {
			name: name || 'New role',
			bodyMarkdown: `## Mission\n${description.trim()}\n\n## Responsibilities\n- Take on the work described in the mission.\n- Say what is done and what is not.\n\n## Working style\nShort updates, with the evidence for each.\n\n## Boundaries\n- Follow approved processes and enforced permission policy.\n`
		};
	}
};
