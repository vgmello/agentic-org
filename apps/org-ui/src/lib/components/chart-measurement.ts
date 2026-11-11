export function chartMeasurement(run: () => void, raf = requestAnimationFrame, cancel = cancelAnimationFrame) {
	let pending: number | undefined;
	return {
		schedule() {
			pending ??= raf(() => { pending = undefined; run(); });
		},
		cancel() {
			if (pending !== undefined) cancel(pending);
			pending = undefined;
		}
	};
}
