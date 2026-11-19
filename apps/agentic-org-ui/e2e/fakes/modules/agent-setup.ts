// Mock for src/lib/modules/agent-setup/api.ts under `bun run dev:mock`. Agents, names, handles, home teams, reporting,
// placement and Paused come from the org mock through the members fake's shared readers, so IDs and every change
// made through the organization's own commands (Pause, Resume, Offboard, Move home team) match the other pages.
// Execution configuration, connections and readiness are JSON in e2e/fakes/data/agent-setup; roles are the Roles
// mock's JSON. Saves change the answers in memory until the page reloads.
// Scenarios (`?mock=` on the page's URL, several joined with commas):
//   slow · fails (reads fail) · empty (no agents) · unavailable · not-found (every agent read answers not found)
//   viewer (read only) · conflict (someone else saves first, once per save) · save-fails (writes fail)
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { Member } from '$lib/proto';
import type { AgentSetupApi } from '../../../src/lib/modules/agent-setup/api';
import type {
	AgentSetupDetail,
	AgentSetupRow,
	Blocker,
	ConnectionState,
	ProfileChoice,
	ProfileModel,
	Readiness
} from '../../../src/lib/modules/agent-setup/types';
import { mockMode } from '../mock-mode';
import data from '../data/agent-setup/agents.json' with { type: 'json' };
import rolesData from '../data/roles/roles.json' with { type: 'json' };
import { handleOf, homeTeamId, officesFor, snapshotFor } from './members';

type Profile = {
	id: string;
	name: string;
	connectionIds: string[];
	models: ProfileModel[] | null;
	blockers?: Blocker[];
};
type Stored = {
	profileId: string | null;
	modelId: string | null;
	revision: number;
	employeeRevision: number;
	generation: number | null;
	observedAgoS?: number;
	observedRuntime?: string;
	readiness: 'known' | 'unknown';
	unknownWhy?: string;
	enabled?: boolean;
	updated?: { byName: string; agoH?: number; at?: string };
	roleId?: string | null;
};

const connections = data.connections as Record<
	string,
	{ name: string; runtime: string; state: ConnectionState }
>;
const profiles = data.profiles as Profile[];
const stored = new Map<string, Stored>(
	Object.entries(structuredClone(data.byMember) as Record<string, Stored>)
);
const roleNames = new Map(
	rolesData.roles.map((r) => [r.id, r.revisions[r.revisions.length - 1].name])
);
const roleOf = new Map<string, string | null>(Object.entries(rolesData.employees));
/** When this fake first saw an agent paused: the mock office "confirms" the stop a few seconds later. */
const pausedSince = new Map<string, number>();
const conflicted = new Set<string>();

const on = (name: string) => mockMode().split(',').includes(name);
const fail = (code: string, message: string, extra: Record<string, unknown> = {}) =>
	Object.assign(new Error(message), { source: 'api', code, message, ...extra });
const ago = (ms: number) => new Date(Date.now() - ms).toISOString();

async function read() {
	await new Promise((r) => setTimeout(r, on('slow') ? 2500 : 200));
	if (on('unavailable')) await unavailableCall();
	if (on('fails'))
		throw {
			source: 'setup',
			code: 'unreachable',
			message: "HQ didn't answer. Check that the office is running, then try again."
		};
}

async function write() {
	await new Promise((r) => setTimeout(r, on('slow') ? 2500 : 400));
	if (on('unavailable')) await unavailableCall();
	if (on('viewer')) throw fail('not_authorized', "You don't have access to change agent setup.");
	if (on('save-fails'))
		throw fail(
			'storage_error',
			"HQ couldn't write the agent's files. Nothing was changed; try again."
		);
}

const storedFor = (id: string): Stored =>
	stored.get(id) ?? {
		profileId: null,
		modelId: null,
		revision: 0,
		employeeRevision: 1,
		generation: 1,
		readiness: 'known'
	};

const profileOf = (s: Stored) => profiles.find((p) => p.id === s.profileId);
const runtimesOf = (p: Profile | undefined) => [
	...new Set((p?.connectionIds ?? []).map((c) => connections[c]?.runtime).filter(Boolean))
];

