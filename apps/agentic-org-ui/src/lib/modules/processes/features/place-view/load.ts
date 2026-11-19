// One read of a place for the Processes views: a place that isn't here (`not_found`) is a state of its own rather
// than an error with Retry.
import { toAppError } from '$lib/office/api';
import { processesApi } from '../../api';
import type { Place, PlaceView } from '../../types';

export type PlaceRead = { view: PlaceView; place: Place } | { notFound: true; place: Place };

export async function readPlace(organizationId: string, place: Place): Promise<PlaceRead> {
	try {
		return { view: await processesApi.view(organizationId, place), place };
	} catch (e) {
		const failure = toAppError(e);
		if (failure.source === 'api' && failure.code === 'not_found') return { notFound: true, place };
		throw e;
	}
}
