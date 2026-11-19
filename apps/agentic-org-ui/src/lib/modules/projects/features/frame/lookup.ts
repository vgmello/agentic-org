// A project read whose "not there" is an answer, not a failure: the frame shows Project not found with the reason
// and keeps the Projects back link, rather than an error with Retry that could never succeed.
import { toAppError } from '$lib/office/api';
import type { ProjectDetail } from '../../types';

export type ProjectLookup =
	{ found: true; project: ProjectDetail } | { found: false; message: string };

export async function lookUpProject(read: () => Promise<ProjectDetail>): Promise<ProjectLookup> {
	try {
		return { found: true, project: await read() };
	} catch (e) {
		const failure = toAppError(e);
		if (failure.source === 'api' && failure.code === 'not_found')
			return { found: false, message: failure.message };
		throw e;
	}
}
