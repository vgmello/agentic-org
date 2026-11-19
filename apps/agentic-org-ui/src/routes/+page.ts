import { redirect } from '@sveltejs/kit';
import { appSession } from '$lib/platform/auth/app-session.svelte';

// The instance URL: signed in opens what §4a.5 says (the organization list until A4), otherwise sign in (SIGN-1).
export async function load() {
	await appSession.boot();
	redirect(307, appSession.state.kind === 'signed-in' ? '/welcome' : '/sign-in');
}
