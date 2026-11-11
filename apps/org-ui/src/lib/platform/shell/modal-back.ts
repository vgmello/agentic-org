type BackEntry = { source: string; destination: string };

let onModalPopState: ((event: PopStateEvent) => void) | undefined;
let returningTo: { key: string; arrive: () => void } | undefined;

/**
 * SvelteKit bypasses beforeNavigate for shallow history and while another load is running.
 * Install before its router so neither the dismissed Back nor its return publishes route state.
 */
export function installModalBack() {
	window.addEventListener('popstate', (event) => {
		if (returningTo && window.navigation?.currentEntry?.key === returningTo.key) {
			event.stopImmediatePropagation();
			returningTo.arrive();
		} else {
			onModalPopState?.(event);
		}
	});
}

export function registerModalBack(close: () => void) {
	const navigation = window.navigation;
	if (!navigation) return;

	let back: BackEntry | undefined;

	function rememberBack(event: NavigateEvent) {
		const current = navigation.currentEntry;
		const destination = event.destination;
		back = event.navigationType === 'traverse' && destination.sameDocument
			&& current && destination.index >= 0 && destination.index < current.index
			? { source: current.key, destination: destination.key }
			: undefined;
	}

	function handlePopState(event: PopStateEvent) {
		if (returningTo || !back || navigation.currentEntry?.key !== back.destination) return;
		const { source, destination } = back;
		back = undefined;
		event.stopImmediatePropagation();
		let arrive!: () => void;
		const arrived = new Promise<void>((resolve) => { arrive = resolve; });
		const pending = { key: source, arrive };
		returningTo = pending;

		requestAnimationFrame(async () => {
			try {
				if (navigation.currentEntry?.key === destination) {
					// Chromium can finish the traversal promise before delivering popstate.
					await Promise.all([navigation.traverseTo(source).finished, arrived]);
				} else if (navigation.currentEntry?.key === source) {
					await arrived;
				}
			} catch {
				// A newer navigation can abort the return.
			} finally {
				if (returningTo === pending) returningTo = undefined;
				if (onModalPopState === handlePopState) close();
			}
		});
	}

	onModalPopState = handlePopState;
	navigation.addEventListener('navigate', rememberBack);
	return () => {
		navigation.removeEventListener('navigate', rememberBack);
		if (onModalPopState === handlePopState) onModalPopState = undefined;
	};
}
