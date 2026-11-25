import type { Member, OrgSnapshot, Team } from '$lib/proto';
import { isOpen } from './present';
import type {
	ActivityEvent,
	Attribution,
	BugDetails,
	ChecklistStep,
	Choices,
	Comment,
	EmployeeChoice,
	EmployeeRef,
	OutputItem,
	Priority,
	PrivateNote,
	ProjectChoice,
	ProjectRef,
	Ref,
	RequestDetails,
	Resolution,
	Severity,
	TaskDetail,
	TaskLink,
	TaskRow,
	TaskStatus,
	TaskTime,
	TaskType,
	TeamChoice,
	Viewer
} from './types';

export interface WireProject {
	id: string;
	number: number;
	name: string;
	owner_team_id: string;
	archived: boolean;
}

export interface WireTaskDetails {
	definition_of_done?: string;
	what_happened?: string;
	expected?: string;
	steps?: string;
	environment?: string;
	severity_confirmed?: boolean;
	reopen_count?: number;
	duplicate_of_task_id?: string;
	closed_reason?: string;
	request_text?: string;
	route_reason?: string;
	routing_source?: string;
	merged_into_task_id?: string;
	suggested_triager_id?: string;
	suggested_verifier_id?: string;
}

export interface WireTaskView {
	id: string;
	key: string;
	revision: number;
	team_id: string;
	assigned_team_id?: string | null;
	project_id?: string | null;
	type: string;
	title: string;
	status: string;
	assignee_id?: string | null;
	body_markdown?: string;
	priority: string;
	effort?: number | null;
	severity?: string | null;
	resolution?: string | null;
	blocked_reason?: string | null;
	reference_files?: string[];
	modified_files?: string[];
	details?: WireTaskDetails | null;
	created_at?: string | null;
	created_by_kind?: string;
	created_by_id?: string | null;
	updated_at?: string | null;
	updated_by_kind?: string;
	updated_by_id?: string | null;
	checklist_total?: number;
	checklist_done?: number;
}

export interface WireChecklistStep {
	id: string;
	position: number;
	text: string;
	required: boolean;
	done: boolean;
	linked_task_id?: string | null;
	checked_by_employee_id?: string | null;
	checked_at?: string | null;
	revision: number;
}

export interface WireTaskComment {
	id: string;
	position: number;
	author_id: string;
	created_at: string;
	body_markdown: string;
}

export interface WireTaskRefSummary {
	id: string;
	key: string;
	title: string;
	type: string;
	status: string;
}

export interface WireTaskEvent {
	task_id: string;
	revision: number;
	occurred_at?: string | null;
	actor_kind?: string;
	actor_id?: string | null;
	change?: Record<string, unknown>;
}

export interface WireTaskInspect {
	task: WireTaskView;
	checklist?: WireChecklistStep[];
	comments?: WireTaskComment[];
	depends_on_tasks?: WireTaskRefSummary[];
	dependent_tasks?: WireTaskRefSummary[];
	history?: WireTaskEvent[];
}

export interface WirePrivateNote {
	id: string;
	task_id: string;
	author_id: string;
	revision: number;
	body_markdown: string;
	created_at: string;
	updated_at: string;
	editable: boolean;
}

export interface OrgLookup {
	snapshot: OrgSnapshot;
	teamsById: ReadonlyMap<string, Team>;
	membersById: ReadonlyMap<string, Member>;
	projectsById: ReadonlyMap<string, WireProject>;
}

const VALID_TYPES = new Set<TaskType>(['task', 'bug', 'request']);
const VALID_STATUSES = new Set<TaskStatus>(['todo', 'in_progress', 'blocked', 'cancelled', 'done']);
const VALID_PRIORITIES = new Set<Priority>(['urgent', 'high', 'normal', 'low']);
const VALID_SEVERITIES = new Set<Severity>(['critical', 'major', 'minor', 'trivial']);
const VALID_RESOLUTIONS = new Set<Resolution>([
	'fixed',
	'duplicate',
	'wont_fix',
	'cant_reproduce',
	'completed',
	'cancelled'
]);

export function buildOrgLookup(snapshot: OrgSnapshot, projects: WireProject[]): OrgLookup {
	return {
		snapshot,
		teamsById: new Map(snapshot.teams.map((t) => [t.id, t])),
		membersById: new Map(snapshot.members.map((m) => [m.id, m])),
		projectsById: new Map(projects.map((p) => [p.id, p]))
	};
}

export function mapTaskType(raw: string | undefined): TaskType {
	return raw && VALID_TYPES.has(raw as TaskType) ? (raw as TaskType) : 'task';
}

