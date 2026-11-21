// Mock for src/lib/modules/members/api.ts under `bun run dev:mock`. Members, teams and projects come from the org
// mock (so IDs, names and edits made through the existing dialogs match every page); what the org snapshot does not
// carry yet (handles for people, presence, readiness, turns, sign-in access) is JSON in e2e/fakes/data/members.
// `?mock=slow|fails|empty` on the page's URL exercises the loading, error and empty states; the directory also answers
// `?mock=unavailable|no-agents|all-former|race`. Sign-in access is kept here only, so Members, the profile and
// Settings › Members and access agree in one session; those three sign-in actions also answer
// `?mock=refuse|viewer` (refused: not the owner or the administrator), `?mock=gone` (the first action on Leo Brandt
// finds he has left the organization) and `?mock=deactivated` (Hana Weiss is deactivated: no access, no actions).
import { invalidate } from '$app/navigation';
import { orgApi } from '$lib/modules/organization/api';
import { newId } from '$lib/modules/organization/commands';
import { officeApi } from '$lib/modules/offices/office-api';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { Member, OrgSnapshot } from '$lib/proto';
import { mockLatency, mockMode } from '../mock-mode';
import type { MembersApi } from '../../../src/lib/modules/members/api';
import { busyText } from '../../../src/lib/modules/members/present';
import type {
	ExecutionView,
	MemberProfile,
	MemberRow,
	PresenceId,
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
	/** People who left the organization during this session (`?mock=gone`): no page lists them any more. */
	left: new Set<string>(),
	added: new Map<string, Turn[]>(),
	/** One Run now check fails once under `?mock=refresh_fails_once` (the profile keeps its page). */
	refreshFailed: false,
	assigned: new Map<string, string>()
};

/** How long the fake office takes to confirm a Run now: HQ records it `accepted`, then the office starts it. */
const CONFIRM_MS = 4000;
const pendingAttempts = new Map<string, { connection: string; model: string }>();

/** The turns Run now added for a member, with any the office has confirmed since moved to `running`. */
function addedTurns(memberId: string): Turn[] {
	const list = state.added.get(memberId) ?? [];
	for (const t of list)
		if (t.state === 'accepted' && Date.now() - Date.parse(t.startedAt) >= CONFIRM_MS) {
			const route = pendingAttempts.get(t.id) ?? {
				connection: 'Default connection',
				model: 'unknown'
			};
			t.state = 'running';
			t.attempts = [
				{
					n: 1,
					...route,
					startedAt: new Date().toISOString(),
					outcome: 'running',
					usageComplete: false
				}
			];
		}
	return list;
}

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

// ---- home teams set by Move home team ----

/**
 * Kept in localStorage beside the org mock (which persists the membership change), per organization, so a reload
 * keeps the move. Each entry names the membership the move created; once that membership is gone (an org-chart move
 * or removal later), the entry no longer applies and the later change wins.
 */
const HOME_KEY = 'agentic-org.mock.members.home';
type HomeEntry = { teamId: string; membershipId: string };
type HomeStore = Record<string, Record<string, HomeEntry>>;

function readHomes(): HomeStore {
	try {
		const parsed = JSON.parse(localStorage.getItem(HOME_KEY) ?? '{}');
		return parsed && typeof parsed === 'object' ? (parsed as HomeStore) : {};
	} catch {
		return {};
	}
}

function recordHome(organizationId: string, memberId: string, entry: HomeEntry) {
	const all = readHomes();
	all[organizationId] = { ...(all[organizationId] ?? {}), [memberId]: entry };
	localStorage.setItem(HOME_KEY, JSON.stringify(all));
}

/** The moved-to home team, while the member still belongs to it in this snapshot. */
function movedHome(s: OrgSnapshot, memberId: string): string | undefined {
	const m = s.members.find((x) => x.id === memberId);
	if (!m?.home_team_id) return undefined;
	for (const byMember of Object.values(readHomes())) {
		const e = byMember?.[memberId];
		if (e && m.home_team_id === e.teamId) return e.teamId;
	}
	return undefined;
}

/**
 * One home team (the employee's `home_team_id`): a Move home team made here, else the member's `home_team_id`, else
 * the sample's recorded value.
 */
