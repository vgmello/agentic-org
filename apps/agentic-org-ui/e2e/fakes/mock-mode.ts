/**
 * Mock-only switch for looking at states: `?mock=error`, `?mock=empty`, ... on the address the app was opened
 * at. Read when a call is made. It exists only in e2e fakes, which no production build contains.
 */
export const mockMode = (): string => {
	try {
		return typeof location === 'undefined'
			? ''
			: (new URLSearchParams(location.search).get('mock') ?? '');
	} catch {
		return '';
	}
};

export const mockLatency = (ms = 250) =>
	new Promise<void>((done) => setTimeout(done, mockMode().includes('slow') ? 4000 : ms));
