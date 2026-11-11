import { expect } from '@playwright/test';

export type Snapshot = { reports: { documentURI: string; directive: string; blockedURI: string; sample: string }[]; errors: string[]; missingHeaders: number };
export const origin = `http://localhost:${process.env.CSP_PW_PORT ?? '4373'}`;
export async function snapshot(): Promise<Snapshot> {
	const response = await fetch(`${origin}/__csp-status`);
	expect(response.status).toBe(200);
	return response.json();
}
export async function clean() {
	const value = await snapshot();
	expect(value.errors, 'collector parse/request errors').toEqual([]);
	expect(value.missingHeaders, 'HTML responses missing policy').toBe(0);
	expect(value.reports, 'unexpected CSP reports').toEqual([]);
}
export async function quiet() {
	let previous = '', since = Date.now();
	await expect.poll(async () => {
		const current = JSON.stringify(await snapshot());
		if (current !== previous) { previous = current; since = Date.now(); }
		return Date.now() - since;
	}, { timeout: 15_000, intervals: [100] }).toBeGreaterThanOrEqual(1000);
}