export function homeTeamId(s: OrgSnapshot, memberId: string): string | undefined {
	const exists = (id: string | undefined) => !!id && s.teams.some((t) => t.id === id);
	const moved = movedHome(s, memberId);
	if (moved) return moved;
	const direct = s.members.find((m) => m.id === memberId)?.home_team_id;
	if (exists(direct)) return direct;
	const recorded = extras[memberId]?.homeTeamId;
	if (exists(recorded)) return recorded;
	return undefined;
}

/**
 * Handles chosen when a person was added (teams.ts Add a person): the one map every page reads, so People, the
 * profile, the team page and the org chart show the same @handle, and uniqueness is checked against it.
 */
const chosenHandles = new Map<string, string>();
export const recordHandle = (memberId: string, handle: string) =>
	chosenHandles.set(memberId, handle);

export const handleOf = (m: Member) =>
	chosenHandles.get(m.id) ??
	(m.kind === 'agent'
		? (m.agent?.messaging_name ?? m.id)
		: (extras[m.id]?.handle ?? m.name.toLowerCase().replace(/[^a-z0-9]+/g, '-')));

const modes = () => mockMode().split(',');
/** `?mock=deactivated`: Hana Weiss is a deactivated person (the org snapshot records deactivation only for agents). */
const deactivatedPerson = (m: Member) =>
	m.kind === 'person' && m.id === 'hana' && modes().includes('deactivated');

const statusOf = (m: Member): MemberRow['status'] =>
	m.agent?.status === 'paused'
		? 'paused'
		: m.agent?.status === 'offboarded' || deactivatedPerson(m)
			? 'inactive'
			: 'active';

// Deactivating a person also removes their access (offices-and-teams-ux §7.4).
const hasAccount = (id: string) =>
	!(id === 'hana' && modes().includes('deactivated')) &&
	(state.access.get(id)?.account ?? extras[id]?.canSignIn ?? false);

/**
 * A person's sign-in access as every page reads it: an account, or an open invitation until the person registers.
 * Settings › Members and access (access.ts) and General (settings.ts) read it from here, never from their own copy.
 */
export function signInOf(memberId: string): { account: boolean; invitedUntil?: string } {
	const account = hasAccount(memberId);
	const invitedUntil = account ? undefined : state.access.get(memberId)?.invitedUntil;
	return { account, ...(invitedUntil ? { invitedUntil } : {}) };
}

/** `?mock=invited` on Settings › Members and access: an open invitation, as if made earlier; once only. */
export function seedInvitation(memberId: string, inMs: number) {
	if (hasAccount(memberId) || state.access.has(memberId)) return;
	state.access.set(memberId, {
		account: false,
		invitedUntil: new Date(Date.now() + inMs).toISOString()
	});
}

/** Deactivated (`inactive`), as the directory and the profile say it. */
export const isInactive = (m: Member) => statusOf(m) === 'inactive';

/** Members still in the organization: someone who left this session is no longer listed anywhere. */
export const present = (m: Member) => !state.left.has(m.id);

