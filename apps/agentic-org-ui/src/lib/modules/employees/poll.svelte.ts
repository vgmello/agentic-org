// Following work that runs at the office (the HR lead's turn, provisioning, the requests list): read again on a timer
// while it is still moving. Call from a component's script; it stops with the component.

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
	let busy = false;
	$effect(() => {
		void round;
		if (!active()) return;
		const timer = setTimeout(() => {
			if (busy) return;
			busy = true;
			void tick().finally(() => {
				busy = false;
				round++;
			});
		}, wait());
		return () => clearTimeout(timer);
	});
}
