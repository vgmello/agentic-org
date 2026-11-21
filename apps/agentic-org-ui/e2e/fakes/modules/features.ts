// Dev-mock fake for src/lib/modules/features/api.ts under `bun run dev:mock`.
// Scenarios (`?mock=`): slow · fails · empty · unavailable · unsupported · viewer · stale · conflict · save-fails · pending-number
import { orgApi } from '$lib/modules/organization/api';
import { newId } from '$lib/modules/organization/commands';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { FeaturesApi } from '../../../src/lib/modules/features/api';
import type {
	EligibleTaskChoice,
	FeatureDetail,
	FeatureProgress,
	FeatureRow,
	ProjectFeaturesOverview
} from '../../../src/lib/modules/features/types';
import { mockMode } from '../mock-mode';
import featuresSeed from '../data/features/features.json' with { type: 'json' };
import projectsSeed from '../data/projects/projects.json' with { type: 'json' };

interface ProjectFeaturesStore {
	features: FeatureDetail[];
	eligibleTasks: EligibleTaskChoice[];
	nextTaskNum: number;
}

const stores = new Map<string, ProjectFeaturesStore>();
let conflictedOnce = false;

const on = (flag: string) => mockMode().split(',').includes(flag);
const fail = (code: string, message: string, extra: Record<string, unknown> = {}) =>
	Object.assign(new Error(message), { source: 'api', code, message, ...extra });

async function readDelay() {
	await new Promise((r) => setTimeout(r, on('slow') ? 2200 : 120));
	if (on('unavailable')) await unavailableCall();
	if (on('unsupported'))
		throw fail('unsupported', "This version of the office doesn't support project features.");
	if (on('fails')) throw fail('internal', "HQ didn't answer while loading features. Try again.");
}

async function writeDelay() {
	await new Promise((r) => setTimeout(r, on('slow') ? 2200 : 180));
	if (on('unavailable')) await unavailableCall();
	if (on('unsupported'))
		throw fail('unsupported', "This version of the office doesn't support editing features.");
	if (on('stale'))
		throw fail('storage_error', "HQ can't be reached, so nothing was saved. Try again.");
	if (on('viewer'))
		throw fail('not_authorized', "You don't have access to edit features in this project.");
	if (on('save-fails'))
		throw fail('storage_error', "HQ couldn't save this feature. Your draft is kept; try again.");
}

function computeProgress(tasks: FeatureDetail['tasks']): FeatureProgress {
	let done = 0;
	let open = 0;
	let blocked = 0;
	let cancelled = 0;
	for (const t of tasks) {
		if (t.status === 'done') done += 1;
		else if (t.status === 'blocked') blocked += 1;
		else if (t.status === 'cancelled') cancelled += 1;
		else open += 1;
	}
	return { done, open, blocked, cancelled };
}

function getStore(projectId: string): ProjectFeaturesStore {
	let store = stores.get(projectId);
	if (store) return store;
	const raw = (featuresSeed.projects as Record<string, unknown>)[projectId] as
		{ features: FeatureDetail[]; eligibleTasks?: EligibleTaskChoice[] } | undefined;
	store = raw
		? {
				features: structuredClone(raw.features).map((f) => ({
					...f,
					progress: computeProgress(f.tasks),
					canEdit: true,
					canLinkTasks: true
				})),
				eligibleTasks: structuredClone(raw.eligibleTasks ?? []),
				nextTaskNum: 60
			}
		: { features: [], eligibleTasks: [], nextTaskNum: 60 };
	stores.set(projectId, store);
	return store;
}

function toRow(f: FeatureDetail): FeatureRow {
	return {
		id: f.id,
		projectId: f.projectId,
		projectNumber: f.projectNumber,
		projectSlug: f.projectSlug,
		number: f.number,
		localSlug: f.localSlug,
		title: f.title,
		status: f.status,
		owner: f.owner,
		workType: f.workType,
		progress: computeProgress(f.tasks),
		origins: f.origins,
		deliveredAt: f.deliveredAt,
		retiredReason: f.retiredReason,
		replacedBy: f.replacedBy,
		pendingPublication: f.pendingPublication,
		updatedAt: f.updatedAt,
		updatedBy: f.updatedBy,
		problem: f.problem
	};
}

export function allMockFeatures(): FeatureDetail[] {
	const all: FeatureDetail[] = [];
	for (const pid of Object.keys(featuresSeed.projects)) {
		all.push(...getStore(pid).features);
	}
	for (const [pid, store] of stores.entries()) {
		if (!(pid in featuresSeed.projects)) all.push(...store.features);
	}
	return all;
}

