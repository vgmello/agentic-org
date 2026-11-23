type BackEntry = { source: string; destination: string };

let installed = false;
let onModalPopState: ((event: PopStateEvent) => void) | undefined;
let returningTo: { key: string; arrive: () => void } | undefined;
let legacyReturning: { url: string; close: () => void } | undefined;
let legacyHistoryLength = 0;
let legacyAppendedFragment: string | undefined;

function sveltekitIndex(state: unknown): number | undefined {
	if (typeof state !== 'object' || state === null) return undefined;
	const index = (state as Record<string, unknown>)['sveltekit:index'];
	return typeof index === 'number' ? index : undefined;
}

function samePathAndSearch(a: string, b: string): boolean {
	try {
		const left = new URL(a);
		const right = new URL(b);
		return left.pathname === right.pathname && left.search === right.search;
	} catch {
		return false;
	}
}

/**
 * Keep legacy (no Navigation API) history length tracking in sync across client navigations
 * so Back from a freshly appended native fragment can be distinguished from Forward.
 */
export function noteModalBackNavigation() {
	if (typeof window === 'undefined' || window.navigation) return;
	legacyHistoryLength = window.history.length;
	legacyAppendedFragment = undefined;
}

/**
 * SvelteKit bypasses beforeNavigate for shallow history and while another load is running.
 * Install before its router so neither the dismissed Back nor its return publishes route state.
 */
export function installModalBack() {
	if (installed) return;
	installed = true;
	legacyHistoryLength = window.history.length;

	window.addEventListener(
		'popstate',
		(event) => {
			if (returningTo && window.navigation?.currentEntry?.key === returningTo.key) {
				event.stopImmediatePropagation();
				returningTo.arrive();
				return;
			}
			if (legacyReturning && !window.navigation) {
				event.stopImmediatePropagation();
				const pending = legacyReturning;
				legacyReturning = undefined;
				requestAnimationFrame(() => pending.close());
				return;
			}
			if (onModalPopState) {
				onModalPopState(event);
				return;
			}
			if (!window.navigation) {
				const length = window.history.length;
				legacyAppendedFragment =
					event.state === null && length > legacyHistoryLength && window.location.hash
						? window.location.href
						: undefined;
				legacyHistoryLength = length;
			}
		},
		{ capture: true }
	);
}

export function registerModalBack(close: () => void) {
	const navigation = window.navigation;
	if (!navigation) {
		const savedUrl = window.location.href;
		const savedHash = window.location.hash;
		const savedState = window.history.state;
		const savedIndex = sveltekitIndex(savedState);
		const savedWasAppendedFragment = legacyAppendedFragment === savedUrl;
		if (!savedWasAppendedFragment && legacyAppendedFragment !== undefined) {
			legacyAppendedFragment = undefined;
		}
		legacyHistoryLength = window.history.length;

		function handleLegacyPopState(event: PopStateEvent) {
			if (legacyReturning) return;
			const nextIndex = sveltekitIndex(event.state);
			const nextUrl = window.location.href;
			const sameRoute = samePathAndSearch(savedUrl, nextUrl);

			let delta = 0;
			if (typeof savedIndex === 'number' && typeof nextIndex === 'number') {
				if (nextIndex < savedIndex) delta = savedIndex - nextIndex;
			} else if (savedWasAppendedFragment && sameRoute && !window.location.hash) {
				delta = 1;
			} else if (
				typeof savedIndex === 'number' &&
				nextIndex === undefined &&
				(!sameRoute || (Boolean(savedHash) && !window.location.hash))
			) {
				delta = 1;
			} else if (savedIndex === undefined && savedState !== null && typeof nextIndex === 'number') {
				delta = 1;
			}

			if (delta <= 0) return;

			event.stopImmediatePropagation();
			legacyReturning = {
				url: savedUrl,
				close: () => {
					if (onModalPopState === handleLegacyPopState) close();
				}
			};
			window.history.go(delta);
		}

		onModalPopState = handleLegacyPopState;
		return () => {
			if (onModalPopState === handleLegacyPopState) onModalPopState = undefined;
			legacyHistoryLength = window.history.length;
		};
	}

	let back: BackEntry | undefined;

	function rememberBack(event: NavigateEvent) {
		const current = navigation.currentEntry;
		const destination = event.destination;
		back =
			event.navigationType === 'traverse' &&
			destination.sameDocument &&
			current &&
			destination.index >= 0 &&
			destination.index < current.index
				? { source: current.key, destination: destination.key }
				: undefined;
	}

	function handlePopState(event: PopStateEvent) {
		if (returningTo || !back || navigation.currentEntry?.key !== back.destination) return;
		const { source, destination } = back;
		back = undefined;
		event.stopImmediatePropagation();
		let arrive!: () => void;
		const arrived = new Promise<void>((resolve) => {
			arrive = resolve;
		});
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
