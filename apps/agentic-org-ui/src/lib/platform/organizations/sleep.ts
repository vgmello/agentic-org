/**
 * A wait that an abort signal ends early (by rejecting). Each wait leaves nothing behind on
 * the signal: a page that polls for a long time does not collect one abort listener per wait.
 */
export function sleep(ms: number, signal?: AbortSignal): Promise<void> {
	return new Promise<void>((resolve, reject) => {
		if (signal?.aborted) return reject(new Error('Stopped'));
		const stopped = () => {
			clearTimeout(timer);
			reject(new Error('Stopped'));
		};
		const timer = setTimeout(() => {
			signal?.removeEventListener('abort', stopped);
			resolve();
		}, ms);
		signal?.addEventListener('abort', stopped, { once: true });
	});
}