export const featuresApi: FeaturesApi = {
	async list(organizationId, projectId): Promise<ProjectFeaturesOverview> {
		await readDelay();
		const { snapshot } = await orgApi.view(organizationId);
		const proj = snapshot.projects?.find((p) => p.id === projectId);
		const seedProj = (projectsSeed.projects as Record<string, { number: number; slug: string }>)[
			projectId
		];
		const firstLink = (snapshot.project_teams ?? []).find((l) => l.project_id === projectId);
		const ownerTeam = snapshot.teams.find((t) => t.id === firstLink?.team_id) ??
			snapshot.teams[0] ?? { id: 'eng', name: 'Engineering' };
		const store = getStore(projectId);
		return {
			projectId,
			projectName: proj?.name ?? projectId,
			projectNumber: seedProj?.number ?? 1,
			projectSlug: seedProj?.slug ?? projectId,
			ownerTeam: { id: ownerTeam.id, name: ownerTeam.name },
			features: on('empty') ? [] : store.features.map(toRow),
			owners: snapshot.members.map((m) => ({
				id: m.id,
				name: m.name,
				kind: m.kind as 'person' | 'agent'
			})),
			teams: snapshot.teams.map((t) => ({ id: t.id, name: t.name })),
			canEdit: !on('viewer'),
			...(on('stale')
				? { stale: { copiedAt: new Date(Date.now() - 25 * 60_000).toISOString() } }
				: {})
		};
	},

	async get(_organizationId, projectId, featureId): Promise<FeatureDetail> {
		await readDelay();
		const store = getStore(projectId);
		const feat = store.features.find((f) => f.id === featureId);
		if (!feat) {
			throw fail('not_found', "This feature isn't in the project.");
		}
		return {
			...structuredClone(feat),
			progress: computeProgress(feat.tasks),
			canEdit: !on('viewer'),
			canLinkTasks: !on('viewer'),
			...(on('stale')
				? { stale: { copiedAt: new Date(Date.now() - 25 * 60_000).toISOString() } }
				: {})
		};
	},

	async create(organizationId, projectId, draft) {
		await writeDelay();
		const { snapshot } = await orgApi.view(organizationId);
		const seedProj = (projectsSeed.projects as Record<string, { number: number; slug: string }>)[
			projectId
		];
		const store = getStore(projectId);
		if (store.features.some((f) => f.localSlug === draft.localSlug)) {
			throw fail('already_exists', `Slug ${draft.localSlug} is already used in this project.`);
		}
		const id = newId();
		const now = new Date().toISOString();
		const members = snapshot.members.map((m) => ({
			id: m.id,
			name: m.name,
			kind: m.kind as 'person' | 'agent'
		}));
		const actor = members.find((m) => m.id === 'you') ??
			members[0] ?? { id: 'you', name: 'Alex Kim', kind: 'person' as const };
		const owner = draft.ownerId ? (members.find((m) => m.id === draft.ownerId) ?? null) : null;
		const maxNum = store.features.reduce((m, f) => Math.max(m, f.number ?? 0), 0);
		const number = on('pending-number') ? null : maxNum + 1;

		const created: FeatureDetail = {
			id,
			projectId,
			projectNumber: seedProj?.number ?? 1,
			projectSlug: seedProj?.slug ?? projectId,
			number,
			localSlug: draft.localSlug,
			title: draft.title,
			status: 'planned',
			owner,
			workType: 'build',
			progress: { done: 0, open: 0, blocked: 0, cancelled: 0 },
			origins: [],
			deliveredAt: null,
			retiredReason: null,
			replacedBy: null,
			pendingPublication: on('pending-number') ? true : undefined,
			revision: 1,
			path: `projects/${projectId}/features/${id}/feature.md`,
			bodyMarkdown: draft.bodyMarkdown,
			createdAt: now,
			createdBy: actor,
			updatedAt: now,
			updatedBy: actor,
			tasks: [],
			affectedBugs: [],
			supportingDocs: [],
			releases: [],
			cost: {
				allowed: true,
				knownCostMicroUsd: null,
				knownTokens: null,
				complete: true,
				leadTimeLabel: 'Just created'
			},
			history: [
				{
					revision: 1,
					at: now,
					by: actor,
					summary: `Created Planned feature ${number === null ? '(Pending number)' : `F-${number}`}`
				}
			],
			canEdit: true,
			canLinkTasks: true
		};
		store.features.unshift(created);
		return { id };
	},

	async save(organizationId, projectId, featureId, change) {
		await writeDelay();
		const store = getStore(projectId);
		const feat = store.features.find((f) => f.id === featureId);
		if (!feat) throw fail('not_found', "This feature isn't in the project.");
		if (on('conflict') && !conflictedOnce) {
			conflictedOnce = true;
			feat.revision += 1;
			feat.bodyMarkdown = `${feat.bodyMarkdown.trimEnd()}\n\n## Note\nConcurrent direct-Git edit.\n`;
			throw fail('conflict', 'Feature changed while you were editing.', {
				latest: structuredClone(feat)
			});
		}
		if (change.expectedRevision !== feat.revision) {
			throw fail('conflict', 'Feature changed while you were editing.', {
				latest: structuredClone(feat)
			});
		}
		const { snapshot } = await orgApi.view(organizationId);
		const members = snapshot.members.map((m) => ({
			id: m.id,
			name: m.name,
			kind: m.kind as 'person' | 'agent'
		}));
		const actor = members.find((m) => m.id === 'you') ??
			members[0] ?? { id: 'you', name: 'Alex Kim', kind: 'person' as const };
		const now = new Date().toISOString();

		feat.title = change.title;
		feat.localSlug = change.localSlug;
		feat.owner = change.ownerId ? (members.find((m) => m.id === change.ownerId) ?? null) : null;
		feat.bodyMarkdown = change.bodyMarkdown;
		feat.revision += 1;
		feat.updatedAt = now;
		feat.updatedBy = actor;
		feat.history.push({
			revision: feat.revision,
			at: now,
			by: actor,
			summary: 'Updated capability definition'
		});
		return structuredClone(feat);
	},

	async setStatus(organizationId, projectId, featureId, change) {
		await writeDelay();
		const store = getStore(projectId);
		const feat = store.features.find((f) => f.id === featureId);
		if (!feat) throw fail('not_found', "This feature isn't in the project.");
		const { snapshot } = await orgApi.view(organizationId);
		const members = snapshot.members.map((m) => ({
			id: m.id,
			name: m.name,
			kind: m.kind as 'person' | 'agent'
		}));
		const actor = members.find((m) => m.id === 'you') ??
			members[0] ?? { id: 'you', name: 'Alex Kim', kind: 'person' as const };
		const now = new Date().toISOString();

		if (
			(change.status === 'in_progress' || change.status === 'live') &&
			!change.ownerId &&
			!feat.owner
		) {
			throw fail('invalid_request', 'In progress and Live require an active accountable owner.');
		}
		if (change.ownerId) {
			feat.owner = members.find((m) => m.id === change.ownerId) ?? feat.owner;
		}
		feat.status = change.status;
		if (change.status === 'live' && !feat.deliveredAt) feat.deliveredAt = now;
		feat.revision += 1;
		feat.updatedAt = now;
		feat.updatedBy = actor;
		feat.history.push({
			revision: feat.revision,
			at: now,
			by: actor,
			summary: `Set status to ${change.status}`
		});
		return structuredClone(feat);
	},

	async retire(organizationId, projectId, featureId, change) {
		await writeDelay();
		const store = getStore(projectId);
		const feat = store.features.find((f) => f.id === featureId);
		if (!feat) throw fail('not_found', "This feature isn't in the project.");
		if (!change.reason.trim()) {
			throw fail('invalid_request', 'A retirement reason is required.');
		}
		const replacement = change.replacedByFeatureId
			? store.features.find((f) => f.id === change.replacedByFeatureId)
			: null;
		const { snapshot } = await orgApi.view(organizationId);
		const members = snapshot.members.map((m) => ({
			id: m.id,
			name: m.name,
			kind: m.kind as 'person' | 'agent'
		}));
		const actor = members.find((m) => m.id === 'you') ??
			members[0] ?? { id: 'you', name: 'Alex Kim', kind: 'person' as const };
		const now = new Date().toISOString();

		feat.status = 'retired';
		feat.retiredReason = change.reason.trim();
		feat.replacedBy = replacement
			? {
					id: replacement.id,
					ref: replacement.number === null ? 'Pending number' : `F-${replacement.number}`,
					title: replacement.title
				}
			: null;
		feat.revision += 1;
		feat.updatedAt = now;
		feat.updatedBy = actor;
		feat.history.push({
			revision: feat.revision,
			at: now,
			by: actor,
			summary: `Retired: ${feat.retiredReason}`
		});
		return structuredClone(feat);
	},

	async eligibleTasks(_organizationId, projectId, featureId) {
		await readDelay();
		const store = getStore(projectId);
		const feat = store.features.find((f) => f.id === featureId);
		const currentIds = new Set(feat?.tasks.map((t) => t.id) ?? []);
		return structuredClone(store.eligibleTasks).filter((t) => !currentIds.has(t.id));
	},

	async linkTask(organizationId, projectId, featureId, change) {
		await writeDelay();
		const store = getStore(projectId);
		const feat = store.features.find((f) => f.id === featureId);
		if (!feat) throw fail('not_found', "This feature isn't in the project.");
		const choice = store.eligibleTasks.find((t) => t.id === change.taskId);
		if (!choice) throw fail('not_found', "That task isn't eligible.");
		const { snapshot } = await orgApi.view(organizationId);
		const firstLink = (snapshot.project_teams ?? []).find((l) => l.project_id === projectId);
		const ownerTeam = snapshot.teams.find((t) => t.id === firstLink?.team_id) ?? {
			id: 'web',
			name: 'Web'
		};

		// Remove from previous feature if moved (FTR-12)
		if (choice.currentFeature) {
			const prev = store.features.find((f) => f.id === choice.currentFeature!.id);
			if (prev) {
				prev.tasks = prev.tasks.filter((t) => t.id !== choice.id);
				prev.progress = computeProgress(prev.tasks);
			}
		}

		feat.tasks.push({
			id: choice.id,
			ref: choice.ref,
			type: choice.type,
			title: choice.title,
			status: choice.status as 'todo',
			team: { id: ownerTeam.id, name: ownerTeam.name },
			assignedTeam: null,
			assignee: null
		});
		feat.progress = computeProgress(feat.tasks);
		choice.currentFeature = {
			id: feat.id,
			ref: feat.number === null ? 'Pending number' : `F-${feat.number}`,
			title: feat.title
		};
	},

	async unlinkTask(_organizationId, projectId, featureId, change) {
		await writeDelay();
		const store = getStore(projectId);
		const feat = store.features.find((f) => f.id === featureId);
		if (!feat) throw fail('not_found', "This feature isn't in the project.");
		const removed = feat.tasks.find((t) => t.id === change.taskId);
		feat.tasks = feat.tasks.filter((t) => t.id !== change.taskId);
		feat.progress = computeProgress(feat.tasks);
		if (removed) {
			const existingChoice = store.eligibleTasks.find((c) => c.id === removed.id);
			if (existingChoice) existingChoice.currentFeature = null;
			else {
				store.eligibleTasks.push({
					id: removed.id,
					ref: removed.ref,
					type: removed.type,
					title: removed.title,
					status: removed.status,
					currentFeature: null
				});
			}
		}
	},

	async breakIntoTasks(organizationId, projectId, featureId, drafts) {
		await writeDelay();
		const store = getStore(projectId);
		const feat = store.features.find((f) => f.id === featureId);
		if (!feat) throw fail('not_found', "This feature isn't in the project.");
		const { snapshot } = await orgApi.view(organizationId);
		const firstLink = (snapshot.project_teams ?? []).find((l) => l.project_id === projectId);
		const ownerTeam = snapshot.teams.find((t) => t.id === firstLink?.team_id) ?? {
			id: 'web',
			name: 'Web'
		};

		const created: { id: string; ref: string; title: string }[] = [];
		for (const d of drafts) {
			const id = newId();
			const ref = `T-A${store.nextTaskNum++}`;
			const assignedTeam = d.assignedTeamId
				? (snapshot.teams.find((t) => t.id === d.assignedTeamId) ?? null)
				: null;
			const assigneeRaw = d.assigneeId
				? (snapshot.members.find((m) => m.id === d.assigneeId) ?? null)
				: null;
			feat.tasks.push({
				id,
				ref,
				type: 'task',
				title: d.title,
				status: 'todo',
				team: { id: ownerTeam.id, name: ownerTeam.name },
				assignedTeam: assignedTeam ? { id: assignedTeam.id, name: assignedTeam.name } : null,
				assignee: assigneeRaw
					? {
							id: assigneeRaw.id,
							name: assigneeRaw.name,
							kind: assigneeRaw.kind as 'person' | 'agent'
						}
					: null
			});
			created.push({ id, ref, title: d.title });
		}
		feat.progress = computeProgress(feat.tasks);
		return { created };
	},

	async repair(_organizationId, projectId, featureId) {
		await writeDelay();
		const store = getStore(projectId);
		const feat = store.features.find((f) => f.id === featureId);
		if (!feat) throw fail('not_found', "This feature isn't in the project.");
		feat.problem = undefined;
		feat.revision += 1;
		return structuredClone(feat);
	}
};
