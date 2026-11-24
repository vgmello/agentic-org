// Stateful dev/test mock for src/lib/modules/workitems/api.ts under `bun run dev:mock`.
// Teams, active employees and projects come from the org mock so IDs match every other page.
// Scenarios (`?mock=` on the URL, comma-separated):
//   slow · fails · empty · unavailable · unsupported · stale · viewer · agent-pm · conflict
//   budget-unknown · budget-stop-pending · budget-extended · create-unknown
//   no-needs-me · no-employee · notes-unsupported · notes-fails · notes-conflict · cost-unsupported
import { orgApi } from '$lib/modules/organization/api';
import { newId } from '$lib/modules/organization/commands';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { OrgSnapshot } from '$lib/proto';
import type { WorkItemsApi } from '../../../src/lib/modules/workitems/api';
import { OUTCOME_UNKNOWN, canAllowOnce } from '../../../src/lib/modules/workitems/budget';
import { normalizeRef } from '../../../src/lib/modules/workitems/present';
import type {
	ActivityEvent,
	AssignmentConflict,
	BudgetWord,
	ChecklistStep,
	Choices,
	Comment,
	EmployeeRef,
	PrivateNote,
	ProjectChoice,
	TaskBudget,
	TaskCost,
	TaskCostAmounts,
	TaskDetail,
	TaskLink,
	TaskRow,
	TeamChoice,
	Viewer,
	WhyLink
} from '../../../src/lib/modules/workitems/types';
import { mockMode, POC_ORG_ID } from '../mock-mode';
import { pocOrgSnapshot, pocWorkItems } from '../poc-dataset';
import seedData from '../data/workitems/tasks.json' with { type: 'json' };
import projectsData from '../data/projects/projects.json' with { type: 'json' };

const on = (name: string) => mockMode().split(',').includes(name);
const fail = (code: string, message: string, extra: Record<string, unknown> = {}) =>
	Object.assign(new Error(message), { source: 'api', code, message, ...extra });

const agoHours = (hours: number) => new Date(Date.now() - hours * 3600_000).toISOString();
const nowIso = () => new Date().toISOString();

const VIEWER_EMPLOYEE: EmployeeRef = { id: 'you', name: 'Alex Kim', kind: 'person' };
const OTHER_ACTOR: EmployeeRef = { id: 'avery', name: 'Avery Stone', kind: 'person' };

interface StoredTask {
	detail: TaskDetail;
	cost: TaskCostAmounts;
	privateNotes: PrivateNote[];
}

// Per-organization in-memory store (reset on page reload).
const stores = new Map<string, Map<string, StoredTask>>();
const createdOps = new Map<string, { id: string; ref: string }>();
const budgetOps = new Map<string, { state: 'applied' | 'not_applied'; reason?: string }>();
const noteOps = new Map<string, PrivateNote | 'deleted' | 'published'>();
let nextSeq = 60;
let conflictedOnce = false;
let noteConflictedOnce = false;
let budgetUnknownOnce = false;
let createUnknownOnce = false;

function checklistCount(steps: ChecklistStep[]) {
	return {
		done: steps.filter((s) => s.done).length,
		total: steps.length,
		requiredLeft: steps.filter((s) => s.required && !s.done).length
	};
}

function budgetWord(b: TaskBudget | null): BudgetWord | null {
	if (!b) return null;
	if (b.hold || b.spent >= b.limit) return 'reached';
	if (b.spent * 5 >= b.limit * 4) return 'warned';
	return 'ok';
}

function syncDerived(entry: StoredTask) {
	entry.detail.checklist = checklistCount(entry.detail.steps);
	entry.detail.budget = budgetWord(entry.cost.budget);
	entry.detail.budgetHold = entry.cost.budget?.hold;
	if (entry.detail.bug) {
		entry.detail.awaitingVerification =
			entry.detail.bug.fixed !== null && entry.detail.status !== 'done';
	}
}

