import { describe, expect, it } from 'vitest';
import { Loader } from './loader.svelte';

describe('Home Loader controller', () => {
	it('ignores stale out-of-order responses and aborts superseded/disposed signals', async () => {
		let resolveFirst!: (value: string) => void;
		let call = 0;
		const signals: (AbortSignal | undefined)[] = [];
		const loader = new Loader<string>(
			(signal) =>
				new Promise<string>((resolve) => {
					signals.push(signal);
					call += 1;
					if (call === 1) resolveFirst = resolve;
					else resolve('fresh');
				})
		);

		const first = loader.run(true);
		expect(signals[0]?.aborted).toBe(false);
		const second = loader.run(true);
		expect(signals[0]?.aborted).toBe(true);
		expect(signals[1]?.aborted).toBe(false);
		await second;
		expect(loader.phase).toBe('ready');
		expect(loader.value).toBe('fresh');

		resolveFirst('stale');
		await first;
		expect(loader.value).toBe('fresh');

		const third = loader.run(true);
		loader.dispose();
		expect(signals[2]?.aborted).toBe(true);
		await third;
	});

	it('preserves existing data on a silent refresh failure and marks unavailable errors', async () => {
		let fail = false;
		const loader = new Loader<string>(async () => {
			if (fail) throw Object.assign(new Error('offline'), { code: 'unavailable' });
			return 'ok';
		});

		await loader.run(true);
		expect(loader.value).toBe('ok');

		fail = true;
		await loader.run(false);
		expect(loader.phase).toBe('ready');
		expect(loader.value).toBe('ok');

		await loader.retry();
		expect(loader.phase).toBe('unavailable');
	});
});
