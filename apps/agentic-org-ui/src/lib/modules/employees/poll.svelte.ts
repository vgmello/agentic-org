// Following work that runs at the office (the HR lead's turn, provisioning, the requests list): read again on a timer
// while it is still moving. Call from a component's script; it stops with the component.

/**
 * Pure timer driver backing `pollWhile`: schedules one `tick()` at a time while `active()` holds,
 * invokes `onRound()` after each completed tick, and cancels pending timers on `stop()`.
 */
export function createPollDriver(
	active: () => boolean,
	tick: () => Promise<void>,
	wait: () => number,
	onRound: () => void
): { schedule: () => (() => void) | undefined } {
	let busy = false;
	return {
		schedule() {
			if (!active()) return undefined;
			const timer = setTimeout(() => {
				if (busy) return;
				busy = true;
				void tick().finally(() => {
					busy = false;
					onRound();
				});
			}, wait());
			return () => clearTimeout(timer);
		}
	};
}

/**
 * While `active()` holds, waits `wait()` ms (read each round, so a caller can back off after failures), then awaits
 * `tick()`. One read at a time: the next round starts once the last one has finished.
 */
export function pollWhile(
	active: () => boolean,
	tick: () => Promise<void>,
	wait: () => number
): void {
	let round = $state(0);
	const driver = createPollDriver(active, tick, wait, () => {
		round++;
	});
	$effect(() => {
		void round;
		return driver.schedule();
	});
}