function buildSeedStore(): Map<string, StoredTask> {
	const map = new Map<string, StoredTask>();
	for (const raw of seedData.tasks) {
		const steps: ChecklistStep[] = (raw.steps ?? []).map((s) => ({
			id: s.id,
			text: s.text,
			required: s.required,
			done: s.done,
			revision: s.revision,
			...('checked' in s && s.checked
				? {
						checked: {
							at: agoHours(s.checked.agoHours),
							by: s.checked.by as EmployeeRef
						}
					}
				: {})
		}));
		const comments: Comment[] = (raw.comments ?? []).map((c) => ({
			id: c.id,
			kind: c.kind as Comment['kind'],
			author: c.author as EmployeeRef,
			at: agoHours(c.agoHours),
			body: c.body
		}));
		const activity: ActivityEvent[] = (raw.activity ?? []).map((e) => ({
			id: e.id,
			at: agoHours(e.agoHours),
			actor: (e.actor as EmployeeRef | null) ?? null,
			kind: e.kind,
			text: e.text,
			...('note' in e && e.note ? { note: e.note } : {})
		}));
		const privateNotes: PrivateNote[] = (raw.privateNotes ?? []).map((n) => ({
			id: n.id,
			author: n.author as EmployeeRef,
			body: n.body,
			createdAt: agoHours(n.createdAgoHours),
			updatedAt: agoHours(n.updatedAgoHours),
			revision: n.revision,
			mine: n.mine
		}));
		const cost: TaskCostAmounts = {
			knownCostMicroUsd: raw.cost.knownCostMicroUsd,
			knownTokens: raw.cost.knownTokens,
			records: raw.cost.records,
			unpricedRecords: raw.cost.unpricedRecords,
			...('incompleteReason' in raw.cost && raw.cost.incompleteReason
				? { incompleteReason: raw.cost.incompleteReason }
				: {}),
			includesApiEquivalent: raw.cost.includesApiEquivalent,
			byAgent: raw.cost.byAgent.map((a) => ({
				agent: a.agent as EmployeeRef,
				tokens: a.tokens,
				costMicroUsd: a.costMicroUsd,
				lastActivityAt: agoHours(a.agoHours)
			})),
			budget: raw.cost.budget ? structuredClone(raw.cost.budget as TaskBudget) : null
		};
		const bug =
			'bug' in raw && raw.bug
				? {
						report: {
							whatHappened: raw.bug.report.whatHappened,
							expected: raw.bug.report.expected,
							steps: raw.bug.report.steps,
							environment: raw.bug.report.environment,
							reported: {
								severity: raw.bug.report.reported.severity as TaskDetail['severity'],
								priority: raw.bug.report.reported.priority as TaskDetail['priority'],
								at: agoHours(raw.bug.report.reported.agoHours),
								by: raw.bug.report.reported.by as EmployeeRef
							}
						},
						severityConfirmed: raw.bug.severityConfirmed,
						suggestedVerifier: raw.bug.suggestedVerifier as EmployeeRef | null,
						suggestedTriager: raw.bug.suggestedTriager as EmployeeRef | null,
						fixed: raw.bug.fixed
							? {
									how: raw.bug.fixed.how as NonNullable<
										NonNullable<TaskDetail['bug']>['fixed']
									>['how'],
									note: raw.bug.fixed.note,
									at: agoHours(raw.bug.fixed.agoHours),
									by: raw.bug.fixed.by as EmployeeRef
								}
							: null,
						verifications: [],
						closedReason: raw.bug.closedReason,
						duplicateOf: raw.bug.duplicateOf as TaskLink | null,
						fixTasks: (raw.bug.fixTasks ?? []) as TaskLink[]
					}
				: undefined;
		const request =
			'request' in raw && raw.request
				? {
						text: raw.request.text,
						routeReason: raw.request.routeReason,
						routingSource: raw.request.routingSource as NonNullable<
							TaskDetail['request']
						>['routingSource'],
						outcome: raw.request.outcome
							? {
									...('feature' in raw.request.outcome && raw.request.outcome.feature
										? {
												feature: raw.request.outcome.feature as NonNullable<
													NonNullable<NonNullable<TaskDetail['request']>['outcome']>['feature']
												>
											}
										: {}),
									docs: structuredClone(raw.request.outcome.docs),
									draftTasks: structuredClone(raw.request.outcome.draftTasks) as NonNullable<
										NonNullable<TaskDetail['request']>['outcome']
									>['draftTasks'],
									estimate: raw.request.outcome.estimate
										? { ...raw.request.outcome.estimate }
										: undefined,
									recorded: {
										at: agoHours(raw.request.outcome.recorded.agoHours),
										by: raw.request.outcome.recorded.by as EmployeeRef
									}
								}
							: null,
						declineReason: raw.request.declineReason,
						mergedInto: raw.request.mergedInto as TaskLink | null,
						created: (raw.request.created ?? []) as TaskLink[]
					}
				: undefined;

		const scoperIsViewer = request?.outcome?.recorded.by.id === VIEWER_EMPLOYEE.id;

		const detail: TaskDetail = {
			id: raw.id,
			ref: raw.ref,
			type: raw.type as TaskDetail['type'],
			title: raw.title,
			description: raw.description,
			status: raw.status as TaskDetail['status'],
			priority: raw.priority as TaskDetail['priority'],
			severity: raw.severity as TaskDetail['severity'],
			effort: raw.effort,
			project: raw.project,
			team: raw.team,
			assignedTeam: raw.assignedTeam,
			assignee: raw.assignee as EmployeeRef | null,
			reporter: raw.reporter as EmployeeRef,
			createdAt: agoHours(raw.createdAgoHours),
			updatedAt: agoHours(raw.updatedAgoHours),
			statusAt: agoHours(raw.statusAgoHours),
			checklist: checklistCount(steps),
			blockedReason: raw.blockedReason,
			reopenCount: raw.reopenCount,
			needsMe: raw.needsMe as TaskRow['needsMe'],
			budget: raw.budget as BudgetWord | null,
			budgetHold: 'budgetHold' in raw ? (raw.budgetHold as 'stop_pending' | 'held') : undefined,
			awaitingVerification:
				'awaitingVerification' in raw ? Boolean(raw.awaitingVerification) : false,
			revision: 1,
			feature: raw.feature,
			milestone: raw.milestone,
			resolution: raw.resolution as TaskDetail['resolution'],
			latestUpdate: {
				at: agoHours(raw.updatedAgoHours),
				by: raw.latestUpdateBy as EmployeeRef
			},
			steps,
			comments,
			activity,
			output: structuredClone(raw.output) as TaskDetail['output'],
			completion: raw.completion
				? {
						at: agoHours(raw.completion.agoHours),
						by: raw.completion.by as EmployeeRef,
						text: raw.completion.text
					}
				: null,
			fixes: (raw.fixes ?? []) as TaskLink[],
			relatesTo: (raw.relatesTo ?? []) as TaskLink[],
			repositories: structuredClone(raw.repositories ?? []),
			why: structuredClone(raw.why ?? []) as WhyLink[],
			time: {
				createdAt: agoHours(raw.createdAgoHours),
				startedAt: raw.time.startedAgoHours !== null ? agoHours(raw.time.startedAgoHours) : null,
				startUnavailable: raw.time.startUnavailable,
				open: raw.time.open,
				leadSeconds: raw.time.leadSeconds,
				cycleSeconds: raw.time.cycleSeconds,
				waitingOnYouSeconds: raw.time.waitingOnYouSeconds,
				blockedSeconds: raw.time.blockedSeconds,
				...('typical' in raw.time && raw.time.typical ? { typical: raw.time.typical } : {})
			},
			...(bug ? { bug } : {}),
			...(request ? { request } : {}),
			can: {
				edit: true,
				triage: raw.type === 'bug',
				verify: raw.type === 'bug' && Boolean(bug?.fixed),
				approve: raw.type === 'request' && !scoperIsViewer,
				...(scoperIsViewer
					? { approveReason: 'The scoper cannot approve their own scoping outcome.' }
					: {})
			}
		};

		const entry: StoredTask = { detail, cost, privateNotes };
		syncDerived(entry);
		map.set(detail.id, entry);
	}
	return map;
}

const usePoc = (organizationId: string) => organizationId === POC_ORG_ID;
const STORAGE_PREFIX = 'agentic-org.mock.workitems.v1.';
const canPersist = () => {
	const mode = mockMode().trim();
	return mode === '' || mode === 'poc' || mode === 'default';
};
const entryOrg = new WeakMap<StoredTask, string>();

function tagEntries(organizationId: string, map: Map<string, StoredTask>) {
	for (const entry of map.values()) entryOrg.set(entry, organizationId);
}

function loadPersistedStore(organizationId: string): Map<string, StoredTask> | undefined {
	if (!canPersist() || typeof sessionStorage === 'undefined') return undefined;
	try {
		const raw = sessionStorage.getItem(`${STORAGE_PREFIX}${organizationId}`);
		if (!raw) return undefined;
		const parsed = JSON.parse(raw) as { nextSeq?: number; entries: [string, StoredTask][] };
		if (typeof parsed.nextSeq === 'number' && parsed.nextSeq > nextSeq) {
			nextSeq = parsed.nextSeq;
		}
		const map = new Map<string, StoredTask>(parsed.entries);
		for (const entry of map.values()) {
			const m = /^T-A(\d+)$/.exec(entry.detail.ref);
			if (m) {
				const n = Number(m[1]);
				if (n >= nextSeq) nextSeq = n + 1;
			}
		}
		tagEntries(organizationId, map);
		return map;
	} catch {
		return undefined;
	}
}

function saveStore(organizationId: string, store: Map<string, StoredTask>) {
	if (!canPersist() || typeof sessionStorage === 'undefined') return;
	try {
		sessionStorage.setItem(
			`${STORAGE_PREFIX}${organizationId}`,
			JSON.stringify({ nextSeq, entries: [...store.entries()] })
		);
	} catch {
		// ignore quota/private-mode storage errors
	}
}

