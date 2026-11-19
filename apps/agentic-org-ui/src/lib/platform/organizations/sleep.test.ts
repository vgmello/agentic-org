import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { sleep } from './sleep';

describe('sleep', () => {
	beforeEach(() => vi.useFakeTimers());
	afterEach(() => vi.useRealTimers());

	it('resolves after the wait and removes its abort listener', async () => {
		const stop = new AbortController();
		const add = vi.spyOn(stop.signal, 'addEventListener');
		const remove = vi.spyOn(stop.signal, 'removeEventListener');
		const waited = sleep(1_000, stop.signal);
		await vi.advanceTimersByTimeAsync(1_000);
		await expect(waited).resolves.toBeUndefined();
		expect(add).toHaveBeenCalledTimes(1);
		expect(remove).toHaveBeenCalledTimes(1);
		expect(remove.mock.calls[0][1]).toBe(add.mock.calls[0][1]);
	});
	it('rejects when stopped during the wait and leaves no timer', async () => {
		const stop = new AbortController();
		const waited = sleep(1_000, stop.signal);
		stop.abort();
		await expect(waited).rejects.toThrow('Stopped');
		expect(vi.getTimerCount()).toBe(0);
	});
	it('rejects at once for a signal that is already stopped, with no timer', async () => {
		const stop = new AbortController();
		stop.abort();
		await expect(sleep(1_000, stop.signal)).rejects.toThrow('Stopped');
		expect(vi.getTimerCount()).toBe(0);
	});
	it('waits without a signal', async () => {
		const waited = sleep(10);
		await vi.advanceTimersByTimeAsync(10);
		await expect(waited).resolves.toBeUndefined();
	});
});
