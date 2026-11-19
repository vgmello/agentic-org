export const UNAVAILABLE_MESSAGE = 'This area is not available yet.';

/** A missing production adapter must never turn into sample data or a successful empty read. */
export async function unavailableCall(): Promise<never> {
	throw Object.assign(new Error(UNAVAILABLE_MESSAGE), { source: 'api' as const, code: 'unavailable' as const });
}
