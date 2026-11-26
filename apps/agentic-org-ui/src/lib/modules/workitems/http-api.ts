import type { AppError } from '$lib/proto';
import {
	gatewayStatusCode,
	readGatewayAppError,
	unwrapGatewayResult,
	type GatewayHttpTransport,
	type GatewayReply
} from '$lib/platform/http/gateway-errors';
import { formatUuidV7 } from '$lib/platform/http/clock';
import { readOperationId } from '$lib/platform/http/wire';
import { unavailableCall } from '$lib/platform/unavailable-api';
import type { OrgApi } from '$lib/modules/organization/api';
import { isOpen, normalizeRef } from './present';
import {
	buildChoices,
	buildOrgLookup,
	buildViewer,
	mapPrivateNote,
	mapTaskDetail,
	mapTaskLink,
	mapTaskRow,
	type OrgLookup,
	type WirePrivateNote,
	type WireProject,
	type WireTaskInspect,
	type WireTaskView
} from './wire-map';
import type { WorkItemsApi } from './api';
import type {
	AssignmentChange,
	Choices,
	CloseAs,
	FixHow,
	NewTask,
	Priority,
	PrivateNote,
	PrivateNotesFeed,
	Severity,
	TaskDetail,
	TaskField,
	TaskLink,
	TaskRow,
	TaskStatus,
	WorkQueue,
	WorkScope
} from './types';

function defaultUuidV7(): string {
	return formatUuidV7(Date.now(), globalThis.crypto.getRandomValues(new Uint8Array(10)));
}

function normalizeUuidV7(candidate: string, fallback: () => string): string {
	try {
		return readOperationId(candidate);
	} catch {
		return readOperationId(fallback());
	}
}

function inferTaskField(error: AppError): TaskField | undefined {
	const msg = error.message.toLowerCase();
	if (msg.includes('title')) return 'title';
	if (msg.includes('team')) return 'team';
	if (msg.includes('project')) return 'project';
	if (msg.includes('blocked')) return 'reason';
	if (msg.includes('step')) return 'step';
	return undefined;
}

function isWireProject(v: unknown): v is WireProject {
	if (!v || typeof v !== 'object') return false;
	const p = v as Partial<WireProject>;
	return (
		typeof p.id === 'string' &&
		typeof p.number === 'number' &&
		typeof p.name === 'string' &&
		typeof p.owner_team_id === 'string' &&
		typeof p.archived === 'boolean'
	);
}

function isWireTaskView(v: unknown): v is WireTaskView {
	if (!v || typeof v !== 'object') return false;
	const t = v as Partial<WireTaskView>;
	return (
		typeof t.id === 'string' &&
		typeof t.key === 'string' &&
		typeof t.revision === 'number' &&
		typeof t.team_id === 'string' &&
		typeof t.title === 'string' &&
		typeof t.status === 'string'
	);
}

function isWirePrivateNote(v: unknown): v is WirePrivateNote {
	if (!v || typeof v !== 'object') return false;
	const n = v as Partial<WirePrivateNote>;
	return (
		typeof n.id === 'string' &&
		typeof n.task_id === 'string' &&
		typeof n.author_id === 'string' &&
		typeof n.revision === 'number' &&
		typeof n.body_markdown === 'string' &&
		typeof n.created_at === 'string' &&
		typeof n.updated_at === 'string' &&
		typeof n.editable === 'boolean'
	);
}

interface PendingIds {
	commandId: string;
	entityId: string;
}

/**
 * Production HTTP adapter for `/api/v1/organizations/{org}/tasks*` and private task notes.
 * Unserved endpoints (cost/budget, why links, request approval/merge) fail closed via `unavailableCall`.
 */
