// Mock for src/lib/modules/projects/api.ts under `bun run dev:mock`. Which projects exist, their names and their teams
// come from the org mock (its first project team is the accountable owner, the others contribute, as the Teams mock
// reads them), so the navigation, search and team pages agree with these pages; changes are written back to it with
// its own commands. The rest (number, slug, counts, brief, repositories, activity) is JSON in e2e/fakes/data/projects,
// kept in memory until the page reloads. Development processes and roles come from their own mocks' JSON, so the
// names match Organization settings › Processes and Roles.
// Scenarios (`?mock=` on the page's URL, several joined with commas):
//   slow · fails (reads fail) · empty (no projects) · unavailable · viewer (no create or edit rights)
//   stale (HQ unreachable: the last copy, changes refused) · conflict (the first save meets a newer revision)
//   save-fails (writes fail) · create-unknown (the first New project is applied, but its answer is lost)
import { orgApi } from '$lib/modules/organization/api';
import { newId } from '$lib/modules/organization/commands';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { OrgSnapshot } from '$lib/proto';
import type { ProjectsApi } from '../../../src/lib/modules/projects/api';
import type {
	BugCounts,
	FeatureCounts,
	ProjectDetail,
	ProjectRepository,
	ProjectRow,
	TaskCounts,
	TeamChoice,
	TeamRef
} from '../../../src/lib/modules/projects/types';
import { OUTCOME_UNKNOWN } from '../../../src/lib/modules/projects/outcome';
import { mockMode, POC_ORG_ID } from '../mock-mode';
import { pocProjectRows } from '../poc-dataset';
import { rows as memberRows, teamRef } from './members';
import data from '../data/projects/projects.json' with { type: 'json' };
import devData from '../data/processes/development-processes.json' with { type: 'json' };
import rolesData from '../data/roles/roles.json' with { type: 'json' };

type Seed = (typeof data.projects)[keyof typeof data.projects] & { owner?: string };
type Line = { id: string; agoMin: number; actor: string; kind: string; text: string };
type Thread = { id: string; title: string; kind: string; agoMin: number; preview: string };
type Extra = {
	number: number;
	slug: string;
	revision: number;
	tasks: TaskCounts;
	bugs: BugCounts;
	features: FeatureCounts;
	releases: number;
	brief: ProjectDetail['brief'];
	repositories: { repositoryId: string; integrationBranch: string }[];
	processId: string | null;
	activity: Line[];
	conversations: Thread[];
};

const seeds = data.projects as Record<string, Seed>;
const processOf = devData.projects as Record<string, string>;
const extras = new Map<string, Extra>();
/** The answer each create operation got: a repeated attempt gets the same project, not a second one. */
const created = new Map<string, { projectId: string; number: number }>();
let conflicted = false;
let unknownOnce = false;
/** HQ's revisions only grow; they don't step by one (other writes land between), so callers re-read, never guess. */
const bump = (e: { revision: number }) => (e.revision += 2);

const zeroTasks = (): TaskCounts => ({ todo: 0, inProgress: 0, blocked: 0, done: 0, cancelled: 0 });
const zeroBugs = (): BugCounts => ({ critical: 0, major: 0, minor: 0, trivial: 0, unset: 0 });

const on = (name: string) => mockMode().split(',').includes(name);
const fail = (code: string, message: string, extra: Record<string, unknown> = {}) =>
	Object.assign(new Error(message), { source: 'api', code, message, ...extra });
const ago = (min: number) => new Date(Date.now() - min * 60_000).toISOString();

async function read() {
	if (on('slow')) await new Promise((r) => setTimeout(r, 2500));
	if (on('unavailable')) await unavailableCall();
	if (on('fails'))
		throw fail('internal', "HQ didn't answer. Check that the office is running, then try again.");
}

async function write(name?: string) {
	if (on('slow')) await new Promise((r) => setTimeout(r, 2500));
	if (on('unavailable')) await unavailableCall();
	if (on('stale'))
		throw fail('storage_error', "HQ can't be reached, so nothing was saved. Try again.");
	if (on('viewer')) throw fail('not_authorized', "You don't have access to change this project.");
	if (on('save-fails'))
		throw fail(
			'storage_error',
			"HQ couldn't save the change. What you entered is kept; try again."
		);
	if (on('conflict') && !conflicted && name) {
		conflicted = true;
		throw fail('conflict', `${name} changed while you were editing.`);
	}
}

