// A read that is refused to viewers the spec doesn't show amounts to (costs-and-insights-ux §1 "Who sees amounts").
// The refusal is part of the answer, not an error: the page says why instead of offering Retry.
import { toAppError } from '$lib/modules/offices/office-api';

export type Scoped<T> = { allowed: true; value: T } | { allowed: false; reason: string };

export async function scoped<T>(load: () => Promise<T>): Promise<Scoped<T>> {
	try {
		return { allowed: true, value: await load() };
	} catch (e) {
		const failure = toAppError(e);
		if (failure.code === 'not_authorized') return { allowed: false, reason: failure.message };
		throw e;
	}
}
