import { redirect } from '@sveltejs/kit';
import { appSession } from '$lib/platform/auth/app-session.svelte';
import { readJsonObject, readResponse } from '$lib/platform/http/wire';
import { selection } from '$lib/platform/organizations/selection';

interface ListedOrganizations {
	ids: string[];
	creatingCount: number;
}

async function listOrganizations(): Promise<ListedOrganizations | undefined> {
	try {
		const reply = await appSession.controller().read('/api/v1/organizations');
		if (reply.status !== 200) return undefined;
		const body = readResponse(reply.body, readJsonObject);
		if (!('result' in body) || !Array.isArray(body.result.organizations)) return undefined;
		const ids = body.result.organizations
			.map((entry) =>
				entry && typeof entry === 'object' && typeof (entry as { id?: unknown }).id === 'string'
					? (entry as { id: string }).id
					: undefined
			)
			.filter((id): id is string => id !== undefined);
		const creatingCount = Array.isArray(body.result.creating) ? body.result.creating.length : 0;
		return { ids, creatingCount };
	} catch {
		return undefined;
	}
}

// The instance URL:
// - Signed out -> /sign-in (SIGN-1).
// - Signed in with a selected organization that exists on this instance -> /teams (live shell).
// - Local authless HQ with zero organizations -> /create (ADR-0034).
// - Otherwise -> /welcome.
export async function load() {
	await appSession.boot();
	if (appSession.state.kind !== 'signed-in') {
		redirect(307, '/sign-in');
	}
	const listed = await listOrganizations();
	if (listed) {
		const selected = selection.current({
			instanceId: appSession.state.session.instance_id,
			accountId: appSession.state.session.account.id
		});
		if (selected && listed.ids.includes(selected)) {
			redirect(307, '/teams');
		}
		if (!appSession.authEnabled && listed.ids.length === 0 && listed.creatingCount === 0) {
			redirect(307, '/create');
		}
	}
	redirect(307, '/welcome');
}