export function mapTaskStatus(raw: string | undefined): TaskStatus {
	return raw && VALID_STATUSES.has(raw as TaskStatus) ? (raw as TaskStatus) : 'todo';
}

export function mapPriority(raw: string | undefined): Priority {
	return raw && VALID_PRIORITIES.has(raw as Priority) ? (raw as Priority) : 'normal';
}

export function mapSeverity(raw: string | null | undefined): Severity | null {
	return raw && VALID_SEVERITIES.has(raw as Severity) ? (raw as Severity) : null;
}

export function mapResolution(raw: string | null | undefined): Resolution | null {
	return raw && VALID_RESOLUTIONS.has(raw as Resolution) ? (raw as Resolution) : null;
}

export function mapEmployeeRef(
	id: string | null | undefined,
	membersById: ReadonlyMap<string, Member>,
	fallbackKind?: string
): EmployeeRef {
	if (id) {
		const found = membersById.get(id);
		if (found) {
			return {
				id: found.id,
				name: found.name,
				kind: found.kind === 'agent' ? 'agent' : 'person',
				...(found.agent?.status === 'offboarded' ? { former: true } : {})
			};
		}
	}
	return {
		id: id || 'system',
		name: id || 'System',
		kind: fallbackKind === 'agent' ? 'agent' : 'person'
	};
}

function mapTeamRef(id: string, teamsById: ReadonlyMap<string, Team>): Ref {
	const team = teamsById.get(id);
	return { id, name: team ? team.name : id };
}

function mapProjectRef(
	id: string | null | undefined,
	projectsById: ReadonlyMap<string, WireProject>
): ProjectRef | null {
	if (!id) return null;
	const project = projectsById.get(id);
	return project ? { id: project.id, name: project.name, number: project.number } : null;
}

function teamPath(team: Team, byId: ReadonlyMap<string, Team>): string {
	const names: string[] = [];
	const seen = new Set<string>();
	let current: Team | undefined = team;
	while (current && !seen.has(current.id)) {
		seen.add(current.id);
		names.unshift(current.name);
		current = current.owner_team_id ? byId.get(current.owner_team_id) : undefined;
	}
	return names.join(' › ');
}

function memberAvailability(member: Member): string {
	if (member.kind === 'person') return 'Available';
	if (member.agent?.status === 'paused') return 'Paused';
	return 'Available';
}

export function buildViewer(
	lookup: OrgLookup,
	context: { employeeId?: string; isAdmin: boolean } | undefined
): Viewer {
	const member = context?.employeeId ? lookup.membersById.get(context.employeeId) : undefined;
	if (!member) {
		return { employee: null, teamIds: [], owner: Boolean(context?.isAdmin) };
	}
	const teamIds = new Set<string>();
	if (member.home_team_id) teamIds.add(member.home_team_id);
	for (const t of lookup.snapshot.teams) {
		if (t.lead_member_id === member.id) teamIds.add(t.id);
	}
	return {
		employee: mapEmployeeRef(member.id, lookup.membersById),
		teamIds: [...teamIds],
		owner: Boolean(member.owner || context?.isAdmin)
	};
}

export function buildChoices(lookup: OrgLookup, projects: WireProject[]): Choices {
	const teams: TeamChoice[] = lookup.snapshot.teams
		.map((t) => ({ id: t.id, name: t.name, path: teamPath(t, lookup.teamsById) }))
		.sort((a, b) => a.path.localeCompare(b.path));
	const employees: EmployeeChoice[] = lookup.snapshot.members
		.filter((m) => m.agent?.status !== 'offboarded')
		.map((m) => ({
			id: m.id,
			name: m.name,
			kind: m.kind === 'agent' ? ('agent' as const) : ('person' as const),
			...(m.agent?.role ? { role: m.agent.role } : {}),
			availability: memberAvailability(m)
		}))
		.sort((a, b) => a.name.localeCompare(b.name));
	const projectChoices: ProjectChoice[] = projects
		.map((p) => ({
			id: p.id,
			name: p.name,
			number: p.number,
			ownerTeam: mapTeamRef(p.owner_team_id, lookup.teamsById),
			archived: p.archived
		}))
		.sort((a, b) => a.name.localeCompare(b.name));
	const rootTeams = lookup.snapshot.teams.filter((t) => !t.owner_team_id);
	const organizationName = rootTeams.length === 1 ? rootTeams[0].name : '';
	return {
		teams,
		employees,
		projects: projectChoices,
		organizationName
	};
}

