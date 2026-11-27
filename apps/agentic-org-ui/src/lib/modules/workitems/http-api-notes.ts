import type { AppError } from '$lib/proto';
import { buildViewer, mapPrivateNote, type OrgLookup } from './wire-map';
import { isWirePrivateNote, type PendingIds } from './http-api-payloads';
import type { WorkItemsApi } from './api';
import type { PrivateNote, PrivateNotesFeed } from './types';

interface WorkItemNotesDeps {
	orgBase: (orgId: string) => string;
	readJson: (path: string, fallbackMessage: string) => Promise<Record<string, unknown>>;
	postJson: (
		path: string,
		body: Record<string, unknown>,
		fallbackMessage: string
	) => Promise<Record<string, unknown>>;
	loadLookup: (organizationId: string) => Promise<{ lookup: OrgLookup }>;
	resolveIds: (operationId: string) => PendingIds;
	clearPendingOp: (operationId: string) => void;
	viewerContext: (organizationId: string) => { employeeId?: string; isAdmin: boolean } | undefined;
}

export function createWorkItemNotesHandlers(
	deps: WorkItemNotesDeps
): Pick<WorkItemsApi, 'notes' | 'addNote' | 'editNote' | 'deleteNote' | 'publishNote'> {
	return {
		async notes(organizationId, taskId): Promise<PrivateNotesFeed> {
			const [res, { lookup }] = await Promise.all([
				deps.readJson(
					`${deps.orgBase(organizationId)}/tasks/${encodeURIComponent(taskId)}/notes`,
					'Private notes could not be loaded.'
				),
				deps.loadLookup(organizationId)
			]);
			const notes = Array.isArray(res.notes)
				? res.notes.filter(isWirePrivateNote).map((n) => mapPrivateNote(n, lookup))
				: [];
			const viewer = buildViewer(lookup, deps.viewerContext(organizationId));
			return { notes, canWrite: Boolean(viewer.employee) };
		},

		async addNote(organizationId, taskId, change): Promise<PrivateNote> {
			const ids = deps.resolveIds(change.operationId);
			const [res, { lookup }] = await Promise.all([
				deps.postJson(
					`${deps.orgBase(organizationId)}/tasks/notes/add`,
					{
						command_id: ids.commandId,
						task: taskId,
						note_id: ids.entityId,
						body_markdown: change.body
					},
					'Could not save private note.'
				),
				deps.loadLookup(organizationId)
			]);
			deps.clearPendingOp(change.operationId);
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
			const ids = deps.resolveIds(change.operationId);
			const [res, { lookup }] = await Promise.all([
				deps.postJson(
					`${deps.orgBase(organizationId)}/tasks/notes/update`,
					{
						command_id: ids.commandId,
						note_id: noteId,
						expected_revision: change.expectedRevision,
						body_markdown: change.body
					},
					'Could not update private note.'
				),
				deps.loadLookup(organizationId)
			]);
			deps.clearPendingOp(change.operationId);
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
			const ids = deps.resolveIds(change.operationId);
			await deps.postJson(
				`${deps.orgBase(organizationId)}/tasks/notes/remove`,
				{
					command_id: ids.commandId,
					note_id: noteId,
					expected_revision: change.expectedRevision
				},
				'Could not delete private note.'
			);
			deps.clearPendingOp(change.operationId);
		},

		async publishNote(organizationId, _taskId, noteId, change): Promise<void> {
			const ids = deps.resolveIds(change.operationId);
			await deps.postJson(
				`${deps.orgBase(organizationId)}/tasks/notes/publish`,
				{
					command_id: ids.commandId,
					note_id: noteId,
					expected_revision: change.expectedRevision,
					comment_id: ids.entityId
				},
				'Could not publish private note.'
			);
			deps.clearPendingOp(change.operationId);
		}
	};
}
