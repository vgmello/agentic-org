// Mock for src/lib/modules/members/api.ts under `bun run dev:mock`. Members, teams and projects come from the org
// mock (so IDs, names and edits made through the existing dialogs match every page); what the org snapshot does not
// carry yet (handles for people, presence, readiness, turns, sign-in access) is JSON in e2e/fakes/data/members.
// `?mock=slow|fails|empty` on the page's URL exercises the loading, error and empty states.
import { orgApi } from '$lib/org/api';
import { officeApi } from '$lib/office/api';
import type { Member, OrgSnapshot } from '$lib/proto';
import type { MembersApi } from '../../../src/lib/modules/members/api';
import type {
	ExecutionView,
	MemberProfile,
	MemberRow,
	PresenceId,
	ProcessRow,
	TeamRef,
	Turn,
	TurnState,
	WorkRow,
	WorkState
} from '../../../src/lib/modules/members/types';
import data from '../data/members/members.json' with { type: 'json' };
import runsData from '../data/members/runs.json' with { type: 'json' };
import tasksData from '../data/members/tasks.json' with { type: 'json' };
import activityData from '../data/members/activity.json' with { type: 'json' };
import conversationsData from '../data/members/conversations.json' with { type: 'json' };
import processesData from '../data/members/processes.json' with { type: 'json' };
import rolesData from '../data/members/roles.json' with { type: 'json' };
import teamsData from '../data/teams/teams.json' with { type: 'json' };
import claudeSession from '../data/members/sessions/claude-noor-run.json' with { type: 'json' };
import codexSession from '../data/members/sessions/codex-kai-run.json' with { type: 'json' };

type Extra = {
	handle?: string;
	canSignIn?: boolean;
	presence?: { id: string; detail?: string };
	allowHiring?: boolean;
	homeTeamId?: string;
	contact?: string;
	title?: string;
	workingOn?: { ref: string; title: string; project?: string };
	hired?: { agoDays: number; by: string };
	runsOn?: string;
	howItRuns?: string;
	execution?: {
		pool: string | null;
		model?: string;
		connection?: string;
		turnState: string;
		generation?: number;
		observedAgoS?: number;
		reasons: string[];
		home?: string;
	};
	supervision?: { health: string; tone: string; holds: string[] };
	openTurn?: string;
};
const extras = data.members as Record<string, Extra>;
const teamExtras = teamsData as Record<
	string,
	{ number: number; purpose?: string; maxMembers?: number }
>;

// ---- scenarios and errors ----

export const scenario = () => new URLSearchParams(location.search).get('mock');
export async function gate() {
	if (scenario() === 'slow') await new Promise((r) => setTimeout(r, 2500));
	if (scenario() === 'fails')
		throw {
			source: 'api',
			code: 'internal',
			message: "HQ didn't answer. Check that the office is running, then try again."
		};
}
export const fail = (code: string, message: string) =>
	Object.assign(new Error(message), { source: 'api', code, message });

const ago = (ms: number) => new Date(Date.now() - ms).toISOString();
const MIN = 60_000;

// ---- in-memory state the mutations change ----

const state = {
	ownerId: undefined as string | undefined,
	allowHiring: new Map<string, boolean>(),
	/** Sign-in access changed here: an account, or an open invitation until the person registers. */
	access: new Map<string, { account: boolean; invitedUntil?: string }>(),
	added: new Map<string, Turn[]>(),
	assigned: new Map<string, string>()
};

// ---- shared reads (teams.ts and employees.ts use these too) ----

export async function snapshotFor(organizationId: string): Promise<OrgSnapshot> {
	return (await orgApi.view(organizationId)).snapshot;
}

export async function officesFor(organizationId: string) {
	const connection = (await officeApi.connections()).find(
		(c) => c.organization.id === organizationId
	);
	return connection?.organization.offices ?? [];
}

export const viewerId = () => data.viewer;
export const ownerIdOf = (s: OrgSnapshot) => state.ownerId ?? s.members.find((m) => m.owner)?.id;

export function teamRef(s: OrgSnapshot, id: string | undefined): TeamRef | undefined {
	const team = s.teams.find((t) => t.id === id);
	if (!team) return undefined;
	const n = teamExtras[team.id]?.number;
	return { id: team.id, name: team.name, ref: n ? `TEAM-${n}` : '' };
}

