import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LiveCheck } from './model';
import { createProbe, type Office } from './probe';

const unreachable = { source: 'setup', code: 'unreachable', message: 'no' };
const notConnected = { source: 'app', code: 'not_connected', message: 'not running' };

function setup(office: Partial<Office> = {}, options = {}) {
	const seen: LiveCheck[] = [];
	const full: Office = { ping: async () => {}, ensureRunning: async () => ({}), ...office };
	const probe = createProbe(full, (c) => seen.push(c), options);
	return { probe, seen, office: full };
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe('OFC-12 the status check', () => {
	it('an explicit check says Checking…, then up when the office answers', async () => {
		const { probe, seen } = setup();
		await probe.run('explicit');
		expect(seen).toEqual(['checking', 'up']);
	});

	it('a background check keeps the last answer: no Checking… flash', async () => {
		const { probe, seen } = setup();
		await probe.run('background');
		expect(seen).toEqual(['up']);
	});

	it('an office that doesn’t answer is down, never up', async () => {
		const { probe, seen } = setup({ ping: async () => Promise.reject(unreachable) });
		await probe.run('background');
		expect(seen).toEqual(['down']);
	});

	it('silence past the timeout is down (a hung office is never shown as Online)', async () => {
		const { probe, seen } = setup({ ping: () => new Promise(() => {}) }, { backgroundMs: 5000 });
		const done = probe.run('background');
		await vi.advanceTimersByTimeAsync(5001);
		await done;
		expect(seen).toEqual(['down']);
	});

	it('a background check is skipped while another is running', async () => {
		let release!: () => void;
		const ping = vi.fn(() => new Promise<void>((resolve) => (release = resolve)));
		const { probe, seen } = setup({ ping });
		const first = probe.run('background');
		await probe.run('background'); // returns at once
		expect(ping).toHaveBeenCalledTimes(1);
		release();
		await first;
		expect(seen).toEqual(['up']);
	});

	it('the newest check wins: an older, slower one never overwrites it', async () => {
		const releases: (() => void)[] = [];
		const results: ('fail' | 'ok')[] = ['fail', 'ok'];
		const ping = vi.fn(
			() =>
				new Promise<void>((resolve, reject) => {
					const outcome = results.shift();
					releases.push(() => (outcome === 'ok' ? resolve() : reject(unreachable)));
				})
		);
		const { probe, seen } = setup({ ping, ensureRunning: async () => Promise.reject(unreachable) });
		const slow = probe.run('background'); // will fail, late
		const retry = probe.run('explicit'); // newer; succeeds
		releases[1]();
		await retry;
		releases[0]();
		await slow;
		expect(seen).toEqual(['checking', 'up']); // the stale failure was dropped
	});

	it('when nothing started the office yet it starts it, then asks again (even in the background)', async () => {
		let started = false;
		const ensureRunning = vi.fn(async () => (started = true));
		const ping = vi.fn(async () => {
			if (!started) throw notConnected;
		});
		const { probe, seen } = setup({ ping, ensureRunning });
		await probe.run('background');
		expect(ensureRunning).toHaveBeenCalledTimes(1);
		expect(seen).toEqual(['up']);
	});

	it('only an explicit Retry may bring a dead office back; the background check never starts it', async () => {
		let alive = false;
		const ensureRunning = vi.fn(async () => (alive = true));
		const ping = vi.fn(async () => {
			if (!alive) throw unreachable;
		});
		const { probe, seen } = setup({ ping, ensureRunning });
		await probe.run('background');
		expect(ensureRunning).not.toHaveBeenCalled();
		expect(seen).toEqual(['down']);
		await probe.run('explicit');
		expect(ensureRunning).toHaveBeenCalledTimes(1);
		expect(seen).toEqual(['down', 'checking', 'up']);
	});

	it('the start can fail: the explicit check ends down, and there is no loop', async () => {
		const ensureRunning = vi.fn(async () => Promise.reject(unreachable));
		const { probe, seen } = setup({ ping: async () => Promise.reject(unreachable), ensureRunning });
		await probe.run('explicit');
		expect(ensureRunning).toHaveBeenCalledTimes(1);
		expect(seen).toEqual(['checking', 'down']);
	});
});
