import { redirect } from '@sveltejs/kit';
import { loadConnections } from '$lib/office/load';

// First launch goes to the welcome screen; after that, straight into the shell.
export async function load() {
	const connections = await loadConnections();
	redirect(307, connections.length > 0 ? '/agents' : '/welcome');
}