/** One home team: the recorded override, else the first membership (the snapshot still allows several). */
export function homeTeamId(s: OrgSnapshot, memberId: string): string | undefined {
	const override = extras[memberId]?.homeTeamId;
	if (override && s.teams.some((t) => t.id === override)) return override;
	return s.memberships.find(
		(m) => m.member_id === memberId && s.teams.some((t) => t.id === m.team_id)
	)?.team_id;
}

export const handleOf = (m: Member) =>
	m.kind === 'agent'
		? (m.agent?.messaging_name ?? m.id)
		: (extras[m.id]?.handle ?? m.name.toLowerCase().replace(/[^a-z0-9]+/g, '-'));

const hasAccount = (id: string) => state.access.get(id)?.account ?? extras[id]?.canSignIn ?? false;

const statusOf = (m: Member): MemberRow['status'] =>
	m.agent?.status === 'paused'
		? 'paused'
		: m.agent?.status === 'offboarded'
			? 'inactive'
			: 'active';

function presenceOf(m: Member): MemberRow['presence'] {
	if (statusOf(m) === 'inactive') return undefined;
	if (statusOf(m) === 'paused') return { id: 'paused' };
	const p = extras[m.id]?.presence;
	// Available means able to run (ADR-0028): an agent with a reason it can't start isn't shown as Available.
	if (p?.id === 'available' && m.kind === 'agent' && extras[m.id]?.execution?.reasons.length)
		return undefined;
	return p ? { id: p.id as PresenceId, ...(p.detail ? { detail: p.detail } : {}) } : undefined;
}

export async function rows(organizationId: string): Promise<MemberRow[]> {
	const s = await snapshotFor(organizationId);
	const offices = await officesFor(organizationId);
	const ownerId = ownerIdOf(s);
	return s.members
		.filter((m) => m.kind === 'agent' || m.kind === 'person')
		.map((m): MemberRow => {
			const x = extras[m.id] ?? {};
			const office = m.kind === 'agent' ? offices.find((o) => o.id === m.office_id) : undefined;
			const boss = s.members.find((b) => b.id === m.reports_to_member_id && b.id !== m.id);
			const assigned = [...state.assigned].find(([, who]) => who === m.id)?.[0];
			const task = assigned ? tasksData.find((t) => t.ref === assigned) : undefined;
			return {
				id: m.id,
				kind: m.kind as 'agent' | 'person',
				name: m.name,
				handle: handleOf(m),
				role: m.agent?.title || m.agent?.role || x.title,
				status: statusOf(m),
				owner: m.id === ownerId,
				canSignIn: m.kind === 'person' && hasAccount(m.id),
				presence: presenceOf(m),
				homeTeam: teamRef(s, homeTeamId(s, m.id)),
				office:
					m.kind === 'agent'
						? office
							? {
									id: office.id,
									name: office.name,
									observedAt: ago((x.execution?.observedAgoS ?? 60) * 1000)
								}
							: null
						: undefined,
				reportsTo: boss ? { id: boss.id, name: boss.name } : undefined,
				workingOn: x.workingOn ?? (task ? { ref: task.ref, title: task.title } : undefined),
				runsOn: m.kind === 'agent' ? x.runsOn : undefined,
				howItRuns: m.kind === 'agent' ? x.howItRuns : undefined,
				contact: m.contact ?? x.contact,
				directReports: s.members.filter((r) => r.reports_to_member_id === m.id && r.id !== m.id)
					.length,
				inactiveSince: m.agent?.offboarded_at
			};
		});
}

/** A pause made here has no office confirmation yet: stop pending, and the manual hold named (DEAL-3). */
function supervisionOf(m: Member, x: Extra): MemberProfile['supervision'] {
	if (m.kind !== 'agent' || statusOf(m) === 'inactive') return undefined;
	if (statusOf(m) === 'paused' && !x.supervision?.holds.length)
		return {
			health: "Pause requested · the office hasn't confirmed the stop yet",
			tone: 'waiting',
			holds: ['Manual pause'],
			stopPending: true
		};
	return x.supervision
		? { ...x.supervision, tone: x.supervision.tone as 'ok' | 'waiting' | 'problem' | 'unknown' }
		: undefined;
}

function execution(m: Member, x: Extra, officeName: string | undefined): ExecutionView | undefined {
	const e = x.execution;
	if (m.kind !== 'agent' || !e) return undefined;
	return {
		requested: e.pool && e.model ? { pool: e.pool, model: e.model } : null,
		connection: e.connection,
		turnState: (state.added.get(m.id)?.some((t) => t.state === 'running')
			? 'running'
			: e.turnState) as TurnState | 'idle',
		office:
			officeName && e.generation
				? {
						name: officeName,
						generation: e.generation,
						observedAt: ago((e.observedAgoS ?? 60) * 1000)
					}
				: undefined,
		readiness: { known: true, reasons: e.reasons },
		home: e.home
	};
}

