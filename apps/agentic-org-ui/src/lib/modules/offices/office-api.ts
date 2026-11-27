import type { AppError, Connection, HelloResult, Organization, StatusUpdate } from '$lib/proto';
import { appSession } from '$lib/platform/auth/app-session.svelte';
import { selection } from '$lib/platform/organizations/selection';
import { unavailableCall } from '$lib/platform/unavailable-api';
import { createHttpOfficeApi } from './http-api';

/** Browser office capability. Concrete gateway adapters replace the unavailable default. */
export interface OfficeApi {
	/** Offices and organizations remembered from earlier launches. */
	connections(): Promise<Connection[]>;
	/** Legacy flow readiness seam; unavailable until replaced by authenticated gateway bootstrap. */
	ensureRunning(): Promise<HelloResult>;
	/** Whether the running office answers. Read-only (never starts anything): safe to call often. */
	ping(): Promise<HelloResult>;
	/** The organizations as the host lists them now, read-only: it never starts or restarts anything (for the status checks). */
	peekOrganizations(signal?: AbortSignal): Promise<Organization[]>;
	/** Ask the host to start an office again (`office.restart`). Rejects with the host's refusal; an office already starting is not an error. */
	restartOffice(officeId: string): Promise<void>;
	/** Create an organization on this Mac's host (reached by `ensureRunning`), and remember it. */
	createOrganization(name: string): Promise<Connection>;
	/** The organizations this Mac already has, each with its offices here (`organization.list`). */
	listOrganizations(): Promise<Organization[]>;
	/** Open one of them: remember the connection to its office here. */
	openOrganization(id: string): Promise<Connection>;
	/** Be told when an office's status changes (the host's `office.status_changed`). Resolves to the function that stops listening. */
	onOfficeStatus(listener: (update: StatusUpdate) => void): Promise<() => void>;
}

function signedInSession() {
	return appSession.state.kind === 'signed-in' ? appSession.state.session : undefined;
}

function selectionOwner() {
	const session = signedInSession();
	return session ? { instanceId: session.instance_id, accountId: session.account.id } : undefined;
}

const gatewayOfficeApi = createHttpOfficeApi({
	http: {
		read: (path, signal) => appSession.controller().read(path, signal),
		submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
	},
	instanceId: () => signedInSession()?.instance_id,
	newId: () => appSession.newOperationId(),
	getSelectedOrganizationId: () => {
		const owner = selectionOwner();
		return owner ? selection.current(owner) : undefined;
	},
	selectOrganization: (id) => {
		const owner = selectionOwner();
		if (owner) selection.select(owner, id);
	}
});

async function browserConnections(): Promise<Connection[]> {
	if (typeof window === 'undefined' || typeof indexedDB === 'undefined') return unavailableCall();
	if (appSession.state.kind === 'loading') {
		await appSession.boot();
	}
	if (!signedInSession()) {
		throw {
			source: 'api',
			code: 'not_authorized',
			message: 'Sign in to continue.'
		} satisfies AppError;
	}
	return gatewayOfficeApi.connections();
}

/**
 * Production office API backed by `/api/v1/organizations` and office restart routes.
 * Fails closed when no signed-in gateway session exists and never imports mocks.
 */
export const officeApi: OfficeApi = Object.freeze({
	connections: browserConnections,
	ensureRunning: () => (signedInSession() ? gatewayOfficeApi.ensureRunning() : unavailableCall()),
	ping: () => (signedInSession() ? gatewayOfficeApi.ping() : unavailableCall()),
	peekOrganizations: (signal?: AbortSignal) =>
		signedInSession() ? gatewayOfficeApi.peekOrganizations(signal) : unavailableCall(),
	restartOffice: (officeId: string) =>
		signedInSession() ? gatewayOfficeApi.restartOffice(officeId) : unavailableCall(),
	createOrganization: unavailableCall,
	listOrganizations: () =>
		signedInSession() ? gatewayOfficeApi.listOrganizations() : unavailableCall(),
	openOrganization: (id: string) =>
		signedInSession() ? gatewayOfficeApi.openOrganization(id) : unavailableCall(),
	onOfficeStatus: (listener: (update: StatusUpdate) => void) =>
		signedInSession() ? gatewayOfficeApi.onOfficeStatus(listener) : unavailableCall()
});
