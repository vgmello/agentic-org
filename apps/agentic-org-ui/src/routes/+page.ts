import { redirect } from '@sveltejs/kit';
import { appSession } from '$lib/platform/auth/app-session.svelte';
import { readJsonObject, readResponse } from '$lib/platform/http/wire';

async function hasNoOrganizations(): Promise<boolean> {
	try {
		const reply = await appSession.controller().read('/api/v1/organizations');
		if (reply.status !== 200) return false;
		const body = readResponse(reply.body, readJsonObject);
		if (!('result' in body) || !Array.isArray(body.result.organizations)) return false;
		const creating = Array.isArray(body.result.creating) ? body.result.creating : [];
		return body.result.organizations.length === 0 && creating.length === 0;
	} catch {
		return false;
	}
}

// The instance URL:
// - When auth_enabled is false (local HTTP HQ, ADR-0034), a fresh HQ with no organizations opens
//   the organization creation wizard (/create) directly with zero sign-in wall; existing
//   organizations open /welcome directly.
// - When auth_enabled is true, signed-in opens /welcome and signed-out opens /sign-in (SIGN-1).
export async function load() {
	await appSession.boot();
	if (appSession.state.kind !== 'signed-in') {
		redirect(307, '/sign-in');
	}
	if (!appSession.authEnabled && (await hasNoOrganizations())) {
		redirect(307, '/create');
	}
	redirect(307, '/welcome');
}
