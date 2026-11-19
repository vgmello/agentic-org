import { afterEach, describe, expect, it, vi } from 'vitest';
import teardown from '../../../e2e-csp/teardown';
import { clean, quiet, snapshot } from '../../../e2e-csp/collector';

const empty = { reports: [], errors: [], missingHeaders: 0 };
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
function respond(body: unknown, status = 200) { vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify(body), { status }))); }
describe('B1-05 collector gate client', () => {
	it('refuses an unavailable status endpoint', async () => { respond({}, 503); await expect(snapshot()).rejects.toThrow(); });
	it.each([
		{ ...empty, reports: [{ documentURI: 'http://localhost/agents', directive: 'script-src', blockedURI: 'inline', sample: 'late' }] },
		{ ...empty, errors: ['unknown report shape'] },
		{ ...empty, missingHeaders: 1 }
	])('fails closed for every nonzero collector dimension: %j', async (body) => { respond(body); await expect(clean()).rejects.toThrow(); });
	it('teardown waits three seconds and invokes the nonzero-report gate', async () => {
		vi.useFakeTimers();
		respond({ ...empty, reports: [{ documentURI: 'late' }] });
		const task = teardown();
		const failed = expect(task).rejects.toThrow();
		await vi.advanceTimersByTimeAsync(2999);
		expect(fetch).not.toHaveBeenCalled();
		await vi.advanceTimersByTimeAsync(1);
		await failed;
		expect(fetch).toHaveBeenCalledOnce();
	});
	it('waits a full quiet second after the latest change, including a report arriving after an earlier poll', async () => {
		let clock = 0, calls = 0;
		const times = [0, 900, 1000, 1500, 1999, 2000];
		vi.spyOn(Date, 'now').mockImplementation(() => clock);
		vi.stubGlobal('fetch', vi.fn(async () => {
			clock = times[Math.min(calls++, times.length - 1)];
			return new Response(JSON.stringify({ ...empty, errors: calls >= 3 ? ['late report parse error'] : [] }));
		}));
		await quiet();
		expect(calls).toBe(6);
	});
});
