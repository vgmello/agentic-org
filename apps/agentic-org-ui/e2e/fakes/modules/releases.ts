// Dev-mock fake for src/lib/modules/releases/api.ts under `bun run dev:mock`.
// Scenarios (`?mock=`): slow · fails · empty · unavailable · unsupported · viewer · stale
import { orgApi } from '$lib/modules/organization/api';
import { newId } from '$lib/modules/organization/commands';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { ReleasesApi } from '../../../src/lib/modules/releases/api';
import type {
	ProjectReleasesOverview,
	ReleaseDetail,
	ReleaseRow
} from '../../../src/lib/modules/releases/types';
import { mockMode } from '../mock-mode';
import releasesSeed from '../data/releases/releases.json' with { type: 'json' };
import projectsSeed from '../data/projects/projects.json' with { type: 'json' };

const stores = new Map<string, ReleaseDetail[]>();

const on = (flag: string) => mockMode().split(',').includes(flag);
const fail = (code: string, message: string) =>
	Object.assign(new Error(message), { source: 'api', code, message });

async function readDelay() {
	await new Promise((r) => setTimeout(r, on('slow') ? 2200 : 120));
	if (on('unavailable')) await unavailableCall();
	if (on('unsupported'))
		throw fail('unsupported', "This version of the office doesn't support project releases.");
	if (on('fails')) throw fail('internal', "HQ didn't answer while loading releases. Try again.");
}

async function writeDelay() {
	await new Promise((r) => setTimeout(r, on('slow') ? 2200 : 180));
	if (on('unavailable')) await unavailableCall();
	if (on('unsupported'))
		throw fail('unsupported', "This version of the office doesn't support editing releases.");
	if (on('stale'))
		throw fail('storage_error', "HQ can't be reached, so nothing was saved. Try again.");
	if (on('viewer'))
		throw fail('not_authorized', "You don't have access to create releases in this project.");
}

function getStore(projectId: string): ReleaseDetail[] {
	let list = stores.get(projectId);
	if (list) return list;
	const raw = (releasesSeed.projects as Record<string, { releases: ReleaseDetail[] }>)[projectId];
	list = raw
		? structuredClone(raw.releases).map((r) => ({
				...r,
				milestoneCount: r.milestones.length,
				featureCount: r.features.length,
				taskCount: r.tasks.length,
				deploymentCount: r.deployments.length,
				canEdit: true
			}))
		: [];
	stores.set(projectId, list);
	return list;
}

function toRow(r: ReleaseDetail): ReleaseRow {
	return {
		id: r.id,
		projectId: r.projectId,
		projectNumber: r.projectNumber,
		number: r.number,
		versionLabel: r.versionLabel,
		title: r.title,
		status: r.status,
		owner: r.owner,
		milestoneCount: r.milestones.length,
		featureCount: r.features.length,
		taskCount: r.tasks.length,
		deploymentCount: r.deployments.length,
		latestDeploymentSummary: r.latestDeploymentSummary,
		updatedAt: r.updatedAt
	};
}

export const releasesApi: ReleasesApi = {
	async list(organizationId, projectId): Promise<ProjectReleasesOverview> {
		await readDelay();
		const { snapshot } = await orgApi.view(organizationId);
		const proj = snapshot.projects?.find((p) => p.id === projectId);
		const seedProj = (projectsSeed.projects as Record<string, { number: number }>)[projectId];
		const list = getStore(projectId);
		return {
			projectId,
			projectName: proj?.name ?? projectId,
			projectNumber: seedProj?.number ?? 1,
			releases: on('empty') ? [] : list.map(toRow),
			canEdit: !on('viewer'),
			...(on('stale')
				? { stale: { copiedAt: new Date(Date.now() - 25 * 60_000).toISOString() } }
				: {})
		};
	},

	async get(_organizationId, projectId, releaseId): Promise<ReleaseDetail> {
		await readDelay();
		const list = getStore(projectId);
		const rel = list.find((r) => r.id === releaseId);
		if (!rel) throw fail('not_found', "This release isn't in the project.");
		return {
			...structuredClone(rel),
			canEdit: !on('viewer'),
			...(on('stale')
				? { stale: { copiedAt: new Date(Date.now() - 25 * 60_000).toISOString() } }
				: {})
		};
	},

	async create(organizationId, projectId, draft) {
		await writeDelay();
		const { snapshot } = await orgApi.view(organizationId);
		const seedProj = (projectsSeed.projects as Record<string, { number: number }>)[projectId];
		const list = getStore(projectId);
		const id = newId();
		const now = new Date().toISOString();
		const actorRaw = snapshot.members.find((m) => m.id === 'you') ?? snapshot.members[0];
		const actor = actorRaw
			? { id: actorRaw.id, name: actorRaw.name, kind: actorRaw.kind as 'person' | 'agent' }
			: { id: 'you', name: 'Alex Kim', kind: 'person' as const };
		const nextNum = list.reduce((m, r) => Math.max(m, r.number), 0) + 1;

		list.unshift({
			id,
			projectId,
			projectNumber: seedProj?.number ?? 1,
			number: nextNum,
			versionLabel: draft.versionLabel,
			title: draft.title,
			status: 'planned',
			owner: actor,
			milestoneCount: 0,
			featureCount: 0,
			taskCount: 0,
			deploymentCount: 0,
			revision: 1,
			path: `projects/${projectId}/releases/${id}/release.md`,
			bodyMarkdown: draft.bodyMarkdown,
			createdAt: now,
			createdBy: actor,
			updatedAt: now,
			updatedBy: actor,
			milestones: [],
			features: [],
			tasks: [],
			flagSettings: [],
			deployments: [],
			supportingDocs: [],
			canEdit: true
		});
		return { id };
	}
};