const stale = () =>
	on('stale') ? { stale: { copiedAt: new Date(Date.now() - 38 * 60_000).toISOString() } } : {};

async function snapshot(organizationId: string): Promise<OrgSnapshot> {
	return (await orgApi.view(organizationId)).snapshot;
}

function extraOf(s: OrgSnapshot, projectId: string): Extra {
	let e = extras.get(projectId);
	if (e) return e;
	const seed = seeds[projectId];
	const used = new Set([
		...Object.values(seeds).map((x) => x.number),
		...[...extras.values()].map((x) => x.number)
	]);
	let next = 1;
	while (used.has(next)) next++;
	const project = s.projects?.find((p) => p.id === projectId);
	e = {
		number: seed?.number ?? next,
		slug: seed?.slug ?? project?.key ?? projectId,
		revision: 1,
		tasks: seed ? { ...seed.tasks } : zeroTasks(),
		bugs: seed ? { ...seed.bugs } : zeroBugs(),
		features: seed ? { ...seed.features } : { planned: 0, inProgress: 0, live: 0 },
		releases: seed?.releases ?? 0,
		brief: seed?.brief ?? null,
		repositories: seed ? structuredClone(seed.repositories) : [],
		processId: processOf[projectId] ?? null,
		activity: seed ? structuredClone(seed.activity) : [],
		conversations: seed ? structuredClone(seed.conversations) : []
	};
	extras.set(projectId, e);
	return e;
}

const roots = (s: OrgSnapshot) => s.teams.filter((t) => !t.owner_team_id);

/**
 * A project always has its accountable team (TEAM-14). The org mock's sample still holds one teamless project from
 * before that rule; it is given the team its seed names (or the first top-level team) once, through the org mock's
 * own command, so every page reads the same owner.
 */
async function repaired(organizationId: string): Promise<OrgSnapshot> {
	let s = await snapshot(organizationId);
	for (const p of s.projects ?? []) {
		if ((s.project_teams ?? []).some((l) => l.project_id === p.id)) continue;
		const owner = seeds[p.id]?.owner ?? roots(s)[0]?.id;
		if (!owner) continue;
		await orgApi.command(organizationId, {
			method: 'project.add_team',
			params: { project_team_id: newId(), project_id: p.id, team_id: owner }
		});
		s = await snapshot(organizationId);
	}
	return s;
}

const links = (s: OrgSnapshot, projectId: string) =>
	(s.project_teams ?? []).filter((l) => l.project_id === projectId);

function ref(s: OrgSnapshot, teamId: string): TeamRef {
	return teamRef(s, teamId) ?? { id: teamId, name: 'Unknown team', ref: '' };
}

const devProcess = (id: string | null) => {
	const p = id ? devData.processes.find((x) => x.id === id) : undefined;
	return p ? { id: p.id, name: p.name } : null;
};

function row(s: OrgSnapshot, projectId: string): ProjectRow | undefined {
	const p = s.projects?.find((x) => x.id === projectId);
	const teams = links(s, projectId);
	if (!p || teams.length === 0) return undefined;
	const e = extraOf(s, p.id);
	return {
		id: p.id,
		number: e.number,
		slug: e.slug,
		name: p.name,
		ownerTeam: ref(s, teams[0].team_id),
		contributors: teams.slice(1).map((l) => ref(s, l.team_id)),
		archived: !!p.archived,
		tasks: { ...e.tasks },
		bugs: { ...e.bugs },
		developmentProcess: devProcess(e.processId)
	};
}

async function projectRow(organizationId: string, projectId: string) {
	const s = await repaired(organizationId);
	const r = row(s, projectId);
	if (!r)
		throw fail(
			'not_found',
			"This project isn't in the organization. It may have been removed, or the link is wrong."
		);
	return { s, r, e: extraOf(s, projectId) };
}

const repositoryOf = (id: string) => data.repositories.find((r) => r.id === id);

