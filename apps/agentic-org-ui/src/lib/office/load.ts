import { error } from '@sveltejs/kit';
import { officeApi, toAppError } from './api';

/**
 * The remembered connections, oldest first. If they can't be read (an unsupported file version,
 * an I/O error), fail the page with the real message instead of SvelteKit's generic one.
 */
export async function loadConnections() {
	try {
		return await officeApi.connections();
	} catch (e) {
		const failure = toAppError(e);
		error(failure.code === 'unavailable' ? 503 : 500, failure.message);
	}
}