function presenceOf(m: Member): MemberRow['presence'] {
	if (statusOf(m) === 'inactive') return undefined;
	if (statusOf(m) === 'paused') return { id: 'paused' };
	const live = addedTurns(m.id).find((t) => t.state === 'accepted' || t.state === 'running');
	if (live?.state === 'running') return { id: 'working' };
	// Requested, not started: able to run, with the request named, never Working before the office says so.
	if (live?.state === 'accepted')
		return { id: 'available', detail: '· run requested, waiting for the office' };
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
		.filter((m) => (m.kind === 'agent' || m.kind === 'person') && present(m))
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
	const live = addedTurns(m.id).find((t) => t.state === 'accepted' || t.state === 'running');
	if (live?.state === 'accepted')
		return {
			health: "Run requested · the office hasn't started the turn yet",
			tone: 'waiting',
			holds: x.supervision?.holds ?? []
		};
	if (live?.state === 'running')
		return {
			health: 'Working · the office started the turn',
			tone: 'ok',
			holds: x.supervision?.holds ?? []
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
		turnState: (addedTurns(m.id).find((t) => t.state === 'accepted' || t.state === 'running')
			?.state ?? e.turnState) as TurnState | 'idle',
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
	return [...addedTurns(memberId), ...turns];
}

/** Moves every `timestamp` in a recorded log so its earliest one falls at `start`, keeping the gaps. */
export function shiftTimes(lines: unknown[], start: number) {
	const stamps: { o: Record<string, unknown>; t: number }[] = [];
	const walk = (v: unknown) => {
		if (Array.isArray(v)) v.forEach(walk);
		else if (v && typeof v === 'object') {
			const o = v as Record<string, unknown>;
			if (typeof o.timestamp === 'string' && Number.isFinite(Date.parse(o.timestamp)))
				stamps.push({ o, t: Date.parse(o.timestamp) });
			Object.values(o).forEach(walk);
		}
	};
	walk(lines);
	if (!stamps.length) return;
	const shift = start - Math.min(...stamps.map((x) => x.t));
	for (const { o, t } of stamps) o.timestamp = new Date(t + shift).toISOString();
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

const member = async (organizationId: string, memberId: string) => {
	const s = await snapshotFor(organizationId);
	const m = s.members.find((x) => x.id === memberId && present(x));
	if (!m) throw fail('not_found', "That member isn't in this organization.");
	return { s, m };
};

/**
 * The sign-in actions' own switches, read when a call is made: `refuse` and `viewer` refuse as the web API does for
 * anyone but the owner or the administrator; `gone` makes Leo Brandt leave the organization on the first action.
 */
async function signInGate(memberId: string) {
	await gate();
	await mockLatency(250);
	if (modes().includes('refuse') || modes().includes('viewer'))
		throw fail('not_authorized', 'Only the owner or the administrator changes sign-in access.');
	if (modes().includes('gone') && memberId === 'leo') state.left.add('leo');
}

const link = (kind: string) => ({
	url: `http://127.0.0.1:7420/${kind}#${Math.random().toString(36).slice(2, 10)}${Math.random().toString(36).slice(2, 10)}`,
	expiresAt: new Date(Date.now() + 24 * 3600_000).toISOString()
});

// `?mock=race`: the read an add starts is superseded by a live change. The first read after mounting (the add's) waits
// 800 ms while the organization changes again; it resolves first but is superseded; a newer read brings the row 1.5 s
// later.
let raceReads = 0;
async function race() {
	raceReads++;
	if (raceReads === 2) {
		setTimeout(() => void invalidate('app:org'), 100);
		await new Promise((r) => setTimeout(r, 800));
	} else if (raceReads === 3) await new Promise((r) => setTimeout(r, 1500));
}

async function profileOf(organizationId: string, memberId: string): Promise<MemberProfile> {
	const { s, m } = await member(organizationId, memberId);
	const row = (await rows(organizationId)).find((r) => r.id === memberId)!;
	const x = extras[memberId] ?? {};
	const offices = await officesFor(organizationId);
	const viewer = viewerId();
	const ownerId = ownerIdOf(s);
	const isOwner = viewer === ownerId;
	const leads = s.teams.filter((t) => t.lead_member_id === memberId).map((t) => teamRef(s, t.id)!);
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
		openTurn: open ? { id: open.id, startedAt: open.startedAt, state: open.state } : undefined,
		cannotStart,
		invitePending:
			m.kind === 'person' && !row.canSignIn && state.access.get(memberId)?.invitedUntil
				? { expiresAt: state.access.get(memberId)!.invitedUntil! }
				: undefined,
		viewer: { isSelf: self, isOwnerOrAdmin: isOwner, costVisible: isOwner }
	} satisfies MemberProfile;
}

export const membersApi: MembersApi = {
	async directory(organizationId) {
		await gate();
		if (scenario() === 'unavailable') await unavailableCall();
		if (scenario() === 'empty') return [];
		if (scenario() === 'race') await race();
		const all = await rows(organizationId);
		// `?mock=no-agents` shows the Agents filter's empty state; `?mock=all-former` the one with only former members.
		if (scenario() === 'no-agents') return all.filter((r) => r.kind !== 'agent');
		if (scenario() === 'all-former')
			return all.map((r) => ({ ...r, status: 'inactive' as const, presence: undefined }));
		return all;
	},
	async profile(organizationId, memberId) {
		await gate();
		if (
			scenario() === 'refresh_fails_once' &&
			!state.refreshFailed &&
			addedTurns(memberId).some((t) => t.state === 'accepted')
		) {
			state.refreshFailed = true;
			throw fail('unavailable_now', "HQ didn't answer this time.");
		}
		return profileOf(organizationId, memberId);
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
	async sessionFile(organizationId, memberId, runId) {
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
		// The sample log was recorded on another day: move its times onto this run's, so the header's start time and
		// the log's agree.
		const run = (await turnsFor(organizationId, memberId)).find((t) => t.id === runId);
		const lines = structuredClone(f.lines);
		if (run) shiftTimes(lines, Date.parse(run.startedAt));
		return { runId, memberId, format: f.format, lines };
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
		if (profile.openTurn) throw fail('busy', busyText(profile.name, profile.openTurn));
		if (profile.cannotStart) throw fail('cannot_start', profile.cannotStart);
		const turn: Turn = {
			id: `t-new-${Date.now()}`,
			startedAt: new Date().toISOString(),
			startedBy: { kind: 'run', byViewer: true, by: { id: viewerId(), name: 'you' } },
			// HQ records the request; the office confirms the start later (addedTurns).
			state: 'accepted',
			summary: note || undefined,
			// No attempt until the office starts one.
			attempts: [],
			hasLog: false
		};
		pendingAttempts.set(turn.id, {
			connection: profile.execution?.connection ?? 'Default connection',
			model: profile.execution?.requested?.model ?? 'unknown'
		});
		state.added.set(memberId, [turn, ...(state.added.get(memberId) ?? [])]);
		return {
			turn: structuredClone(turn),
			profile: await profileOf(organizationId, memberId)
		};
	},
	async setAllowHiring(organizationId, memberId, on) {
		await gate();
		const profile = await membersApi.profile(organizationId, memberId);
		if (!profile.allowHiring.canChange)
			throw fail('not_authorized', profile.allowHiring.reason ?? "You can't change this.");
		state.allowHiring.set(memberId, on);
	},
	async moveHomeTeam(organizationId, memberId, toTeamId) {
		await gate();
		const { s, m } = await member(organizationId, memberId);
		const to = s.teams.find((t) => t.id === toTeamId);
		if (!to) throw fail('not_found', "That team isn't in this organization any more.");
		const from = homeTeamId(s, memberId);
		if (from === toTeamId || m.home_team_id === toTeamId)
			throw fail('conflict', `${to.name} is already ${m.name}'s home team.`);
		// Move the member's home team in one command so the org chart and team pages agree.
		const membershipId = newId();
		await orgApi.command(organizationId, {
			method: 'member.move',
			params: {
				membership_id: m.id,
				expected_version: m.version,
				new_membership_id: membershipId,
				to_team_id: toTeamId
			}
		});
		recordHome(organizationId, memberId, { teamId: toTeamId, membershipId });
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
		await signInGate(memberId);
		const { m } = await member(organizationId, memberId);
		if (m.kind !== 'person') throw fail('invalid', 'An account is never tied to an agent.');
		if (statusOf(m) === 'inactive')
			throw fail('conflict', `${m.name} is deactivated. Reactivate ${m.name} first.`);
		if (hasAccount(memberId))
			throw fail('conflict', `${m.name} can already sign in. Make a new sign-in link instead.`);
		// Inviting again cancels the previous link: only the newest one is remembered.
		const issued = link('register');
		state.access.set(memberId, { account: false, invitedUntil: issued.expiresAt });
		return issued;
	},
	async newSignInLink(organizationId, memberId) {
		await signInGate(memberId);
		const { m } = await member(organizationId, memberId);
		if (m.kind !== 'person') throw fail('invalid', 'An account is never tied to an agent.');
		if (!hasAccount(memberId))
			throw fail('invalid', `${m.name} has no account yet. Invite ${m.name} to sign in instead.`);
		return link('reset');
	},
	async removeAccess(organizationId, memberId) {
		await signInGate(memberId);
		const { s, m } = await member(organizationId, memberId);
		if (m.kind !== 'person') throw fail('invalid', 'An account is never tied to an agent.');
		if (!hasAccount(memberId)) throw fail('invalid', `${m.name} has no account to remove.`);
		if (ownerIdOf(s) === memberId) throw fail('conflict', 'Hand over ownership first.');
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