export function mapTaskRow(wire: WireTaskView, lookup: OrgLookup, requiredLeft = 0): TaskRow {
	const createdAt = wire.created_at ?? new Date().toISOString();
	const updatedAt = wire.updated_at ?? createdAt;
	return {
		id: wire.id,
		ref: wire.key,
		revision: wire.revision,
		type: mapTaskType(wire.type),
		title: wire.title,
		status: mapTaskStatus(wire.status),
		priority: mapPriority(wire.priority),
		severity: mapSeverity(wire.severity),
		effort: wire.effort ?? null,
		project: mapProjectRef(wire.project_id, lookup.projectsById),
		team: mapTeamRef(wire.team_id, lookup.teamsById),
		assignedTeam: wire.assigned_team_id
			? mapTeamRef(wire.assigned_team_id, lookup.teamsById)
			: null,
		assignee: wire.assignee_id ? mapEmployeeRef(wire.assignee_id, lookup.membersById) : null,
		reporter: mapEmployeeRef(wire.created_by_id, lookup.membersById, wire.created_by_kind),
		createdAt,
		updatedAt,
		statusAt: updatedAt,
		checklist: {
			done: wire.checklist_done ?? 0,
			total: wire.checklist_total ?? 0,
			requiredLeft
		},
		blockedReason: wire.blocked_reason ?? null,
		reopenCount: wire.details?.reopen_count ?? 0,
		needsMe: null,
		budget: null
	};
}

export function mapTaskLink(raw: WireTaskRefSummary): TaskLink {
	return {
		id: raw.id,
		ref: raw.key,
		type: mapTaskType(raw.type),
		title: raw.title,
		status: mapTaskStatus(raw.status)
	};
}

function mapChecklistSteps(
	raw: WireChecklistStep[] | undefined,
	lookup: OrgLookup
): ChecklistStep[] {
	return [...(raw ?? [])]
		.sort((a, b) => a.position - b.position)
		.map((s) => ({
			id: s.id,
			text: s.text,
			required: s.required,
			done: s.done,
			revision: s.revision,
			...(s.done && s.checked_at && s.checked_by_employee_id
				? {
						checked: {
							at: s.checked_at,
							by: mapEmployeeRef(s.checked_by_employee_id, lookup.membersById)
						}
					}
				: {})
		}));
}

function mapComments(raw: WireTaskComment[] | undefined, lookup: OrgLookup): Comment[] {
	return [...(raw ?? [])]
		.sort((a, b) => a.position - b.position)
		.map((c) => ({
			id: c.id,
			kind: 'comment' as const,
			author: mapEmployeeRef(c.author_id, lookup.membersById),
			at: c.created_at,
			body: c.body_markdown
		}));
}

const EVENT_TEXT_MAP: Record<string, [string, string]> = {
	created: ['created', 'created this work item'],
	updated: ['edited', 'updated task fields'],
	transitioned: ['status', 'changed status'],
	assigned: ['assigned', 'updated assignment'],
	commented: ['comment', 'posted a shared comment'],
	checklist_added: ['step', 'added a checklist step'],
	checklist_checked: ['step', 'updated a checklist step'],
	checklist_edited: ['step', 'edited a checklist step'],
	checklist_removed: ['step', 'removed a checklist step'],
	checklist_moved: ['step', 'reordered checklist steps']
};

function describeEvent(change: Record<string, unknown> | undefined): {
	kind: string;
	text: string;
	note?: string;
} {
	const rawKind = typeof change?.kind === 'string' ? change.kind : 'unknown';
	const mapped = EVENT_TEXT_MAP[rawKind];
	const note = typeof change?.note === 'string' && change.note ? change.note : undefined;
	if (!mapped) return { kind: rawKind, text: 'updated this item', ...(note ? { note } : {}) };
	const [kind, baseText] = mapped;
	if (rawKind === 'transitioned' && typeof change?.to === 'string') {
		return {
			kind,
			text: `moved status to ${change.to.replace('_', ' ')}`,
			...(note ? { note } : {})
		};
	}
	return { kind, text: baseText, ...(note ? { note } : {}) };
}

function mapActivity(raw: WireTaskEvent[] | undefined, lookup: OrgLookup): ActivityEvent[] {
	return [...(raw ?? [])]
		.sort((a, b) => b.revision - a.revision)
		.map((ev) => {
			const desc = describeEvent(ev.change);
			return {
				id: `${ev.task_id}:${ev.revision}`,
				at: ev.occurred_at ?? new Date().toISOString(),
				actor: ev.actor_id ? mapEmployeeRef(ev.actor_id, lookup.membersById, ev.actor_kind) : null,
				...desc
			};
		});
}

function mapOutputFiles(task: WireTaskView): OutputItem[] {
	const paths = [...new Set([...(task.modified_files ?? []), ...(task.reference_files ?? [])])];
	return paths.map((p, idx) => ({
		id: `file-${idx}-${p}`,
		kind: 'file' as const,
		label: p
	}));
}