export function createHttpWorkItemsApi({
	http,
	orgApi,
	viewerContext,
	newId = defaultUuidV7
}: {
	http: GatewayHttpTransport;
	orgApi: Pick<OrgApi, 'view'>;
	viewerContext: (organizationId: string) => { employeeId?: string; isAdmin: boolean } | undefined;
	newId?: () => string;
}): WorkItemsApi {
	const pendingOps = new Map<string, PendingIds>();

	function resolveIds(operationId: string): PendingIds {
		const existing = pendingOps.get(operationId);
		if (existing) return existing;
		const created: PendingIds = {
			commandId: normalizeUuidV7(operationId, newId),
			entityId: normalizeUuidV7('', newId)
		};
		pendingOps.set(operationId, created);
		return created;
	}

	function orgBase(orgId: string): string {
		return `/api/v1/organizations/${encodeURIComponent(orgId)}`;
	}

	async function readJson(path: string, fallbackMessage: string): Promise<Record<string, unknown>> {
		let reply: GatewayReply;
		try {
			reply = await http.read(path);
		} catch {
			throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies AppError;
		}
		return unwrapGatewayResult(reply, fallbackMessage);
	}

	async function postJson(
		path: string,
		body: Record<string, unknown>,
		fallbackMessage: string
	): Promise<Record<string, unknown>> {
		let reply: GatewayReply;
		try {
			reply = await http.submit(path, body);
		} catch {
			throw { source: 'api', code: 'unavailable', message: fallbackMessage } satisfies AppError;
		}
		if (reply.status !== 200) {
			const err = readGatewayAppError(reply.body, gatewayStatusCode(reply.status), fallbackMessage);
			const field = inferTaskField(err);
			throw field ? { ...err, field } : err;
		}
		return unwrapGatewayResult(reply, fallbackMessage);
	}

	async function readProjects(organizationId: string): Promise<WireProject[]> {
		try {
			const res = await readJson(
				`${orgBase(organizationId)}/projects?include_archived=true`,
				'Projects could not be loaded.'
			);
			return Array.isArray(res.projects) ? res.projects.filter(isWireProject) : [];
		} catch {
			return [];
		}
	}

	async function loadLookup(organizationId: string): Promise<{
		lookup: OrgLookup;
		projects: WireProject[];
	}> {
		const [view, projects] = await Promise.all([
			orgApi.view(organizationId),
			readProjects(organizationId)
		]);
		return { lookup: buildOrgLookup(view.snapshot, projects), projects };
	}

	async function readTasks(
		organizationId: string,
		params?: URLSearchParams
	): Promise<WireTaskView[]> {
		const qs = params?.toString();
		const res = await readJson(
			`${orgBase(organizationId)}/tasks${qs ? `?${qs}` : ''}`,
			'Work items could not be loaded.'
		);
		if (!Array.isArray(res.tasks)) {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'The tasks response could not be read.'
			} satisfies AppError;
		}
		return res.tasks.filter(isWireTaskView);
	}

	async function inspectRaw(organizationId: string, refOrId: string): Promise<WireTaskInspect> {
		const res = await readJson(
			`${orgBase(organizationId)}/tasks/${encodeURIComponent(refOrId)}`,
			'The work item could not be loaded.'
		);
		if (!isWireTaskView(res.task)) {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'The work item could not be read.'
			} satisfies AppError;
		}
		return res as unknown as WireTaskInspect;
	}

	function buildCreateDetails(draft: NewTask): Record<string, unknown> | undefined {
		if (draft.type === 'bug' && draft.bug) {
			return {
				what_happened: draft.bug.whatHappened || undefined,
				expected: draft.bug.expected || undefined,
				steps: draft.bug.steps || undefined,
				environment: draft.bug.environment || undefined,
				severity_confirmed: false,
				reopen_count: 0
			};
		}
		if (draft.type === 'request') {
			return {
				request_text: draft.description || undefined,
				severity_confirmed: false,
				reopen_count: 0
			};
		}
		return undefined;
	}

	function buildCreateBody(draft: NewTask, ids: PendingIds): Record<string, unknown> {
		const dependsOn = [draft.fixesTaskId, draft.relatesToTaskId].filter(
			(v): v is string => typeof v === 'string' && v.length > 0
		);
		const details = buildCreateDetails(draft);
		const body: Record<string, unknown> = {
			command_id: ids.commandId,
			task_id: ids.entityId,
			type: draft.type,
			title: draft.title,
			body_markdown: draft.description,
			priority: draft.priority
		};
		if (draft.projectId) body.project_id = draft.projectId;
		else body.team_id = draft.teamId;
		if (draft.assignedTeamId) body.assigned_team_id = draft.assignedTeamId;
		if (draft.assigneeId) body.assignee_id = draft.assigneeId;
		if (draft.severity) body.severity = draft.severity;
		if (dependsOn.length > 0) body.depends_on = dependsOn;
		if (details) body.details = details;
		return body;
	}

	function buildClosePayload(change: CloseAs): {
		status: TaskStatus;
		resolution: string;
		note?: string;
	} {
		if (change.as === 'done') return { status: 'done', resolution: 'completed' };
		if (change.as === 'duplicate') {
			return {
				status: 'cancelled',
				resolution: 'duplicate',
				note: `Duplicate of ${normalizeRef(change.originalRef)}`
			};
		}
		if (change.as === 'wont_fix' || change.as === 'cant_reproduce') {
			return { status: 'cancelled', resolution: change.as, note: change.reason };
		}
		return {
			status: 'cancelled',
			resolution: 'cancelled',
			...(change.reason ? { note: change.reason } : {})
		};
	}

	return Object.freeze<WorkItemsApi>({
		async queue(organizationId: string, scope: WorkScope): Promise<WorkQueue> {
			const params = new URLSearchParams();
			if (scope.kind === 'project') params.set('project_id', scope.projectId);
			const [tasks, { lookup }] = await Promise.all([
				readTasks(organizationId, params),
				loadLookup(organizationId)
			]);
			return {
				rows: tasks.map((t) => mapTaskRow(t, lookup)),
				viewer: buildViewer(lookup, viewerContext(organizationId)),
				needsMeSources: []
			};
		},

		async task(organizationId: string, ref: string): Promise<TaskDetail> {
			const [inspect, { lookup }] = await Promise.all([
				inspectRaw(organizationId, ref),
				loadLookup(organizationId)
			]);
			return mapTaskDetail(inspect, lookup, buildViewer(lookup, viewerContext(organizationId)));
		},

		async choices(organizationId: string): Promise<Choices> {
			const { lookup, projects } = await loadLookup(organizationId);
			return buildChoices(lookup, projects);
		},

		async similar(organizationId, query): Promise<TaskRow[]> {
			const params = new URLSearchParams({ type: query.type, q: query.text, limit: '3' });
			if (query.projectId) params.set('project_id', query.projectId);
			else if (query.teamId) params.set('team_id', query.teamId);
			const [tasks, { lookup }] = await Promise.all([
				readTasks(organizationId, params),
				loadLookup(organizationId)
			]);
			return tasks
				.map((t) => mapTaskRow(t, lookup))
				.filter((r) => isOpen(r.status))
				.slice(0, 3);
		},

		async lookup(organizationId: string, ref: string): Promise<TaskLink> {
			const inspect = await inspectRaw(organizationId, normalizeRef(ref));
			return mapTaskLink({
				id: inspect.task.id,
				key: inspect.task.key,
				title: inspect.task.title,
				type: inspect.task.type,
				status: inspect.task.status
			});
		},

		async create(organizationId: string, draft: NewTask): Promise<{ id: string; ref: string }> {
			const ids = resolveIds(draft.operationId);
			const res = await postJson(
				`${orgBase(organizationId)}/tasks`,
				buildCreateBody(draft, ids),
				'The work item could not be created.'
			);
			pendingOps.delete(draft.operationId);
			if (!isWireTaskView(res.task)) {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'The created work item could not be read.'
				} satisfies AppError;
			}
			return { id: res.task.id, ref: res.task.key };
		},

		createOperation: unavailableCall,

		async addToExisting(organizationId, taskId, change): Promise<{ ref: string }> {
			const ids = resolveIds(change.operationId);
			const res = await postJson(
				`${orgBase(organizationId)}/tasks/comment`,
				{
					command_id: ids.commandId,
					task: taskId,
					comment_id: ids.entityId,
					body_markdown: change.text
				},
				'Could not add to the existing work item.'
			);
			pendingOps.delete(change.operationId);
			const key = isWireTaskView(res.task) ? res.task.key : taskId;
			return { ref: key };
		},

		async setStatus(organizationId, taskId, change): Promise<void> {
			await postJson(
				`${orgBase(organizationId)}/tasks/transition`,
				{
					command_id: normalizeUuidV7('', newId),
					task: taskId,
					expected_revision: change.expectedRevision,
					status: change.status,
					...(change.blockedReason ? { blocked_reason: change.blockedReason } : {})
				},
				'Could not change status.'
			);
		},

		async setPriority(organizationId, taskId, change): Promise<void> {
			await postJson(
				`${orgBase(organizationId)}/tasks/update`,
				{
					command_id: normalizeUuidV7('', newId),
					task: taskId,
					expected_revision: change.expectedRevision,
					priority: change.priority
				},
				'Could not update priority.'
			);
		},

		async edit(organizationId, taskId, change): Promise<void> {
			await postJson(
				`${orgBase(organizationId)}/tasks/update`,
				{
					command_id: normalizeUuidV7('', newId),
					task: taskId,
					expected_revision: change.expectedRevision,
					title: change.title,
					body_markdown: change.description
				},
				'Could not save changes.'
			);
		},

		async assign(organizationId, taskId, change: AssignmentChange): Promise<void> {
			await postJson(
				`${orgBase(organizationId)}/tasks/assign`,
				{
					command_id: normalizeUuidV7('', newId),
					task: taskId,
					expected_revision: change.expectedRevision,
					assigned_team_id: change.assignedTeamId,
					assignee_id: change.assigneeId
				},
				'Could not update assignment.'
			);
		},

		async triage(
			organizationId,
			taskId,
			change: AssignmentChange & { severity: Severity | null; priority: Priority }
		): Promise<void> {
			const updated = await postJson(
				`${orgBase(organizationId)}/tasks/update`,
				{
					command_id: normalizeUuidV7('', newId),
					task: taskId,
					expected_revision: change.expectedRevision,
					priority: change.priority,
					severity: change.severity
				},
				'Could not triage bug.'
			);
			const nextRev = isWireTaskView(updated.task)
				? updated.task.revision
				: change.expectedRevision + 1;
			await postJson(
				`${orgBase(organizationId)}/tasks/assign`,
				{
					command_id: normalizeUuidV7('', newId),
					task: taskId,
					expected_revision: nextRev,
					assigned_team_id: change.assignedTeamId,
					assignee_id: change.assigneeId
				},
				'Could not update assignment during triage.'
			);
		},

		async addSteps(organizationId, taskId, texts): Promise<void> {
			for (const text of texts) {
				await postJson(
					`${orgBase(organizationId)}/tasks/checklist/add`,
					{
						command_id: normalizeUuidV7('', newId),
						task: taskId,
						step_id: normalizeUuidV7('', newId),
						text,
						required: false
					},
					'Could not add checklist step.'
				);
			}
		},

		async setStep(organizationId, taskId, stepId, change): Promise<void> {
			if (change.done !== undefined) {
				await postJson(
					`${orgBase(organizationId)}/tasks/checklist/check`,
					{
						command_id: normalizeUuidV7('', newId),
						task: taskId,
						step_id: stepId,
						done: change.done
					},
					'Could not update checklist step.'
				);
			}
			if (change.required !== undefined) {
				await postJson(
					`${orgBase(organizationId)}/tasks/checklist/edit`,
					{
						command_id: normalizeUuidV7('', newId),
						task: taskId,
						step_id: stepId,
						required: change.required
					},
					'Could not update checklist step.'
				);
			}
		},

		async moveStep(organizationId, taskId, stepId, toIndex): Promise<void> {
			await postJson(
				`${orgBase(organizationId)}/tasks/checklist/move`,
				{
					command_id: normalizeUuidV7('', newId),
					task: taskId,
					step_id: stepId,
					position: toIndex
				},
				'Could not reorder checklist step.'
			);
		},

		async removeStep(organizationId, taskId, stepId): Promise<void> {
			await postJson(
				`${orgBase(organizationId)}/tasks/checklist/remove`,
				{
					command_id: normalizeUuidV7('', newId),
					task: taskId,
					step_id: stepId
				},
				'Could not remove checklist step.'
			);
		},

		async comment(organizationId, taskId, body): Promise<void> {
			await postJson(
				`${orgBase(organizationId)}/tasks/comment`,
				{
					command_id: normalizeUuidV7('', newId),
					task: taskId,
					comment_id: normalizeUuidV7('', newId),
					body_markdown: body
				},
				'Could not post comment.'
			);
		},

		async notes(organizationId, taskId): Promise<PrivateNotesFeed> {
			const [res, { lookup }] = await Promise.all([
				readJson(
					`${orgBase(organizationId)}/tasks/${encodeURIComponent(taskId)}/notes`,
					'Private notes could not be loaded.'
				),
				loadLookup(organizationId)
			]);
			const notes = Array.isArray(res.notes)
				? res.notes.filter(isWirePrivateNote).map((n) => mapPrivateNote(n, lookup))
				: [];
			const viewer = buildViewer(lookup, viewerContext(organizationId));
			return { notes, canWrite: Boolean(viewer.employee) };
		},

		async addNote(organizationId, taskId, change): Promise<PrivateNote> {
			const ids = resolveIds(change.operationId);
			const [res, { lookup }] = await Promise.all([
				postJson(
					`${orgBase(organizationId)}/tasks/notes/add`,
					{
						command_id: ids.commandId,
						task: taskId,
						note_id: ids.entityId,
						body_markdown: change.body
					},
					'Could not save private note.'
				),
				loadLookup(organizationId)
			]);
			pendingOps.delete(change.operationId);
			if (!isWirePrivateNote(res.note)) {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'The saved private note could not be read.'
				} satisfies AppError;
			}
			return mapPrivateNote(res.note, lookup);
		},

		async editNote(organizationId, _taskId, noteId, change): Promise<PrivateNote> {
			const ids = resolveIds(change.operationId);
			const [res, { lookup }] = await Promise.all([
				postJson(
					`${orgBase(organizationId)}/tasks/notes/update`,
					{
						command_id: ids.commandId,
						note_id: noteId,
						expected_revision: change.expectedRevision,
						body_markdown: change.body
					},
					'Could not update private note.'
				),
				loadLookup(organizationId)
			]);
			pendingOps.delete(change.operationId);
			if (!isWirePrivateNote(res.note)) {
				throw {
					source: 'api',
					code: 'unavailable',
					message: 'The updated private note could not be read.'
				} satisfies AppError;
			}
			return mapPrivateNote(res.note, lookup);
		},

		async deleteNote(organizationId, _taskId, noteId, change): Promise<void> {
			const ids = resolveIds(change.operationId);
			await postJson(
				`${orgBase(organizationId)}/tasks/notes/remove`,
				{
					command_id: ids.commandId,
					note_id: noteId,
					expected_revision: change.expectedRevision
				},
				'Could not delete private note.'
			);
			pendingOps.delete(change.operationId);
		},

		async publishNote(organizationId, _taskId, noteId, change): Promise<void> {
			const ids = resolveIds(change.operationId);
			await postJson(
				`${orgBase(organizationId)}/tasks/notes/publish`,
				{
					command_id: ids.commandId,
					note_id: noteId,
					expected_revision: change.expectedRevision,
					comment_id: ids.entityId
				},
				'Could not publish private note.'
			);
			pendingOps.delete(change.operationId);
		},

		async markFixed(
			organizationId,
			taskId,
			change: { how: FixHow; note: string; expectedRevision: number }
		): Promise<void> {
			const prefix =
				change.how.kind === 'task'
					? `Marked as fixed via ${change.how.taskRef}.`
					: change.how.kind === 'commit'
						? `Marked as fixed in commit ${change.how.commit}.`
						: 'Marked as fixed.';
			const body = change.note.trim() ? `${prefix}\n\n${change.note.trim()}` : prefix;
			await postJson(
				`${orgBase(organizationId)}/tasks/comment`,
				{
					command_id: normalizeUuidV7('', newId),
					task: taskId,
					comment_id: normalizeUuidV7('', newId),
					body_markdown: body
				},
				'Could not record fix.'
			);
		},

		async verify(organizationId, taskId, change): Promise<void> {
			const isFixed = change.result === 'fixed';
			await postJson(
				`${orgBase(organizationId)}/tasks/transition`,
				{
					command_id: normalizeUuidV7('', newId),
					task: taskId,
					expected_revision: change.expectedRevision,
					status: isFixed ? 'done' : 'todo',
					...(isFixed ? { resolution: 'fixed' } : {}),
					...(change.note.trim() ? { note: change.note.trim() } : {})
				},
				'Could not record verification.'
			);
		},

		async close(organizationId, taskId, change): Promise<void> {
			const payload = buildClosePayload(change);
			await postJson(
				`${orgBase(organizationId)}/tasks/transition`,
				{
					command_id: normalizeUuidV7('', newId),
					task: taskId,
					expected_revision: change.expectedRevision,
					...payload
				},
				'Could not close work item.'
			);
		},

		async reopen(organizationId, taskId, change): Promise<void> {
			await postJson(
				`${orgBase(organizationId)}/tasks/transition`,
				{
					command_id: normalizeUuidV7('', newId),
					task: taskId,
					expected_revision: change.expectedRevision,
					status: 'todo',
					note: change.note
				},
				'Could not reopen work item.'
			);
		},

		async decline(organizationId, taskId, change): Promise<void> {
			await postJson(
				`${orgBase(organizationId)}/tasks/transition`,
				{
					command_id: normalizeUuidV7('', newId),
					task: taskId,
					expected_revision: change.expectedRevision,
					status: 'cancelled',
					resolution: 'cancelled',
					note: change.reason
				},
				'Could not decline request.'
			);
		},

		approve: unavailableCall,
		askForChanges: unavailableCall,
		merge: unavailableCall,
		whyChoices: unavailableCall,
		linkWhy: unavailableCall,
		cost: unavailableCall,
		setBudget: unavailableCall,
		budgetAction: unavailableCall,
		budgetOperation: unavailableCall
	});
}
