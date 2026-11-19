import { describe, expect, it, vi } from 'vitest';
import { chartMeasurement } from './chart-measurement';

function clock() {
	let id = 0;
	const pending = new Map<number, FrameRequestCallback>();
	return {
		raf: vi.fn((callback: FrameRequestCallback) => { pending.set(id, callback); return id++; }),
		cancel: vi.fn((frame: number) => { pending.delete(frame); }),
		flush(time: number) {
			const callbacks = [...pending.values()];
			pending.clear();
			for (const callback of callbacks) callback(time);
		}
	};
}

describe('chart measurement scheduling', () => {
	it('coalesces observer and content requests into one measurement per frame, using the latest content', () => {
		const frames = clock();
		let content = 'before';
		const run = vi.fn(() => content);
		const measure = chartMeasurement(run, frames.raf, frames.cancel);
		measure.schedule(); // ResizeObserver
		content = 'after';
		measure.schedule(); // post-DOM content effect
		measure.schedule();
		expect(frames.raf).toHaveBeenCalledTimes(1);
		expect(run).not.toHaveBeenCalled();
		frames.flush(16);
		expect(run).toHaveBeenCalledTimes(1);
		expect(run).toHaveLastReturnedWith('after');
		measure.schedule();
		measure.schedule();
		frames.flush(32);
		expect(run).toHaveBeenCalledTimes(2);
		expect(frames.raf).toHaveBeenCalledTimes(2);
	});

	it('cancels frame zero on cleanup, tolerates repeated cleanup and can schedule again', () => {
		const frames = clock();
		const run = vi.fn();
		const measure = chartMeasurement(run, frames.raf, frames.cancel);
		measure.schedule();
		measure.cancel();
		measure.cancel();
		expect(frames.cancel).toHaveBeenCalledExactlyOnceWith(0);
		frames.flush(16);
		expect(run).not.toHaveBeenCalled();
		measure.schedule();
		frames.flush(32);
		expect(run).toHaveBeenCalledTimes(1);
	});
});