function mapTaskTime(row: TaskRow): TaskTime {
	const createdMs = Date.parse(row.createdAt);
	const endMs = isOpen(row.status) ? Date.now() : Date.parse(row.updatedAt);
	const leadSeconds =
		Number.isFinite(createdMs) && Number.isFinite(endMs)
			? Math.max(0, Math.floor((endMs - createdMs) / 1000))
			: null;
	return {
		createdAt: row.createdAt,
		startedAt: null,
		startUnavailable: false,
		open: isOpen(row.status),
		leadSeconds,
		cycleSeconds: null,
		waitingOnYouSeconds: 0,
		blockedSeconds: 0
	};
}

function mapBugDetails(
	wire: WireTaskView,
	row: TaskRow,
	lookup: OrgLookup,
	dependentTasks: TaskLink[]
): BugDetails {
	const d = wire.details ?? {};
	return {
		report: {
			whatHappened: d.what_happened || wire.body_markdown || '',
			expected: d.expected ?? null,
			steps: d.steps ?? null,
			environment: d.environment ?? null,
			reported: {
				severity: row.severity,
				priority: row.priority,
				at: row.createdAt,
				by: row.reporter
			}
		},
		severityConfirmed: Boolean(d.severity_confirmed),
		suggestedVerifier: d.suggested_verifier_id
			? mapEmployeeRef(d.suggested_verifier_id, lookup.membersById)
			: null,
		suggestedTriager: d.suggested_triager_id
			? mapEmployeeRef(d.suggested_triager_id, lookup.membersById)
			: null,
		fixed: null,
		verifications: [],
		closedReason: d.closed_reason ?? null,
		duplicateOf: null,
		fixTasks: dependentTasks
	};
}

function mapRequestDetails(wire: WireTaskView): RequestDetails {
	const d = wire.details ?? {};
	const src = d.routing_source;
	const routingSource = src === 'person' || src === 'rule' || src === 'copilot' ? src : null;
	return {
		text: d.request_text || wire.body_markdown || '',
		routeReason: d.route_reason ?? null,
		routingSource,
		outcome: null,
		declineReason: d.closed_reason ?? null,
		mergedInto: null,
		created: []
	};
}

export function mapTaskDetail(
	inspect: WireTaskInspect,
	lookup: OrgLookup,
	viewer: Viewer
): TaskDetail {
	const steps = mapChecklistSteps(inspect.checklist, lookup);
	const requiredLeft = steps.filter((s) => s.required && !s.done).length;
	const row = mapTaskRow(inspect.task, lookup, requiredLeft);
	row.checklist = {
		done: steps.filter((s) => s.done).length,
		total: steps.length,
		requiredLeft
	};
	const latestUpdate: Attribution = {
		at: row.updatedAt,
		by: mapEmployeeRef(
			inspect.task.updated_by_id ?? inspect.task.created_by_id,
			lookup.membersById,
			inspect.task.updated_by_kind ?? inspect.task.created_by_kind
		)
	};
	const dependsOn = (inspect.depends_on_tasks ?? []).map(mapTaskLink);
	const dependents = (inspect.dependent_tasks ?? []).map(mapTaskLink);
	const canEdit = Boolean(viewer.employee);
	return {
		...row,
		description: inspect.task.body_markdown ?? '',
		feature: null,
		milestone: null,
		resolution: mapResolution(inspect.task.resolution),
		latestUpdate,
		steps,
		comments: mapComments(inspect.comments, lookup),
		activity: mapActivity(inspect.history, lookup),
		output: mapOutputFiles(inspect.task),
		completion: null,
		fixes: dependsOn.filter((t) => t.type === 'bug'),
		relatesTo: [...dependsOn.filter((t) => t.type !== 'bug'), ...dependents],
		repositories: [],
		why: [],
		time: mapTaskTime(row),
		...(row.type === 'bug' ? { bug: mapBugDetails(inspect.task, row, lookup, dependents) } : {}),
		...(row.type === 'request' ? { request: mapRequestDetails(inspect.task) } : {}),
		can: {
			edit: canEdit,
			triage: canEdit,
			verify: canEdit,
			approve: false,
			approveReason: 'Request approval workflow is not served on this office yet.'
		}
	};
}

export function mapPrivateNote(wire: WirePrivateNote, lookup: OrgLookup): PrivateNote {
	return {
		id: wire.id,
		author: mapEmployeeRef(wire.author_id, lookup.membersById),
		body: wire.body_markdown,
		createdAt: wire.created_at,
		updatedAt: wire.updated_at,
		revision: wire.revision,
		mine: wire.editable
	};
}