// ---- turns ----

const nameOf = (s: OrgSnapshot, id: string | undefined) =>
	s.members.find((m) => m.id === id)?.name ?? id ?? '';

async function turnsFor(organizationId: string, memberId: string): Promise<Turn[]> {
	const s = await snapshotFor(organizationId);
	const listed = ((runsData as Record<string, unknown[]>)[memberId] ?? []) as {
		id: string;
		startedAgoMin: number;
		durationS?: number;
		startedBy: { kind: string; by?: string };
		state: string;
		summary?: string;
		failure?: string;
		attempts: {
			connection: string;
			model: string;
			outcome: string;
			reason?: string;
			input?: number;
			output?: number;
			usageComplete: boolean;
		}[];
		cost: { usd: number; complete: boolean } | null;
		hasLog: boolean;
	}[];
	const turns = listed.map((t): Turn => {
		const startedAt = ago(t.startedAgoMin * MIN);
		const endedAt =
			t.durationS !== undefined
				? new Date(Date.parse(startedAt) + t.durationS * 1000).toISOString()
				: undefined;
		return {
			id: t.id,
			startedAt,
			endedAt,
			startedBy: {
				kind: t.startedBy.kind as Turn['startedBy']['kind'],
				...(t.startedBy.by
					? {
							by: { id: t.startedBy.by, name: nameOf(s, t.startedBy.by) },
							byViewer: t.startedBy.by === viewerId()
						}
					: {})
			},
			state: t.state as TurnState,
			summary: t.summary,
			failure: t.failure,
			cost: t.cost ?? undefined,
			hasLog: t.hasLog,
			attempts: t.attempts.map((a, i) => ({
				n: i + 1,
				connection: a.connection,
				model: a.model,
				outcome: a.outcome as Turn['attempts'][number]['outcome'],
				reason: a.reason,
				inputTokens: a.input,
				outputTokens: a.output,
				usageComplete: a.usageComplete
			}))
		};
	});
	return [...(state.added.get(memberId) ?? []), ...turns];
}

// ---- work, activity, conversations ----

export function workRows(
	s: OrgSnapshot,
	filter: (t: (typeof tasksData)[number]) => boolean
): WorkRow[] {
	return tasksData
		.map((t) => ({ ...t, assigneeId: state.assigned.get(t.ref) ?? t.assigneeId }))
		.filter(filter)
		.map((t) => ({
			ref: t.ref,
			title: t.title,
			project: s.projects?.find((p) => p.id === t.projectId)?.name,
			assignee: t.assigneeId ? nameOf(s, t.assigneeId) : undefined,
			priority: t.priority as WorkRow['priority'],
			state: t.state as WorkState
		}));
}

export const activityFor = (pick: (a: (typeof activityData)[number]) => boolean) =>
	activityData.filter(pick).map((a) => ({
		id: a.id,
		at: ago(a.agoMin * MIN),
		actor: a.actor,
		kind: a.kind,
		text: a.text
	}));

export const conversationsFor = (pick: (c: (typeof conversationsData)[number]) => boolean) =>
	conversationsData.filter(pick).map((c) => ({
		id: c.id,
		title: c.title,
		kind: c.kind as 'channel' | 'direct' | 'thread',
		lastAt: ago(c.agoMin * MIN),
		preview: c.preview
	}));

/** The effective processes for a team: the organization's, with the team's replacements and additions. */
export function processesForTeam(s: OrgSnapshot, teamId: string | undefined): ProcessRow[] {
	const org: ProcessRow[] = processesData.organization.map((p) => ({
		...p,
		level: 'organization',
		relation: p.relation as ProcessRow['relation']
	}));
	const own = teamId
		? ((processesData.teams as Record<string, typeof processesData.organization>)[teamId] ?? [])
		: [];
	const teamName = teamRef(s, teamId)?.name;
	const mine: ProcessRow[] = own.map((p) => ({
		...p,
		level: 'team',
		from: teamName,
		relation: p.relation as ProcessRow['relation']
	}));
	return [...org.filter((o) => !mine.some((m) => m.key === o.key)), ...mine];
}

const member = async (organizationId: string, memberId: string) => {
	const s = await snapshotFor(organizationId);
	const m = s.members.find((x) => x.id === memberId);
	if (!m) throw fail('not_found', "That member isn't in this organization.");
	return { s, m };
};

