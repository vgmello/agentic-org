import { describe, expect, it } from 'vitest';
import { Loader } from './loader.svelte';

describe('Home Loader controller', () => {
	it('ignores stale out-of-order responses when a newer run completes first', async () => {
		let resolveFirst!: (value: string) => void;
		let call = 0;
		const loader = new Loader<string>(
			() =>
				new Promise<string>((resolve) => {
					call += 1;
					if (call === 1) resolveFirst = resolve;
					else resolve('fresh');
				})
		);

		const first = loader.run(true);
		const second = loader.run(true);
		await second;
		expect(loader.phase).toBe('ready');
		expect(loader.value).toBe('fresh');

		resolveFirst('stale');
		await first;
		expect(loader.value).toBe('fresh');
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