async function row(organizationId: string, m: Member): Promise<AgentSetupRow> {
	const snapshot = await snapshotFor(organizationId);
	const offices = await officesFor(organizationId);
	const s = storedFor(m.id);
	const profile = profileOf(s);
	const office = offices.find((o) => o.id === m.office_id);
	const team = snapshot.teams.find((t) => t.id === homeTeamId(snapshot, m.id));
	const roleId = s.roleId !== undefined ? s.roleId : (roleOf.get(m.id) ?? null);
	const placement = office
		? {
				office: { id: office.id, name: office.name },
				generation: s.generation ?? 1,
				...(s.observedAgoS !== undefined ? { observedAt: ago(s.observedAgoS * 1000) } : {})
			}
		: null;
	const readiness: Readiness = !placement
		? { known: false, why: `${m.name} isn't placed in an office, so no office can check it.` }
		: s.readiness === 'unknown'
			? { known: false, why: s.unknownWhy ?? "The office hasn't reported readiness yet." }
			: {
					known: true,
					checkedAt: ago((s.observedAgoS ?? 30) * 1000),
					blockers: profile?.blockers ?? []
				};
	const paused = m.agent?.status === 'paused';
	if (paused && !pausedSince.has(m.id)) pausedSince.set(m.id, Date.now());
	if (!paused) pausedSince.delete(m.id);
	return {
		memberId: m.id,
		name: m.name,
		handle: handleOf(m),
		...(roleId && roleNames.has(roleId) ? { role: roleNames.get(roleId) } : {}),
		...(m.agent?.role ? { roleText: m.agent.role } : {}),
		builtIn: m.agent?.messaging_name === 'hr',
		status: paused ? 'paused' : 'active',
		...(team ? { homeTeam: { id: team.id, name: team.name } } : {}),
		execution:
			profile && s.modelId
				? {
						profile: { id: profile.id, name: profile.name },
						modelId: s.modelId,
						enabled: s.enabled ?? true,
						runtimes: runtimesOf(profile)
					}
				: null,
		placement,
		readiness,
		catalog:
			placement && s.readiness === 'known' && profile?.models
				? {
						reported: true,
						models: profile.models.map((x) => x.id),
						asOf: ago((s.observedAgoS ?? 30) * 1000)
					}
				: { reported: false }
	};
}

async function currentAgents(organizationId: string) {
	const snapshot = await snapshotFor(organizationId);
	return snapshot.members.filter((m) => m.kind === 'agent' && m.agent?.status !== 'offboarded');
}

async function detail(organizationId: string, memberId: string): Promise<AgentSetupDetail> {
	const m = (await currentAgents(organizationId)).find((x) => x.id === memberId);
	if (!m || on('not-found'))
		throw fail('not_found', "There's no current agent with this ID in the organization.");
	const snapshot = await snapshotFor(organizationId);
	const base = await row(organizationId, m);
	const s = storedFor(m.id);
	const profile = profileOf(s);
	const boss = snapshot.members.find((b) => b.id === m.reports_to_member_id && b.id !== m.id);
	const roleId = s.roleId !== undefined ? s.roleId : (roleOf.get(m.id) ?? null);
	const checkedAt = base.placement ? ago((s.observedAgoS ?? 30) * 1000) : undefined;
	const paused = base.status === 'paused';
	const choices: ProfileChoice[] = profiles.map((p) => ({
		id: p.id,
		name: p.name,
		runtimes: runtimesOf(p),
		models: p.models
	}));
	return {
		...base,
		execution: base.execution
			? {
					...base.execution,
					revision: s.revision,
					connections: (profile?.connectionIds ?? []).map((id) => ({
						id,
						name: connections[id].name,
						runtime: connections[id].runtime,
						state: base.placement && s.readiness === 'known' ? connections[id].state : 'unknown',
						...(checkedAt && s.readiness === 'known' ? { checkedAt } : {})
					})),
					tools: data.tools,
					hasPrompt: true,
					...(s.updated
						? {
								updated: {
									byName: s.updated.byName,
									at: s.updated.at ?? ago((s.updated.agoH ?? 1) * 3600_000)
								}
							}
						: {})
				}
			: null,
		employeeRevision: s.employeeRevision,
		...(roleId ? { roleId } : {}),
		...(boss ? { reportsTo: { id: boss.id, name: boss.name } } : {}),
		...(base.placement && s.observedRuntime ? { observedRuntime: s.observedRuntime } : {}),
		...(base.placement ? { managedHome: `/srv/agentic-org/lumen/agents/${handleOf(m)}` } : {}),
		holds: paused ? ['Manual pause'] : [],
		stopPending: paused && Date.now() - (pausedSince.get(m.id) ?? 0) < 6000,
		profiles: choices,
		roles: [...roleNames]
			.map(([id, name]) => ({ id, name }))
			.sort((a, b) => a.name.localeCompare(b.name)),
		viewer: { canChange: !on('viewer') }
	};
}

