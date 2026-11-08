import { redirect } from '@sveltejs/kit';
import { loadConnections } from '$lib/office/load';

// The shell needs a remembered connection; without one, start from the welcome screen.
export async function load({ depends }) {
	depends('app:connections');
	const connections = await loadConnections();
	if (connections.length === 0) redirect(307, '/welcome');
	// The list is oldest first: show the most recently created or opened organization.
	return { connection: connections[connections.length - 1] };
}