function buildPocStore(): Map<string, StoredTask> {
	const map = new Map<string, StoredTask>();
	const snap = pocOrgSnapshot();
	const projects = snap.projects ?? [];
	const teams = snap.teams ?? [];
	const members = snap.members ?? [];
	const projectTeams = snap.project_teams ?? [];

	const memberRef = (id: string | undefined): EmployeeRef | null => {
		if (!id) return null;
		const m = members.find((x) => x.id === id);
		if (!m) return null;
		return { id: m.id, name: m.name, kind: m.kind };
	};
	const fallbackReporter: EmployeeRef = memberRef('ada') ?? {
		id: 'ada',
		name: 'Ada',
		kind: 'agent'
	};
	const fallbackTeam = teams[0]
		? { id: teams[0].id, name: teams[0].name }
		: { id: 'platform', name: 'Platform (Rust)' };

	const pocNeedsMe: Record<string, TaskRow['needsMe']> = {
		'w-t118': {
			source: 'owner_block',
			text: 'File-access choice waiting 25m · Build box office offline',
			inboxItemId: 'in2'
		},
		'w-t124': {
			source: 'owner_block',
			text: 'Waiting for local test access permission',
			inboxItemId: 'in3'
		},
		'w-t121': {
			source: 'approval',
			text: "Approve publishing Kai's sync update to v1.0",
			inboxItemId: 'in1'
		}
	};

	const mapStatus = (s: string): TaskDetail['status'] => {
		if (s === 'in_progress' || s === 'scoping') return 'in_progress';
		if (s === 'blocked') return 'blocked';
		if (s === 'done' || s === 'closed' || s === 'approved') return 'done';
		if (s === 'cancelled') return 'cancelled';
		return 'todo';
	};

	for (const w of pocWorkItems()) {
		const pIdx = projects.findIndex((p) => p.id === w.project_id);
		const proj = pIdx >= 0 ? projects[pIdx] : undefined;
		const ptLink = proj ? projectTeams.find((l) => l.project_id === proj.id) : undefined;
		const ownerTeamRaw = ptLink ? teams.find((t) => t.id === ptLink.team_id) : undefined;
		const ownerTeam = ownerTeamRaw
			? { id: ownerTeamRaw.id, name: ownerTeamRaw.name }
			: fallbackTeam;
		const assigneeMember = w.assignee_id ? members.find((m) => m.id === w.assignee_id) : undefined;
		const assignedTeamRaw = assigneeMember?.home_team_id
			? teams.find((t) => t.id === assigneeMember.home_team_id)
			: undefined;
		const assignedTeam = assignedTeamRaw
			? { id: assignedTeamRaw.id, name: assignedTeamRaw.name }
			: ownerTeam;
		const assignee = memberRef(w.assignee_id);
		const reporter = memberRef(w.created_by) ?? fallbackReporter;
		const status = mapStatus(w.status);
		const isClosed = status === 'done' || status === 'cancelled';
		const description =
			w.id === 'w-t118'
				? 'Implementation and unit checks complete; awaiting file-access ruling between timed access and fixed lock.'
				: w.blocked_reason
					? `${w.title} — ${w.blocked_reason}`
					: `${w.title} (${w.short_id}).`;

		const steps: ChecklistStep[] = [
			{
				id: `${w.id}-s1`,
				text: 'Implementation and unit checks complete',
				required: true,
				done: true,
				revision: 1,
				checked: { at: agoHours(2), by: assignee ?? reporter }
			},
			{
				id: `${w.id}-s2`,
				text: 'Review sign-off recorded',
				required: true,
				done: isClosed,
				revision: 1,
				...(isClosed ? { checked: { at: agoHours(1), by: reporter } } : {})
			}
		];

		const detail: TaskDetail = {
			id: w.id,
			ref: w.short_id,
			type: w.kind,
			title: w.title,
			description,
			status,
			priority: w.priority ?? 'normal',
			severity: w.kind === 'bug' ? (w.severity ?? null) : null,
			effort: null,
			project: proj ? { id: proj.id, name: proj.name, number: pIdx + 1 } : null,
			team: ownerTeam,
			assignedTeam,
			assignee,
			reporter,
			createdAt: w.created_at,
			updatedAt: w.updated_at,
			statusAt: w.updated_at,
			checklist: checklistCount(steps),
			blockedReason: status === 'blocked' ? (w.blocked_reason ?? 'Blocked') : null,
			reopenCount: w.reopen_count ?? 0,
			needsMe: pocNeedsMe[w.id] ?? null,
			budget: null,
			awaitingVerification: false,
			revision: 1,
			feature: null,
			milestone: null,
			resolution: isClosed ? (w.kind === 'bug' ? 'fixed' : 'completed') : null,
			latestUpdate: { at: w.updated_at, by: assignee ?? reporter },
			steps,
			comments: [],
			activity: [
				{
					id: `${w.id}-ev1`,
					at: w.created_at,
					actor: reporter,
					kind: 'created',
					text: `created ${w.kind === 'bug' ? 'Bug' : w.kind === 'request' ? 'Request' : 'Task'} ${w.short_id}`
				}
			],
			output: [],
			completion: isClosed
				? { at: w.updated_at, by: assignee ?? reporter, text: 'Completed and verified.' }
				: null,
			fixes: [],
			relatesTo: [],
			repositories: [],
			why: [],
			time: {
				createdAt: w.created_at,
				startedAt: status === 'todo' ? null : agoHours(3),
				startUnavailable: false,
				open: !isClosed,
				leadSeconds: 10800,
				cycleSeconds: status === 'todo' ? null : 7200,
				waitingOnYouSeconds: pocNeedsMe[w.id] ? 1500 : 0,
				blockedSeconds: status === 'blocked' ? 1500 : 0
			},
			...(w.kind === 'bug'
				? {
						bug: {
							report: {
								whatHappened: description,
								expected: 'Works reliably without regressions.',
								steps: null,
								environment: 'HQ macOS office',
								reported: {
									severity: w.severity ?? null,
									priority: w.priority ?? 'normal',
									at: w.created_at,
									by: reporter
								}
							},
							severityConfirmed: Boolean(w.severity_confirmed),
							suggestedVerifier: reporter,
							suggestedTriager: null,
							fixed: isClosed
								? {
										how: { kind: 'note' },
										note: 'Resolved and verified.',
										at: w.updated_at,
										by: assignee ?? reporter
									}
								: null,
							verifications: [],
							closedReason: null,
							duplicateOf: null,
							fixTasks: []
						}
					}
				: {}),
			...(w.kind === 'request'
				? {
						request: {
							text: description,
							routeReason: null,
							routingSource: 'person',
							outcome: null,
							declineReason: null,
							mergedInto: null,
							created: []
						}
					}
				: {}),
			can: {
				edit: true,
				triage: w.kind === 'bug',
				verify: false,
				approve: w.kind === 'request'
			}
		};

		const entry: StoredTask = {
			detail,
			cost: {
				knownCostMicroUsd: null,
				knownTokens: null,
				records: 0,
				unpricedRecords: 0,
				includesApiEquivalent: false,
				byAgent: [],
				budget: null
			},
			privateNotes: []
		};
		syncDerived(entry);
		map.set(detail.id, entry);
	}
	return map;
}

function storeFor(organizationId: string): Map<string, StoredTask> {
	const poc = usePoc(organizationId);
	const persist = canPersist();
	const key = `${organizationId}:${poc ? 'poc' : 'default'}:${persist ? 'p' : `m:${mockMode().trim()}`}`;
	let s = stores.get(key);
	if (!s) {
		s = loadPersistedStore(organizationId) ?? (poc ? buildPocStore() : buildSeedStore());
		tagEntries(organizationId, s);
		stores.set(key, s);
	}
	return s;
}