/** Someone else saved first: the stored record moves on, and the refusal carries what is now there. */
async function conflictOnce(
	organizationId: string,
	memberId: string,
	which: 'execution' | 'employee'
) {
	if (!on('conflict') || conflicted.has(`${memberId}/${which}`)) return;
	conflicted.add(`${memberId}/${which}`);
	const s = storedFor(memberId);
	if (which === 'execution')
		stored.set(memberId, {
			...s,
			revision: s.revision + 1,
			updated: { byName: 'Sam Okafor', at: new Date().toISOString() }
		});
	else stored.set(memberId, { ...s, employeeRevision: s.employeeRevision + 1 });
	throw fail(
		'conflict',
		which === 'execution'
			? "This agent's setup changed while you were editing. The newer version is shown; your choices are kept."
			: "This agent's record changed while you were editing. The newer version is shown; your choice is kept.",
		{ latest: await detail(organizationId, memberId) }
	);
}

export const agentSetupApi: AgentSetupApi = {
	async list(organizationId) {
		await read();
		const agents = on('empty') ? [] : await currentAgents(organizationId);
		return {
			agents: await Promise.all(agents.map((m) => row(organizationId, m))),
			viewer: { canChange: !on('viewer') }
		};
	},
	async agent(organizationId, memberId) {
		await read();
		return detail(organizationId, memberId);
	},
	async saveExecution(organizationId, memberId, edit) {
		await write();
		await detail(organizationId, memberId);
		await conflictOnce(organizationId, memberId, 'execution');
		const s = storedFor(memberId);
		if (edit.expectedRevision !== s.revision)
			throw fail(
				'conflict',
				"This agent's setup changed while you were editing. The newer version is shown; your choices are kept.",
				{ latest: await detail(organizationId, memberId) }
			);
		if (!profiles.some((p) => p.id === edit.profileId))
			throw fail(
				'invalid_request',
				"That model profile isn't in the organization any more. Choose another."
			);
		if (!edit.modelId.trim()) throw fail('invalid_request', 'Choose a model to save.');
		stored.set(memberId, {
			...s,
			profileId: edit.profileId,
			modelId: edit.modelId,
			enabled: true,
			revision: s.revision + 1,
			updated: { byName: 'Alex Kim', at: new Date().toISOString() }
		});
		return detail(organizationId, memberId);
	},
	async setRole(organizationId, memberId, edit) {
		await write();
		await detail(organizationId, memberId);
		await conflictOnce(organizationId, memberId, 'employee');
		const s = storedFor(memberId);
		if (edit.expectedRevision !== s.employeeRevision)
			throw fail(
				'conflict',
				"This agent's record changed while you were editing. The newer version is shown; your choice is kept.",
				{ latest: await detail(organizationId, memberId) }
			);
		if (edit.roleId && !roleNames.has(edit.roleId))
			throw fail('not_found', "That role isn't in the organization any more. Choose another.");
		stored.set(memberId, { ...s, roleId: edit.roleId, employeeRevision: s.employeeRevision + 1 });
		return detail(organizationId, memberId);
	}
};
