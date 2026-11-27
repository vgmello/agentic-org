import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createPollDriver, pollWhile } from './poll.svelte';

describe('pollWhile & createPollDriver controller', () => {
	beforeEach(() => {
		vi.useFakeTimers();
	});

	afterEach(() => {
		vi.useRealTimers();
	});

	it('polls while active, prevents overlapping ticks, and cancels timer on stop', async () => {
		let active = true;
		let rounds = 0;
		const tick = vi.fn(async () => {});
		const driver = createPollDriver(
			() => active,
			tick,
			() => 50,
			() => {
				rounds++;
			}
		);

		const stop1 = driver.schedule();
		expect(stop1).toBeDefined();
		await vi.advanceTimersByTimeAsync(50);
		expect(tick).toHaveBeenCalledTimes(1);
		expect(rounds).toBe(1);

		const stop2 = driver.schedule();
		stop2?.();
		await vi.advanceTimersByTimeAsync(100);
		expect(tick).toHaveBeenCalledTimes(1);

		active = false;
		expect(driver.schedule()).toBeUndefined();
		expect(typeof pollWhile).toBe('function');
	});
});