function toRow(t: TaskDetail): TaskRow {
	return {
		id: t.id,
		ref: t.ref,
		revision: t.revision,
		type: t.type,
		title: t.title,
		status: t.status,
		priority: t.priority,
		severity: t.severity,
		effort: t.effort,
		project: t.project,
		team: t.team,
		assignedTeam: t.assignedTeam,
		assignee: t.assignee,
		reporter: t.reporter,
		createdAt: t.createdAt,
		updatedAt: t.updatedAt,
		statusAt: t.statusAt,
		checklist: { ...t.checklist },
		blockedReason: t.blockedReason,
		reopenCount: t.reopenCount,
		needsMe: t.needsMe ? { ...t.needsMe } : null,
		budget: t.budget,
		budgetHold: t.budgetHold,
		awaitingVerification: t.awaitingVerification,
		...(t.request?.mergedInto ? { merged: true } : {})
	};
}

function toLink(t: TaskDetail): TaskLink {
	return { id: t.id, ref: t.ref, type: t.type, title: t.title, status: t.status };
}

function actorFor(organizationId: string): EmployeeRef {
	return viewerInfo(organizationId).employee ?? VIEWER_EMPLOYEE;
}

function pushEvent(entry: StoredTask, kind: string, text: string, note?: string) {
	const at = nowIso();
	const orgId = entryOrg.get(entry);
	const actor = orgId ? actorFor(orgId) : VIEWER_EMPLOYEE;
	entry.detail.activity.push({
		id: newId(),
		at,
		actor,
		kind,
		text,
		...(note ? { note } : {})
	});
	entry.detail.updatedAt = at;
	entry.detail.latestUpdate = { at, by: actor };
	entry.detail.revision += 1;
	if (orgId) saveStore(orgId, storeFor(orgId));
}

async function readGate() {
	await new Promise((r) => setTimeout(r, on('slow') ? 2500 : 120));
	if (on('unavailable')) await unavailableCall();
	if (on('unsupported'))
		throw fail('unsupported', "This version of the office doesn't support work items.");
	if (on('fails'))
		throw fail('internal', "The office didn't answer. Check that it is running, then try again.");
}

async function writeGate() {
	await new Promise((r) => setTimeout(r, on('slow') ? 2500 : 160));
	if (on('unavailable')) await unavailableCall();
	if (on('stale'))
		throw fail('storage_error', "Can't reach the office, so changes are paused (read-only).");
	if (on('viewer')) throw fail('not_authorized', "You don't have permission to change this item.");
}

function staleCopy() {
	return on('stale') ? { stale: { copiedAt: agoHours(1) } } : {};
}

function viewerInfo(organizationId: string): Viewer {
	if (on('no-employee')) return { employee: null, teamIds: [], owner: true };
	if (usePoc(organizationId)) {
		return {
			employee: { id: 'you', name: 'You', kind: 'person' },
			teamIds: ['leadership', 'platform', 'app', 'product', 'quality'],
			owner: !on('viewer') && !on('agent-pm')
		};
	}
	return {
		employee: VIEWER_EMPLOYEE,
		teamIds: ['eng', 'web'],
		owner: !on('viewer') && !on('agent-pm')
	};
}

function findEntry(organizationId: string, idOrRef: string): StoredTask {
	const store = storeFor(organizationId);
	const byId = store.get(idOrRef);
	if (byId) return byId;
	const norm = normalizeRef(idOrRef);
	for (const entry of store.values()) {
		if (normalizeRef(entry.detail.ref) === norm) return entry;
	}
	throw fail(
		'not_found',
		`Work item ${idOrRef} isn't in this organization. It may have been removed or the reference is wrong.`
	);
}

function teamPath(s: OrgSnapshot, teamId: string): string {
	const names: string[] = [];
	for (
		let t = s.teams.find((x) => x.id === teamId);
		t;
		t = s.teams.find((x) => x.id === t!.owner_team_id)
	) {
		names.unshift(t.name);
	}
	return names.join(' › ');
}

async function loadChoices(organizationId: string): Promise<Choices> {
	const { snapshot } = await orgApi.view(organizationId);
	const teams: TeamChoice[] = snapshot.teams
		.map((t) => ({ id: t.id, name: t.name, path: teamPath(snapshot, t.id) }))
		.sort((a, b) => a.path.localeCompare(b.path));
	const employees = snapshot.members
		.filter((m) => m.agent?.status !== 'offboarded')
		.map((m) => ({
			id: m.id,
			name: m.name,
			kind: m.kind,
			role: m.agent?.role ?? m.title_in_team,
			availability: m.agent ? 'Available' : 'Person'
		}))
		.sort((a, b) => a.name.localeCompare(b.name));
	const seeds = projectsData.projects as Record<string, { number: number; owner?: string }>;
	const projects: ProjectChoice[] = (snapshot.projects ?? []).map((p, idx) => {
		const link = (snapshot.project_teams ?? []).find((l) => l.project_id === p.id);
		const ownerId = link?.team_id ?? seeds[p.id]?.owner ?? snapshot.teams[0]?.id ?? 'eng';
		const ownerTeam = snapshot.teams.find((t) => t.id === ownerId) ?? {
			id: ownerId,
			name: 'Engineering'
		};
		return {
			id: p.id,
			name: p.name,
			number: seeds[p.id]?.number ?? idx + 1,
			ownerTeam: { id: ownerTeam.id, name: ownerTeam.name },
			archived: Boolean(p.archived)
		};
	});
	return {
		teams,
		employees,
		projects,
		organizationName: usePoc(organizationId) ? 'Agentic Org' : 'Lumen Labs'
	};
}

