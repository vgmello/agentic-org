import { appSession } from '$lib/platform/auth/app-session.svelte';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { orgApi } from '$lib/modules/organization/api';
import { createHttpProcessesApi } from './http-api';
import type {
	DevPart,
	DevProcessCatalogue,
	DevProcessDetail,
	DocumentDetail,
	DocumentDraft,
	EditorOptions,
	Place,
	PlaceView,
	ProcessDocument,
	ProcessFile
} from './types';

/**
 * What the Processes screens read and change (processes-ux §2; development-processes design §9). The office
 * resolves effective sets and authorizes and validates every write. Refusals reject with an AppError:
 * `not_found`; `invalid_request` with `field` naming the field (a document field, or a development process
 * file's field in `message`; a document that would replace one marked not overridable is refused on `key` with
 * `rule: 'not_overridable'` and a message naming the document above); `already_exists` for a key that is taken; `conflict` with `latest` when the record moved on since
 * `expectedRevision`; `not_authorized`; `too_large` for an import.
 */
export interface ProcessesApi {
	/** What one place follows. Rejects with `not_found` for a team, project or agent that isn't here. */
	view(organizationId: string, place: Place): Promise<PlaceView>;
	document(organizationId: string, documentId: string): Promise<DocumentDetail>;
	editorOptions(organizationId: string): Promise<EditorOptions>;
	create(organizationId: string, draft: DocumentDraft): Promise<ProcessDocument>;
	update(
		organizationId: string,
		documentId: string,
		draft: DocumentDraft,
		expectedRevision: number
	): Promise<ProcessDocument>;

	devProcesses(organizationId: string): Promise<DevProcessCatalogue>;
	devProcess(organizationId: string, processId: string): Promise<DevProcessDetail>;
	/** A new development process, as a draft with a manifest to fill in. */
	createDevProcess(
		organizationId: string,
		key: string,
		name: string
	): Promise<{ processId: string }>;
	/** Writes one file of an organization's development process; advances its revision. */
	writeDevPart(
		organizationId: string,
		processId: string,
		part: DevPart,
		content: string,
		expectedRevision: number
	): Promise<{ processId: string; revision: number }>;
	copyDevProcess(
		organizationId: string,
		processId: string,
		key: string,
		name: string
	): Promise<{ processId: string }>;
	/** Validates the file, gives it a new ID and adds it as a draft. */
	importDevProcess(
		organizationId: string,
		file: File,
		key?: string
	): Promise<{ processId: string }>;
	exportDevProcess(organizationId: string, processId: string): Promise<ProcessFile>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

const gatewayProcessesApi = createHttpProcessesApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	orgApi,
	session: () => {
		const s = signedInSession();
		return s ? { accountAdmin: s.admin } : undefined;
	}
});

// Production processes API backed by `/api/v1/organizations/{org}/processes/*`.
// Under `bun run dev:mock`, e2e/fakes/modules/processes.ts stands in for this file.
export const processesApi: ProcessesApi = Object.freeze<ProcessesApi>({
	view: (organizationId, place) =>
		signedInSession() ? gatewayProcessesApi.view(organizationId, place) : unavailableCall(),
	document: (organizationId, documentId) =>
		signedInSession()
			? gatewayProcessesApi.document(organizationId, documentId)
			: unavailableCall(),
	editorOptions: (organizationId) =>
		signedInSession() ? gatewayProcessesApi.editorOptions(organizationId) : unavailableCall(),
	create: (organizationId, draft) =>
		signedInSession() ? gatewayProcessesApi.create(organizationId, draft) : unavailableCall(),
	update: (organizationId, documentId, draft, expectedRevision) =>
		signedInSession()
			? gatewayProcessesApi.update(organizationId, documentId, draft, expectedRevision)
			: unavailableCall(),
	devProcesses: (organizationId) =>
		signedInSession() ? gatewayProcessesApi.devProcesses(organizationId) : unavailableCall(),
	devProcess: (organizationId, processId) =>
		signedInSession()
			? gatewayProcessesApi.devProcess(organizationId, processId)
			: unavailableCall(),
	createDevProcess: (organizationId, key, name) =>
		signedInSession()
			? gatewayProcessesApi.createDevProcess(organizationId, key, name)
			: unavailableCall(),
	writeDevPart: (organizationId, processId, part, content, expectedRevision) =>
		signedInSession()
			? gatewayProcessesApi.writeDevPart(organizationId, processId, part, content, expectedRevision)
			: unavailableCall(),
	copyDevProcess: (organizationId, processId, key, name) =>
		signedInSession()
			? gatewayProcessesApi.copyDevProcess(organizationId, processId, key, name)
			: unavailableCall(),
	importDevProcess: (organizationId, file, key) =>
		signedInSession()
			? gatewayProcessesApi.importDevProcess(organizationId, file, key)
			: unavailableCall(),
	exportDevProcess: (organizationId, processId) =>
		signedInSession()
			? gatewayProcessesApi.exportDevProcess(organizationId, processId)
			: unavailableCall()
});