function teamPath(s: OrgSnapshot, teamId: string): string {
	const names: string[] = [];
	for (
		let t = s.teams.find((x) => x.id === teamId);
		t;
		t = s.teams.find((x) => x.id === t!.owner_team_id)
	)
		names.unshift(t.name);
	return names.join(' › ');
}

const viewer = () => ({ canEdit: !on('viewer') });

/** Ensures every check passes before the first command is sent: the org mock's changes are not atomic. */
function checkRevision(e: Extra, expected: number, name: string) {
	if (expected !== e.revision) throw fail('conflict', `${name} changed while you were editing.`);
}

async function command(organizationId: string, input: Parameters<typeof orgApi.command>[1]) {
	return orgApi.command(organizationId, input);
}

/** Puts `ownerId` first and `contributorIds` after it, through the org mock's add and remove commands. */
async function setTeams(
	organizationId: string,
	projectId: string,
	ownerId: string,
	contributorIds: string[]
) {
	let s = await snapshot(organizationId);
	if (!links(s, projectId).some((l) => l.team_id === ownerId))
		await command(organizationId, {
			method: 'project.add_team',
			params: { project_team_id: newId(), project_id: projectId, team_id: ownerId }
		});
	s = await snapshot(organizationId);
	const wanted = new Set(contributorIds);
	const first = links(s, projectId)[0];
	// The owner must be the first link: when it isn't, every other link goes and the contributors come back after it.
	const reorder = first?.team_id !== ownerId;
	for (const l of links(s, projectId)) {
		if (l.team_id === ownerId) continue;
		if (!reorder && wanted.has(l.team_id)) continue;
		await command(organizationId, {
			method: 'project.remove_team',
			params: { project_team_id: l.id, expected_version: l.version ?? 1, areas_to_team_id: ownerId }
		});
	}
	s = await snapshot(organizationId);
	for (const id of contributorIds)
		if (id !== ownerId && !links(s, projectId).some((l) => l.team_id === id))
			await command(organizationId, {
				method: 'project.add_team',
				params: { project_team_id: newId(), project_id: projectId, team_id: id }
			});
}