export const workItemsApi: WorkItemsApi = {
	async queue(organizationId, scope) {
		await readGate();
		if (on('empty')) {
			return {
				rows: [],
				viewer: viewerInfo(organizationId),
				needsMeSources: on('no-needs-me')
					? []
					: ['owner_block', 'verify', 'critical', 'budget', 'approval'],
				...staleCopy()
			};
		}
		const store = storeFor(organizationId);
		const all = [...store.values()].map((e) => {
			syncDerived(e);
			return toRow(e.detail);
		});
		const rows =
			scope.kind === 'project' ? all.filter((r) => r.project?.id === scope.projectId) : all;
		return {
			rows,
			viewer: viewerInfo(organizationId),
			needsMeSources: on('no-needs-me')
				? []
				: ['owner_block', 'review', 'handoff', 'ask', 'budget', 'verify', 'critical', 'approval'],
			...staleCopy()
		};
	},

	async task(organizationId, ref) {
		await readGate();
		const entry = findEntry(organizationId, ref);
		syncDerived(entry);
		const detail = structuredClone(entry.detail);
		if (on('viewer')) {
			detail.can = { edit: false, triage: false, verify: false, approve: false };
		}
		return { ...detail, ...staleCopy() };
	},

	async choices(organizationId) {
		await readGate();
		return loadChoices(organizationId);
	},

	async similar(organizationId, query) {
		const text = query.text.trim().toLowerCase();
		if (text.length < 3) return [];
		const words = text.split(/\s+/).filter((w) => w.length >= 3);
		if (words.length === 0) return [];
		const store = storeFor(organizationId);
		const matches: TaskRow[] = [];
		for (const entry of store.values()) {
			const t = entry.detail;
			if (t.type !== query.type) continue;
			if (t.status === 'done' || t.status === 'cancelled') continue;
			const hay = `${t.title} ${t.description}`.toLowerCase();
			if (words.some((w) => hay.includes(w))) {
				matches.push(toRow(t));
			}
		}
		return matches.slice(0, query.type === 'bug' ? 3 : 2);
	},

	async lookup(organizationId, ref) {
		await readGate();
		const entry = findEntry(organizationId, ref);
		return toLink(entry.detail);
	},

	async create(organizationId, draft) {
		const prior = createdOps.get(draft.operationId);
		if (prior) return prior;
		await writeGate();
		const choices = await loadChoices(organizationId);
		const project = draft.projectId
			? (choices.projects.find((p) => p.id === draft.projectId) ?? null)
			: null;
		const team = project ? project.ownerTeam : choices.teams.find((t) => t.id === draft.teamId);
		if (!team) throw fail('invalid_request', 'Choose an accountable team.', { field: 'team' });
		const assignedTeam = draft.assignedTeamId
			? (choices.teams.find((t) => t.id === draft.assignedTeamId) ?? null)
			: null;
		const assignee = draft.assigneeId
			? (choices.employees.find((e) => e.id === draft.assigneeId) ?? null)
			: null;

		const store = storeFor(organizationId);
		const id = newId();
		const ref = `T-A${nextSeq++}`;
		const at = nowIso();
		const fixes: TaskLink[] = [];
		if (draft.fixesTaskId && store.has(draft.fixesTaskId)) {
			const bugEntry = store.get(draft.fixesTaskId)!;
			fixes.push(toLink(bugEntry.detail));
		}

		const actor = actorFor(organizationId);
		const detail: TaskDetail = {
			id,
			ref,
			type: draft.type,
			title: draft.title.trim(),
			description: draft.description.trim(),
			status: 'todo',
			priority: draft.priority,
			severity: draft.type === 'bug' ? draft.severity : null,
			effort: null,
			project: project ? { id: project.id, name: project.name, number: project.number } : null,
			team: { id: team.id, name: team.name },
			assignedTeam: assignedTeam ? { id: assignedTeam.id, name: assignedTeam.name } : null,
			assignee: assignee ? { id: assignee.id, name: assignee.name, kind: assignee.kind } : null,
			reporter: actor,
			createdAt: at,
			updatedAt: at,
			statusAt: at,
			checklist: { done: 0, total: 0, requiredLeft: 0 },
			blockedReason: null,
			reopenCount: 0,
			needsMe:
				draft.type === 'bug' && draft.severity === 'critical' && !assignee && !assignedTeam
					? {
							source: 'critical',
							text: 'Critical bug is unassigned — stays in Needs you until assigned or closed'
						}
					: null,
			budget: null,
			revision: 1,
			feature: null,
			milestone: null,
			resolution: null,
			latestUpdate: { at, by: actor },
			steps: [],
			comments: [],
			activity: [
				{
					id: newId(),
					at,
					actor,
					kind: 'created',
					text: `created ${draft.type === 'bug' ? 'Bug' : draft.type === 'request' ? 'Request' : 'Task'} ${ref}`
				}
			],
			output: [],
			completion: null,
			fixes,
			relatesTo: [],
			repositories: [],
			why: [],
			time: {
				createdAt: at,
				startedAt: null,
				startUnavailable: false,
				open: true,
				leadSeconds: 60,
				cycleSeconds: null,
				waitingOnYouSeconds: 0,
				blockedSeconds: 0
			},
			...(draft.type === 'bug'
				? {
						bug: {
							report: {
								whatHappened: draft.bug?.whatHappened ?? draft.description,
								expected: draft.bug?.expected || null,
								steps: draft.bug?.steps || null,
								environment: draft.bug?.environment || null,
								reported: {
									severity: draft.severity,
									priority: draft.priority,
									at,
									by: actor
								}
							},
							severityConfirmed: false,
							suggestedVerifier: actor,
							suggestedTriager: null,
							fixed: null,
							verifications: [],
							closedReason: null,
							duplicateOf: null,
							fixTasks: []
						}
					}
				: {}),
			...(draft.type === 'request'
				? {
						request: {
							text: draft.description.trim() || draft.title.trim(),
							routeReason: null,
							routingSource: assignee || assignedTeam ? 'person' : null,
							outcome: null,
							declineReason: null,
							mergedInto: null,
							created: []
						}
					}
				: {}),
			can: {
				edit: true,
				triage: draft.type === 'bug',
				verify: false,
				approve: draft.type === 'request'
			}
		};

		const entry: StoredTask = {
			detail,
			cost: {
				knownCostMicroUsd: null,
				knownTokens: null,
				records: 0,
				unpricedRecords: 0,
				includesApiEquivalent: false,
				byAgent: [],
				budget: null
			},
			privateNotes: []
		};
		entryOrg.set(entry, organizationId);
		store.set(id, entry);
		saveStore(organizationId, store);

		// Derive inverse fixed_by on the bug if this is a fix task.
		if (draft.fixesTaskId && store.has(draft.fixesTaskId)) {
			const bugEntry = store.get(draft.fixesTaskId)!;
			if (bugEntry.detail.bug) {
				bugEntry.detail.bug.fixTasks.push(toLink(detail));
				pushEvent(bugEntry, 'linked', `linked fix task ${ref}`);
			}
		}

		const result = { id, ref };
		createdOps.set(draft.operationId, result);
		if (on('create-unknown') && !createUnknownOnce) {
			createUnknownOnce = true;
			throw fail(OUTCOME_UNKNOWN, 'Connection dropped before create was confirmed.');
		}
		return result;
	},

	async createOperation(_organizationId, operationId) {
		await readGate();
		const found = createdOps.get(operationId);
		return found
			? { state: 'applied', ...found }
			: { state: 'not_applied', reason: 'The office has no record of this creation attempt.' };
	},

	async addToExisting(organizationId, taskId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		entry.detail.comments.push({
			id: newId(),
			kind: 'comment',
			author: actorFor(organizationId),
			at: nowIso(),
			body: change.text
		});
		pushEvent(entry, 'comment', 'added a shared comment from duplicate report');
		return { ref: entry.detail.ref };
	},

	async setStatus(organizationId, taskId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		const t = entry.detail;
		if (change.expectedRevision !== t.revision) {
			throw fail(
				'conflict',
				'This work item changed while you were updating it. Reload and try again.'
			);
		}
		t.status = change.status;
		t.statusAt = nowIso();
		t.blockedReason = change.status === 'blocked' ? (change.blockedReason ?? 'Blocked') : null;
		if (change.status === 'in_progress' && !t.time.startedAt) {
			t.time.startedAt = t.statusAt;
		}
		if (change.status === 'done' || change.status === 'cancelled') {
			t.time.open = false;
			if (t.needsMe?.source === 'critical') t.needsMe = null;
		} else {
			t.time.open = true;
		}
		pushEvent(entry, 'status', `moved ${t.ref} to ${change.status}`, change.blockedReason);
		syncDerived(entry);
	},

	async setPriority(organizationId, taskId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		if (change.expectedRevision !== entry.detail.revision) {
			throw fail(
				'conflict',
				'This work item changed while you were updating it. Reload and try again.'
			);
		}
		entry.detail.priority = change.priority;
		pushEvent(entry, 'priority', `changed priority to ${change.priority}`);
	},

	async edit(organizationId, taskId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		entry.detail.title = change.title;
		entry.detail.description = change.description;
		pushEvent(entry, 'edited', `updated title and description of ${entry.detail.ref}`);
	},

	async assign(organizationId, taskId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		if (on('conflict') && !conflictedOnce) {
			conflictedOnce = true;
			entry.detail.revision += 1;
			const current: AssignmentConflict = {
				assignedTeam: { id: 'platform', name: 'Platform' },
				assignee: { id: 'kai', name: 'Kai Lindqvist', kind: 'agent' },
				by: OTHER_ACTOR,
				at: nowIso(),
				revision: entry.detail.revision
			};
			entry.detail.assignedTeam = current.assignedTeam;
			entry.detail.assignee = current.assignee;
			throw fail('conflict', 'Assignment changed while you were editing.', { current });
		}
		const choices = await loadChoices(organizationId);
		entry.detail.assignedTeam = change.assignedTeamId
			? (choices.teams.find((t) => t.id === change.assignedTeamId) ?? null)
			: null;
		const emp = change.assigneeId
			? (choices.employees.find((e) => e.id === change.assigneeId) ?? null)
			: null;
		entry.detail.assignee = emp ? { id: emp.id, name: emp.name, kind: emp.kind } : null;
		if (
			entry.detail.needsMe?.source === 'critical' &&
			(entry.detail.assignedTeam || entry.detail.assignee)
		) {
			entry.detail.needsMe = null;
		}
		pushEvent(entry, 'assigned', `updated assignment on ${entry.detail.ref}`);
	},

	async triage(organizationId, taskId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		if (on('conflict') && !conflictedOnce) {
			conflictedOnce = true;
			entry.detail.revision += 1;
			const current: AssignmentConflict = {
				assignedTeam: { id: 'web', name: 'Web' },
				assignee: { id: 'noor', name: 'Noor Haddad', kind: 'agent' },
				by: OTHER_ACTOR,
				at: nowIso(),
				revision: entry.detail.revision
			};
			entry.detail.assignedTeam = current.assignedTeam;
			entry.detail.assignee = current.assignee;
			throw fail('conflict', 'Bug changed while you were triaging.', { current });
		}
		const choices = await loadChoices(organizationId);
		entry.detail.severity = change.severity;
		entry.detail.priority = change.priority;
		entry.detail.assignedTeam = change.assignedTeamId
			? (choices.teams.find((t) => t.id === change.assignedTeamId) ?? null)
			: null;
		const emp = change.assigneeId
			? (choices.employees.find((e) => e.id === change.assigneeId) ?? null)
			: null;
		entry.detail.assignee = emp ? { id: emp.id, name: emp.name, kind: emp.kind } : null;
		if (entry.detail.bug) entry.detail.bug.severityConfirmed = true;
		if (
			entry.detail.needsMe?.source === 'critical' &&
			(entry.detail.assignedTeam || entry.detail.assignee)
		) {
			entry.detail.needsMe = null;
		}
		pushEvent(entry, 'triaged', `triaged ${entry.detail.ref}`);
	},

	async addSteps(organizationId, taskId, texts) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		for (const text of texts) {
			entry.detail.steps.push({
				id: newId(),
				text,
				required: false,
				done: false,
				revision: 1
			});
		}
		pushEvent(
			entry,
			'step',
			`added ${texts.length} checklist ${texts.length === 1 ? 'step' : 'steps'}`
		);
		syncDerived(entry);
	},

	async setStep(organizationId, taskId, stepId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		const step = entry.detail.steps.find((s) => s.id === stepId);
		if (!step) throw fail('not_found', 'Checklist step not found.');
		if (change.done !== undefined) {
			step.done = change.done;
			step.checked = change.done ? { at: nowIso(), by: actorFor(organizationId) } : undefined;
			pushEvent(entry, 'step', `${change.done ? 'checked' : 'unchecked'} "${step.text}"`);
		}
		if (change.required !== undefined) {
			step.required = change.required;
			pushEvent(
				entry,
				'step',
				`${change.required ? 'marked required' : 'marked optional'}: "${step.text}"`
			);
		}
		step.revision += 1;
		syncDerived(entry);
	},

	async moveStep(organizationId, taskId, stepId, toIndex) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		const from = entry.detail.steps.findIndex((s) => s.id === stepId);
		if (from < 0) throw fail('not_found', 'Checklist step not found.');
		const [item] = entry.detail.steps.splice(from, 1);
		entry.detail.steps.splice(Math.max(0, Math.min(toIndex, entry.detail.steps.length)), 0, item);
		pushEvent(entry, 'step', `reordered checklist step "${item.text}"`);
	},

	async removeStep(organizationId, taskId, stepId) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		const idx = entry.detail.steps.findIndex((s) => s.id === stepId);
		if (idx < 0) throw fail('not_found', 'Checklist step not found.');
		const [removed] = entry.detail.steps.splice(idx, 1);
		pushEvent(entry, 'step', `removed checklist step "${removed.text}"`);
		syncDerived(entry);
	},

	async comment(organizationId, taskId, body) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		entry.detail.comments.push({
			id: newId(),
			kind: 'comment',
			author: actorFor(organizationId),
			at: nowIso(),
			body
		});
		pushEvent(entry, 'comment', 'posted a shared comment');
	},

	async notes(organizationId, taskId) {
		await readGate();
		if (on('notes-unsupported'))
			throw fail('unsupported', "This version of the office doesn't support private task notes.");
		if (on('notes-fails'))
			throw fail('internal', "Couldn't reach the private notes store. Try again.");
		const entry = findEntry(organizationId, taskId);
		const readOnlyViewer = on('viewer') || on('no-employee');
		const notes = structuredClone(entry.privateNotes).map((n) => ({
			...n,
			mine: readOnlyViewer ? false : n.mine
		}));
		return {
			notes,
			canWrite: !readOnlyViewer && !on('stale')
		};
	},

	async addNote(organizationId, taskId, change) {
		const prior = noteOps.get(change.operationId);
		if (prior && typeof prior === 'object') return structuredClone(prior);
		await writeGate();
		if (on('no-employee'))
			throw fail('not_authorized', 'This account has no employee record to author private notes.');
		const entry = findEntry(organizationId, taskId);
		const at = nowIso();
		const created: PrivateNote = {
			id: newId(),
			author: actorFor(organizationId),
			body: change.body,
			createdAt: at,
			updatedAt: at,
			revision: 1,
			mine: true
		};
		entry.privateNotes.push(created);
		noteOps.set(change.operationId, structuredClone(created));
		saveStore(organizationId, storeFor(organizationId));
		// Private notes never write to shared activity (PN-01..06).
		return structuredClone(created);
	},

	async editNote(organizationId, taskId, noteId, change) {
		const prior = noteOps.get(change.operationId);
		if (prior && typeof prior === 'object') return structuredClone(prior);
		await writeGate();
		if (on('no-employee'))
			throw fail('not_authorized', 'This account has no employee record to edit private notes.');
		const entry = findEntry(organizationId, taskId);
		const note = entry.privateNotes.find((n) => n.id === noteId);
		if (!note || !note.mine) throw fail('not_authorized', 'Only the author can edit this note.');
		if (on('notes-conflict') && !noteConflictedOnce) {
			noteConflictedOnce = true;
			note.revision += 1;
			note.updatedAt = nowIso();
			throw fail(
				'conflict',
				'This note changed in another session. Reload the latest revision to save your draft.'
			);
		}
		if (change.expectedRevision !== note.revision) {
			throw fail(
				'conflict',
				'This note changed in another session. Reload the latest revision to save your draft.'
			);
		}
		note.body = change.body;
		note.updatedAt = nowIso();
		note.revision += 1;
		noteOps.set(change.operationId, structuredClone(note));
		saveStore(organizationId, storeFor(organizationId));
		return structuredClone(note);
	},

	async deleteNote(organizationId, taskId, noteId, change) {
		if (noteOps.get(change.operationId) === 'deleted') return;
		await writeGate();
		if (on('no-employee'))
			throw fail('not_authorized', 'This account has no employee record to delete private notes.');
		const entry = findEntry(organizationId, taskId);
		const idx = entry.privateNotes.findIndex((n) => n.id === noteId);
		if (idx < 0 || !entry.privateNotes[idx].mine)
			throw fail('not_authorized', 'Only the author can delete this note.');
		if (change.expectedRevision !== entry.privateNotes[idx].revision) {
			throw fail(
				'conflict',
				'This note changed in another session. Reload the latest revision before deleting.'
			);
		}
		entry.privateNotes.splice(idx, 1);
		noteOps.set(change.operationId, 'deleted');
		saveStore(organizationId, storeFor(organizationId));
	},

	async publishNote(organizationId, taskId, noteId, change) {
		if (noteOps.get(change.operationId) === 'published') return;
		await writeGate();
		if (on('no-employee'))
			throw fail('not_authorized', 'This account has no employee record to publish private notes.');
		const entry = findEntry(organizationId, taskId);
		const note = entry.privateNotes.find((n) => n.id === noteId);
		if (!note || !note.mine) throw fail('not_authorized', 'Only the author can publish this note.');
		if (change.expectedRevision !== note.revision) {
			throw fail(
				'conflict',
				'This note changed in another session. Reload the latest revision before publishing.'
			);
		}
		// Private original stays private; copy enters shared comments.
		entry.detail.comments.push({
			id: newId(),
			kind: 'comment',
			author: actorFor(organizationId),
			at: nowIso(),
			body: note.body
		});
		pushEvent(entry, 'comment', 'published a shared comment');
		noteOps.set(change.operationId, 'published');
	},

	async markFixed(organizationId, taskId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		if (!entry.detail.bug) throw fail('invalid_request', 'Only bugs can be marked as fixed.');
		let how: NonNullable<NonNullable<TaskDetail['bug']>['fixed']>['how'];
		if (change.how.kind === 'task') {
			const linked = findEntry(organizationId, change.how.taskRef);
			how = { kind: 'task', task: toLink(linked.detail) };
		} else if (change.how.kind === 'commit') {
			how = { kind: 'commit', commit: change.how.commit };
		} else {
			how = { kind: 'note' };
		}
		entry.detail.bug.fixed = {
			how,
			note: change.note || null,
			at: nowIso(),
			by: actorFor(organizationId)
		};
		entry.detail.can.verify = true;
		pushEvent(entry, 'fixed', `marked ${entry.detail.ref} as fixed`, change.note || undefined);
		syncDerived(entry);
	},

	async verify(organizationId, taskId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		if (!entry.detail.bug) throw fail('invalid_request', 'Only bugs can be verified.');
		const at = nowIso();
		entry.detail.bug.verifications.push({
			result: change.result,
			note: change.note || null,
			at,
			by: actorFor(organizationId)
		});
		if (change.result === 'still_happens') {
			entry.detail.bug.fixed = null;
			entry.detail.status = 'todo';
			entry.detail.statusAt = at;
			entry.detail.reopenCount += 1;
			entry.detail.can.verify = false;
			if (entry.detail.needsMe?.source === 'verify') entry.detail.needsMe = null;
			pushEvent(
				entry,
				'verified',
				`verified ${entry.detail.ref}: still happens (reopened)`,
				change.note
			);
		} else {
			entry.detail.can.verify = false;
			if (entry.detail.needsMe?.source === 'verify') entry.detail.needsMe = null;
			pushEvent(
				entry,
				'verified',
				`verified ${entry.detail.ref} as fixed`,
				change.note || undefined
			);
		}
		syncDerived(entry);
	},

	async close(organizationId, taskId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		const t = entry.detail;
		if (change.as === 'duplicate') {
			const orig = findEntry(organizationId, change.originalRef);
			if (orig.detail.id === t.id)
				throw fail('invalid_request', "A bug can't be a duplicate of itself.", {
					field: 'original'
				});
			t.status = 'cancelled';
			t.resolution = 'duplicate';
			if (t.bug) t.bug.duplicateOf = toLink(orig.detail);
			pushEvent(entry, 'closed', `closed ${t.ref} as Duplicate of ${orig.detail.ref}`);
		} else if (change.as === 'wont_fix' || change.as === 'cant_reproduce') {
			t.status = 'cancelled';
			t.resolution = change.as;
			if (t.bug) t.bug.closedReason = change.reason;
			pushEvent(entry, 'closed', `closed ${t.ref} as ${change.as}`, change.reason);
		} else if (change.as === 'cancelled') {
			t.status = 'cancelled';
			t.resolution = 'cancelled';
			if (t.bug && change.reason) t.bug.closedReason = change.reason;
			pushEvent(entry, 'closed', `closed ${t.ref} as Cancelled`, change.reason ?? undefined);
		} else {
			t.status = 'done';
			t.resolution = t.type === 'bug' ? 'fixed' : 'completed';
			pushEvent(entry, 'closed', `closed ${t.ref} as Done`);
		}
		t.statusAt = nowIso();
		t.time.open = false;
		t.needsMe = null;
		syncDerived(entry);
	},

	async reopen(organizationId, taskId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		entry.detail.status = 'todo';
		entry.detail.statusAt = nowIso();
		entry.detail.resolution = null;
		entry.detail.reopenCount += 1;
		entry.detail.time.open = true;
		pushEvent(entry, 'reopened', `reopened ${entry.detail.ref}`, change.note);
		syncDerived(entry);
	},

	async approve(organizationId, taskId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		const req = entry.detail.request;
		if (!req || !req.outcome) throw fail('invalid_request', 'No scoping outcome to approve.');
		if (!entry.detail.can.approve)
			throw fail(
				'not_authorized',
				entry.detail.can.approveReason ?? 'Cannot approve this request.'
			);
		const created: TaskLink[] = [];
		const store = storeFor(organizationId);
		for (const draft of change.drafts) {
			const res = await this.create(organizationId, {
				operationId: `${change.operationId}:${draft.draftId}`,
				type: 'task',
				title: draft.title,
				description: draft.description ?? `Created from Request ${entry.detail.ref}.`,
				projectId: entry.detail.project?.id ?? null,
				teamId: draft.team?.id ?? entry.detail.team.id,
				assignedTeamId: draft.team?.id ?? null,
				assigneeId: draft.assignee?.id ?? null,
				priority: draft.priority ?? 'normal',
				severity: null,
				relatesToTaskId: entry.detail.id
			});
			const made = store.get(res.id)!;
			made.detail.relatesTo.push(toLink(entry.detail));
			created.push(toLink(made.detail));
		}
		req.created = created;
		req.approved = { at: nowIso(), by: actorFor(organizationId) };
		entry.detail.status = 'done';
		entry.detail.resolution = 'completed';
		entry.detail.needsMe = null;
		pushEvent(
			entry,
			'approved',
			`approved ${entry.detail.ref} and created ${created.length} tasks`
		);
		syncDerived(entry);
		return { created };
	},

	async askForChanges(organizationId, taskId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		if (!entry.detail.request) throw fail('invalid_request', 'Not a request.');
		entry.detail.request.changesAsked = {
			at: nowIso(),
			by: actorFor(organizationId),
			comment: change.comment
		};
		entry.detail.needsMe = null;
		pushEvent(entry, 'changes_asked', `asked for changes on ${entry.detail.ref}`, change.comment);
	},

	async decline(organizationId, taskId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		if (!entry.detail.request) throw fail('invalid_request', 'Not a request.');
		entry.detail.request.declineReason = change.reason;
		entry.detail.status = 'cancelled';
		entry.detail.resolution = 'cancelled';
		entry.detail.needsMe = null;
		pushEvent(entry, 'declined', `declined ${entry.detail.ref}`, change.reason);
		syncDerived(entry);
	},

	async merge(organizationId, taskId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		const target = findEntry(organizationId, change.intoRef);
		if (target.detail.id === entry.detail.id)
			throw fail('invalid_request', "A request can't be merged into itself.");
		if (target.detail.type !== 'request')
			throw fail('invalid_request', `${target.detail.ref} is not a request.`);
		if (!entry.detail.request) throw fail('invalid_request', 'Not a request.');
		entry.detail.request.mergedInto = toLink(target.detail);
		entry.detail.status = 'cancelled';
		entry.detail.resolution = 'cancelled';
		entry.detail.needsMe = null;
		pushEvent(entry, 'merged', `merged ${entry.detail.ref} into ${target.detail.ref}`);
		syncDerived(entry);
	},

	async whyChoices() {
		await readGate();
		return structuredClone(seedData.whyChoices) as WhyLink[];
	},

	async linkWhy(organizationId, taskId, documentId) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		const doc = (seedData.whyChoices as WhyLink[]).find((d) => d.id === documentId);
		if (!doc) throw fail('not_found', 'Document not found.');
		if (!entry.detail.why.some((w) => w.id === doc.id)) {
			entry.detail.why.push(structuredClone(doc));
			pushEvent(entry, 'linked', `linked ${doc.key ?? doc.title}`);
		}
	},

	async cost(organizationId, taskId) {
		await readGate();
		if (on('cost-unsupported'))
			throw fail('unsupported', "This version of the office doesn't support task cost reads.");
		const entry = findEntry(organizationId, taskId);
		const amounts = structuredClone(entry.cost);
		if (amounts.budget) {
			if (on('budget-stop-pending') && amounts.budget.hold) {
				amounts.budget.hold = 'stop_pending';
			}
			if (on('budget-extended') && amounts.budget.hold) {
				amounts.budget.extended = true;
			}
		}
		if (on('viewer')) {
			const word = budgetWord(amounts.budget);
			return {
				scope: 'state',
				budget: word,
				...(amounts.budget?.hold ? { hold: amounts.budget.hold } : {})
			} satisfies TaskCost;
		}
		const rights = on('agent-pm')
			? { set: true, raise: false, stop: true }
			: { set: true, raise: true, stop: true };
		return { scope: 'amounts', amounts, rights } satisfies TaskCost;
	},

	async setBudget(organizationId, taskId, change) {
		await writeGate();
		if (change.limit <= 0 || !Number.isInteger(change.limit))
			throw fail('invalid_request', 'Budget limit must be a positive integer.', { field: 'limit' });
		const entry = findEntry(organizationId, taskId);
		const current = entry.cost.budget;
		if (
			on('agent-pm') &&
			current &&
			(change.unit !== current.unit || change.limit > current.limit)
		) {
			throw fail('not_authorized', 'Only a person can raise, clear or extend a budget.', {
				field: 'limit'
			});
		}
		const spent =
			change.unit === 'usd' ? (entry.cost.knownCostMicroUsd ?? 0) : (entry.cost.knownTokens ?? 0);
		const reached = spent >= change.limit;
		entry.cost.budget = {
			unit: change.unit,
			limit: change.limit,
			spent,
			complete: !entry.cost.incompleteReason,
			...(entry.cost.incompleteReason ? { incompleteReason: entry.cost.incompleteReason } : {}),
			...(reached ? { hold: 'held' } : {}),
			extended: false,
			baseLimit: change.limit,
			policyRevision: (current?.policyRevision ?? 0) + 1
		};
		if (!reached && entry.detail.needsMe?.source === 'budget') {
			entry.detail.needsMe = null;
			if (entry.detail.blockedReason === 'Budget reached') {
				entry.detail.status = 'in_progress';
				entry.detail.blockedReason = null;
			}
		}
		pushEvent(entry, 'budget', `updated task budget (${change.unit})`);
		syncDerived(entry);
		budgetOps.set(change.operationId, { state: 'applied' });
		if (on('budget-unknown') && !budgetUnknownOnce) {
			budgetUnknownOnce = true;
			throw fail(OUTCOME_UNKNOWN, 'Connection dropped before budget save was confirmed.');
		}
	},

	async budgetAction(organizationId, taskId, change) {
		await writeGate();
		const entry = findEntry(organizationId, taskId);
		const b = entry.cost.budget;
		if (!b) throw fail('invalid_request', 'This task has no budget.');
		if (change.action !== 'stop' && on('agent-pm')) {
			throw fail('not_authorized', 'Only a person can raise, clear or extend a budget.');
		}

		if (change.action === 'allow_once') {
			const check = canAllowOnce(b);
			if (!check.ok) throw fail('invalid_request', check.reason);
			b.limit = check.nextLimit;
			b.extended = true;
			b.hold = undefined;
			b.policyRevision += 1;
			if (entry.detail.needsMe?.source === 'budget') entry.detail.needsMe = null;
			if (entry.detail.blockedReason === 'Budget reached') {
				entry.detail.status = 'in_progress';
				entry.detail.blockedReason = null;
			}
			pushEvent(entry, 'budget', 'allowed 20% more once on task budget');
		} else if (change.action === 'clear') {
			entry.cost.budget = null;
			if (entry.detail.needsMe?.source === 'budget') entry.detail.needsMe = null;
			if (entry.detail.blockedReason === 'Budget reached') {
				entry.detail.status = 'in_progress';
				entry.detail.blockedReason = null;
			}
			pushEvent(entry, 'budget', 'cleared task budget');
		} else if (change.action === 'stop') {
			entry.detail.status = 'cancelled';
			entry.detail.resolution = entry.detail.type === 'bug' ? 'wont_fix' : 'cancelled';
			entry.detail.statusAt = nowIso();
			entry.detail.time.open = false;
			entry.detail.needsMe = null;
			b.hold = undefined;
			pushEvent(entry, 'closed', `stopped ${entry.detail.ref} after budget reached`, change.reason);
		}
		syncDerived(entry);
		budgetOps.set(change.operationId, { state: 'applied' });
		if (on('budget-unknown') && !budgetUnknownOnce) {
			budgetUnknownOnce = true;
			throw fail(OUTCOME_UNKNOWN, 'Connection dropped before budget action was confirmed.');
		}
	},

	async budgetOperation(_organizationId, operationId) {
		await readGate();
		const op = budgetOps.get(operationId);
		if (!op)
			return { state: 'not_applied', reason: 'The office has no record of this budget change.' };
		return op.state === 'applied'
			? { state: 'applied' }
			: { state: 'not_applied', reason: op.reason ?? 'Not applied.' };
	}
};