const link = (kind: string) => ({
	url: `http://127.0.0.1:7420/${kind}#${Math.random().toString(36).slice(2, 10)}${Math.random().toString(36).slice(2, 10)}`,
	expiresAt: new Date(Date.now() + 24 * 3600_000).toISOString()
});

export const membersApi: MembersApi = {
	async directory(organizationId) {
		await gate();
		if (scenario() === 'empty') return [];
		return rows(organizationId);
	},
	async profile(organizationId, memberId) {
		await gate();
		const { s, m } = await member(organizationId, memberId);
		const row = (await rows(organizationId)).find((r) => r.id === memberId)!;
		const x = extras[memberId] ?? {};
		const offices = await officesFor(organizationId);
		const viewer = viewerId();
		const ownerId = ownerIdOf(s);
		const isOwner = viewer === ownerId;
		const leads = s.teams
			.filter((t) => t.lead_member_id === memberId)
			.map((t) => teamRef(s, t.id)!);
		// Only the owner, or a person above this employee, changes Allow hiring; nobody changes their own (§7.1).
		const above = (() => {
			let cur = m.reports_to_member_id;
			const seen = new Set<string>();
			while (cur && !seen.has(cur)) {
				if (cur === viewer) return true;
				seen.add(cur);
				cur = s.members.find((b) => b.id === cur)?.reports_to_member_id;
			}
			return false;
		})();
		const self = memberId === viewer;
		const allowOn = state.allowHiring.get(memberId) ?? x.allowHiring ?? m.kind === 'person';
		const turns = await turnsFor(organizationId, memberId);
		const open = turns.find((t) => t.state === 'running' || t.state === 'accepted');
		const exec = execution(m, x, offices.find((o) => o.id === m.office_id)?.name);
		const cannotStart =
			m.kind !== 'agent'
				? undefined
				: row.status === 'paused'
					? `${m.name} is paused. Resume ${m.name} to run it.`
					: row.status === 'inactive'
						? `${m.name} has been offboarded.`
						: exec?.readiness.reasons[0];
		return {
			...row,
			title: m.agent?.title ?? x.title,
			focus: m.agent?.focus,
			hired:
				m.kind === 'agent' && x.hired
					? {
							at: ago(x.hired.agoDays * 24 * 60 * MIN),
							by: { id: x.hired.by, name: nameOf(s, x.hired.by) }
						}
					: undefined,
			leads,
			allowHiring: {
				on: allowOn,
				canChange: !self && (isOwner || above),
				reason: self
					? "You can't change your own."
					: !(isOwner || above)
						? 'Only the owner, or a person above them, changes this.'
						: undefined
			},
			currentTask: row.workingOn,
			supervision: supervisionOf(m, x),
			execution: exec,
			openTurn: open ? { id: open.id, startedAt: open.startedAt } : undefined,
			cannotStart,
			invitePending:
				m.kind === 'person' && !row.canSignIn && state.access.get(memberId)?.invitedUntil
					? { expiresAt: state.access.get(memberId)!.invitedUntil! }
					: undefined,
			viewer: { isSelf: self, isOwnerOrAdmin: isOwner, costVisible: isOwner }
		} satisfies MemberProfile;
	},
	async work(organizationId, memberId) {
		await gate();
		const s = await snapshotFor(organizationId);
		return scenario() === 'empty' ? [] : workRows(s, (t) => t.assigneeId === memberId);
	},
	async conversations(_organizationId, memberId) {
		await gate();
		return scenario() === 'empty' ? [] : conversationsFor((c) => c.members.includes(memberId));
	},
	async activity(_organizationId, memberId) {
		await gate();
		return scenario() === 'empty' ? [] : activityFor((a) => a.members.includes(memberId));
	},
	async runs(organizationId, memberId) {
		await gate();
		if (scenario() === 'empty') return [];
		const turns = await turnsFor(organizationId, memberId);
		const s = await snapshotFor(organizationId);
		return ownerIdOf(s) === viewerId() ? turns : turns.map((t) => ({ ...t, cost: undefined }));
	},
	async sessionFile(_organizationId, memberId, runId) {
		await gate();
		const fixtures: Record<
			string,
			{ member: string; format: 'claude' | 'codex'; lines: unknown[] }
		> = {
			't-n4': { member: 'noor', format: 'claude', lines: claudeSession },
			't-k2': { member: 'kai', format: 'codex', lines: codexSession }
		};
		const f = fixtures[runId];
		if (!f || f.member !== memberId)
			throw fail('not_found', 'No session log was kept for this run.');
		return { runId, memberId, format: f.format, lines: structuredClone(f.lines) };
	},
	async processes(organizationId, memberId) {
		await gate();
		const s = await snapshotFor(organizationId);
		const hasCurrentWork = tasksData.some(
			(t) => (state.assigned.get(t.ref) ?? t.assigneeId) === memberId && t.state !== 'done'
		);
		return {
			hasCurrentWork,
			rows: processesForTeam(s, hasCurrentWork ? homeTeamId(s, memberId) : undefined)
		};
	},
	async role(organizationId, memberId) {
		await gate();
		const { m } = await member(organizationId, memberId);
		const role = (rolesData as Record<string, { name: string; revision: number; body: string }>)[
			memberId
		];
		return { role, roleText: m.agent?.role, title: m.agent?.title ?? extras[memberId]?.title };
	},
	async runNow(organizationId, memberId, note) {
		await gate();
		const profile = await membersApi.profile(organizationId, memberId);
		if (profile.openTurn)
			throw fail(
				'busy',
				`${profile.name} is already running a turn. It takes new input when that turn ends.`
			);
		if (profile.cannotStart) throw fail('cannot_start', profile.cannotStart);
		const turn: Turn = {
			id: `t-new-${Date.now()}`,
			startedAt: new Date().toISOString(),
			startedBy: { kind: 'run', byViewer: true, by: { id: viewerId(), name: 'you' } },
			state: 'running',
			summary: note || undefined,
			attempts: [
				{
					n: 1,
					connection: profile.execution?.connection ?? 'Default connection',
					model: profile.execution?.requested?.model ?? 'unknown',
					outcome: 'running',
					usageComplete: false
				}
			],
			hasLog: false
		};
		state.added.set(memberId, [turn, ...(state.added.get(memberId) ?? [])]);
		return turn;
	},
	async setAllowHiring(organizationId, memberId, on) {
		await gate();
		const profile = await membersApi.profile(organizationId, memberId);
		if (!profile.allowHiring.canChange)
			throw fail('not_authorized', profile.allowHiring.reason ?? "You can't change this.");
		state.allowHiring.set(memberId, on);
	},
	async assignableTasks(organizationId) {
		await gate();
		const s = await snapshotFor(organizationId);
		return tasksData
			.filter((t) => !(state.assigned.get(t.ref) ?? t.assigneeId) && t.state !== 'done')
			.map((t) => ({
				ref: t.ref,
				title: t.title,
				project: s.projects?.find((p) => p.id === t.projectId)?.name
			}));
	},
	async assignTask(_organizationId, memberId, ref) {
		await gate();
		state.assigned.set(ref, memberId);
	},
	async inviteToSignIn(organizationId, memberId) {
		await gate();
		const { m } = await member(organizationId, memberId);
		if (m.kind !== 'person') throw fail('invalid', 'An account is never tied to an agent.');
		if (hasAccount(memberId))
			throw fail('conflict', `${m.name} can already sign in. Make a new sign-in link instead.`);
		// Inviting again cancels the previous link: only the newest one is remembered.
		const issued = link('register');
		state.access.set(memberId, { account: false, invitedUntil: issued.expiresAt });
		return issued;
	},
	async newSignInLink(organizationId, memberId) {
		await gate();
		const { m } = await member(organizationId, memberId);
		if (m.kind !== 'person') throw fail('invalid', 'An account is never tied to an agent.');
		if (!hasAccount(memberId))
			throw fail('invalid', `${m.name} has no account yet. Invite ${m.name} to sign in instead.`);
		return link('reset');
	},
	async removeAccess(organizationId, memberId) {
		await gate();
		const { s, m } = await member(organizationId, memberId);
		if (m.kind !== 'person') throw fail('invalid', 'An account is never tied to an agent.');
		if (!hasAccount(memberId)) throw fail('invalid', `${m.name} has no account to remove.`);
		if (ownerIdOf(s) === memberId) throw fail('not_authorized', 'Hand over ownership first.');
		state.access.set(memberId, { account: false });
	},
	async handOverOwnership(organizationId, toMemberId, password) {
		await gate();
		if (password !== 'owner-password')
			throw fail('wrong_password', "That password isn't right. Nothing changed.");
		const s = await snapshotFor(organizationId);
		const to = s.members.find((m) => m.id === toMemberId);
		if (!to || to.kind !== 'person')
			throw fail('invalid', 'Only an active person with an account can become the owner.');
		state.ownerId = toMemberId;
	}
};