export const projectsApi: ProjectsApi = {
	async list(organizationId) {
		await read();
		if (organizationId === POC_ORG_ID) {
			return {
				projects: on('empty') ? [] : structuredClone(pocProjectRows()),
				viewer: { canCreate: !on('viewer') },
				...stale()
			};
		}
		const s = await repaired(organizationId);
		const projects = on('empty')
			? []
			: (s.projects ?? []).flatMap((p) => {
					const r = row(s, p.id);
					return r ? [r] : [];
				});
		return { projects, viewer: { canCreate: !on('viewer') }, ...stale() };
	},

	async project(organizationId, projectId) {
		await read();
		const { r, e } = await projectRow(organizationId, projectId);
		const process = e.processId ? devData.processes.find((p) => p.id === e.processId) : undefined;
		return {
			...r,
			revision: e.revision,
			brief: e.brief ? { ...e.brief } : null,
			features: { ...e.features },
			releases: e.releases,
			processTabs: (process?.tabs ?? []).map((t) => ({ ...t, view: t.view as 'documents' })),
			repositories: e.repositories.flatMap((a): ProjectRepository[] => {
				const repo = repositoryOf(a.repositoryId);
				return repo
					? [
							{
								repositoryId: repo.id,
								key: repo.key,
								kind: repo.kind as 'remote' | 'local',
								remoteUrl: repo.remoteUrl,
								integrationBranch: a.integrationBranch
							}
						]
					: [];
			}),
			viewer: viewer(),
			...stale()
		};
	},

	async team(organizationId, projectId) {
		await read();
		const { s, r } = await projectRow(organizationId, projectId);
		const entry = (t: TeamRef) => {
			const lead = s.teams.find((x) => x.id === t.id)?.lead_member_id;
			const m = lead ? s.members.find((x) => x.id === lead) : undefined;
			return { ...t, lead: m ? { id: m.id, name: m.name } : undefined };
		};
		const teamIds = new Set([r.ownerTeam.id, ...r.contributors.map((c) => c.id)]);
		const members = (await memberRows(organizationId))
			.filter((m) => m.status !== 'inactive' && m.homeTeam && teamIds.has(m.homeTeam.id))
			.map((m) => ({ id: m.id, name: m.name, kind: m.kind, role: m.role, homeTeam: m.homeTeam! }))
			.toSorted((a, b) => a.name.localeCompare(b.name));
		return { owner: entry(r.ownerTeam), contributors: r.contributors.map(entry), members };
	},

	async activity(organizationId, projectId) {
		await read();
		const { e } = await projectRow(organizationId, projectId);
		const rows = e.activity.map((a) => ({
			id: a.id,
			at: ago(a.agoMin),
			actor: a.actor,
			kind: a.kind,
			text: a.text
		}));
		return on('oldest-first') ? rows.toReversed() : rows;
	},

	async conversations(organizationId, projectId) {
		await read();
		const { e } = await projectRow(organizationId, projectId);
		return e.conversations.map((c) => ({
			id: c.id,
			title: c.title,
			kind: c.kind as 'channel' | 'direct' | 'thread',
			lastAt: ago(c.agoMin),
			preview: c.preview
		}));
	},

	async teamChoices(organizationId) {
		await read();
		const s = await snapshot(organizationId);
		return s.teams
			.map((t): TeamChoice => ({
				...ref(s, t.id),
				path: teamPath(s, t.id),
				// The organization's own top-level team: the first root, made with the organization.
				organizationTeam: t.id === roots(s)[0]?.id
			}))
			.toSorted((a, b) => a.path.localeCompare(b.path));
	},

	async repositoryChoices() {
		await read();
		return data.repositories.map((r) => ({ ...r, kind: r.kind as 'remote' | 'local' }));
	},

	async create(organizationId, draft) {
		const done = created.get(draft.operationId);
		if (done) return done;
		await write();
		let s = await snapshot(organizationId);
		const name = draft.name.trim();
		if ((s.projects ?? []).some((p) => p.name.toLowerCase() === name.toLowerCase()))
			throw fail(
				'already_exists',
				`There's already a project called ${name}. Choose another name.`,
				{ field: 'name' }
			);
		if (
			[...extras.values()].some((e) => e.slug === draft.slug) ||
			Object.values(seeds).some((x) => x.slug === draft.slug)
		)
			throw fail('already_exists', `The slug ${draft.slug} is taken. Choose another.`, {
				field: 'slug'
			});
		let teamId: string;
		if (draft.owner.kind === 'existing') {
			teamId = draft.owner.teamId;
			if (!s.teams.some((t) => t.id === teamId))
				throw fail('not_found', "That team isn't in the organization any more. Choose another.", {
					field: 'owner'
				});
		} else {
			teamId = newId();
			if (s.teams.some((t) => t.name.toLowerCase() === name.toLowerCase()))
				throw fail(
					'already_exists',
					`There's already a team called ${name}. Choose it as an existing team, or name the project differently.`,
					{ field: 'owner' }
				);
			await command(organizationId, {
				method: 'team.create',
				params: { team_id: teamId, name, owner_team_id: draft.owner.parentTeamId }
			});
		}
		const projectId = newId();
		await command(organizationId, {
			method: 'project.create',
			params: {
				project_id: projectId,
				name,
				teams: [{ project_team_id: newId(), team_id: teamId }],
				general_area_id: newId()
			}
		});
		s = await snapshot(organizationId);
		const e = extraOf(s, projectId);
		e.slug = draft.slug;
		e.processId = draft.developmentProcessId;
		e.activity.unshift({
			id: newId(),
			agoMin: 0,
			actor: 'Alex Kim',
			kind: 'Project',
			text: `created ${name}`
		});
		const answer = { projectId, number: e.number };
		created.set(draft.operationId, answer);
		if (on('create-unknown') && !unknownOnce) {
			unknownOnce = true;
			throw Object.assign(new Error('The connection to HQ dropped before it answered.'), {
				source: 'app',
				code: OUTCOME_UNKNOWN,
				message: 'The connection to HQ dropped before it answered.'
			});
		}
		return answer;
	},

	async createOperation(_organizationId, operationId) {
		await read();
		const done = created.get(operationId);
		return done
			? { state: 'applied', ...done }
			: { state: 'not_applied', reason: 'HQ has no record of this project.' };
	},

	async rename(organizationId, projectId, change) {
		const { r, e } = await projectRow(organizationId, projectId);
		await write(r.name);
		checkRevision(e, change.expectedRevision, r.name);
		const s = await snapshot(organizationId);
		const name = change.name.trim();
		if (
			(s.projects ?? []).some(
				(p) => p.id !== projectId && p.name.toLowerCase() === name.toLowerCase()
			)
		)
			throw fail('already_exists', `There's already a project called ${name}.`, { field: 'name' });
		if ([...extras.entries()].some(([id, x]) => id !== projectId && x.slug === change.slug))
			throw fail('already_exists', `The slug ${change.slug} is taken. Choose another.`, {
				field: 'slug'
			});
		const project = s.projects!.find((p) => p.id === projectId)!;
		if (name !== project.name)
			await command(organizationId, {
				method: 'project.update',
				params: { project_id: projectId, expected_version: project.version ?? 0, name }
			});
		e.slug = change.slug;
		bump(e);
	},

	async ownerChangePreview(organizationId, projectId) {
		await read();
		const { e } = await projectRow(organizationId, projectId);
		const t = e.tasks;
		return { tasks: t.todo + t.inProgress + t.blocked + t.done + t.cancelled };
	},

	async setTeams(organizationId, projectId, change) {
		const { r, e } = await projectRow(organizationId, projectId);
		await write(r.name);
		checkRevision(e, change.expectedRevision, r.name);
		const s = await snapshot(organizationId);
		const ids = [change.ownerTeamId, ...change.contributorTeamIds];
		if (change.contributorTeamIds.includes(change.ownerTeamId))
			throw fail('invalid_request', "The accountable team can't also be a contributor.", {
				field: 'owner'
			});
		if (ids.some((id) => !s.teams.some((t) => t.id === id)))
			throw fail('not_found', "One of those teams isn't in the organization any more.", {
				field: 'owner'
			});
		// Checked first, then applied whole: the org mock's own commands are not one transaction.
		await setTeams(organizationId, projectId, change.ownerTeamId, change.contributorTeamIds);
		bump(e);
	},

	async setRepositories(organizationId, projectId, change) {
		const { r, e } = await projectRow(organizationId, projectId);
		await write(r.name);
		checkRevision(e, change.expectedRevision, r.name);
		const ids = change.repositories.map((x) => x.repositoryId);
		if (new Set(ids).size !== ids.length)
			throw fail('invalid_request', 'A repository can be associated once.', {
				field: 'repositories'
			});
		for (const x of change.repositories)
			if (!repositoryOf(x.repositoryId))
				throw fail('not_found', "That repository isn't registered any more.", {
					field: 'repositories'
				});
		e.repositories = structuredClone(change.repositories);
		bump(e);
	},

	async processPreview(_organizationId, _projectId, processId) {
		await read();
		const p = devData.processes.find((x) => x.id === processId);
		if (!p) throw fail('not_found', "That development process isn't in the catalogue any more.");
		const have = new Map(rolesData.roles.map((role) => [role.key, role.revisions.at(-1)!.name]));
		return {
			createsRoles: p.roles
				.filter((role) => !have.has(role.key))
				.map(({ key, name }) => ({ key, name })),
			usesRoles: p.roles
				.filter((role) => have.has(role.key))
				.map(({ key }) => ({ key, name: have.get(key)! })),
			addsSections: p.folders.map((f) => f.title)
		};
	},

	async setProcess(organizationId, projectId, change) {
		const { r, e } = await projectRow(organizationId, projectId);
		await write(r.name);
		checkRevision(e, change.expectedRevision, r.name);
		if (change.processId) {
			const p = devData.processes.find((x) => x.id === change.processId);
			if (!p || p.status !== 'current')
				throw fail('invalid_request', 'Only a current development process can be chosen.', {
					field: 'process'
				});
		}
		e.processId = change.processId;
		bump(e);
	}
};
