type BackEntry = { source: string; destination: string };
type LegacyEntry = { href: string; index: number | undefined };
type LegacyReturn = {
	href: string;
	index: number | undefined;
	targetCursor: number;
	close: () => void;
};

const HISTORY_INDEX = 'sveltekit:history';

let installed = false;
let onModalPopState: ((event: PopStateEvent) => void) | undefined;
let returningTo: { key: string; arrive: () => void } | undefined;
let legacyReturning: LegacyReturn | undefined;
let legacyEntries: LegacyEntry[] = [];
let legacyCursor = -1;
let legacyHistoryLength = 0;

function sveltekitHistoryIndex(state: unknown): number | undefined {
	if (typeof state !== 'object' || state === null) return undefined;
	const index = (state as Record<string, unknown>)[HISTORY_INDEX];
	return typeof index === 'number' ? index : undefined;
}

function currentLegacyEntry(state: unknown = window.history.state): LegacyEntry {
	return { href: window.location.href, index: sveltekitHistoryIndex(state) };
}

function sameEntry(a: LegacyEntry | undefined, b: LegacyEntry): boolean {
	return Boolean(a) && a!.href === b.href && a!.index === b.index;
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

function findLegacyCursor(entry: LegacyEntry): number {
	if (entry.index !== undefined) {
		let best = -1;
		let bestDist = Infinity;
		let bestUntracked = -1;
		let bestUntrackedDist = Infinity;
		for (let i = 0; i < legacyEntries.length; i++) {
			const candidate = legacyEntries[i];
			if (candidate.index === entry.index) {
				const dist = Math.abs(i - legacyCursor) + (candidate.href === entry.href ? 0 : 0.5);
				if (dist < bestDist) {
					best = i;
					bestDist = dist;
				}
			} else if (candidate.index === undefined && candidate.href === entry.href) {
				// Anchor/fragment entries recorded before SvelteKit assigns sveltekit:history.
				const dist = Math.abs(i - legacyCursor) + (i === legacyCursor ? 0.5 : 0);
				if (dist < bestUntrackedDist) {
					bestUntracked = i;
					bestUntrackedDist = dist;
				}
			}
		}
		return best >= 0 ? best : bestUntracked;
	}

	let bestUntracked = -1;
	let bestUntrackedDist = Infinity;
	let bestAny = -1;
	let bestAnyDist = Infinity;
	for (let i = 0; i < legacyEntries.length; i++) {
		const candidate = legacyEntries[i];
		if (candidate.href !== entry.href) continue;
		const dist = Math.abs(i - legacyCursor) + (i === legacyCursor ? 0.5 : 0);
		if (candidate.index === undefined && dist < bestUntrackedDist) {
			bestUntracked = i;
			bestUntrackedDist = dist;
		}
		if (dist < bestAnyDist) {
			bestAny = i;
			bestAnyDist = dist;
		}
	}
	return bestUntracked >= 0 ? bestUntracked : bestAny;
}

function recordLegacyPush(entry: LegacyEntry) {
	if (legacyCursor >= 0 && sameEntry(legacyEntries[legacyCursor], entry)) {
		legacyHistoryLength = window.history.length;
		return;
	}
	legacyEntries = legacyEntries.slice(0, legacyCursor + 1);
	legacyEntries.push(entry);
	legacyCursor = legacyEntries.length - 1;
	legacyHistoryLength = window.history.length;
}

function syncLegacyEntry(entry: LegacyEntry, isPopState = false) {
	if (legacyCursor < 0) {
		recordLegacyPush(entry);
		return;
	}
	if (sameEntry(legacyEntries[legacyCursor], entry)) {
		legacyHistoryLength = window.history.length;
		return;
	}

	if (isPopState) {
		const grew = window.history.length > legacyHistoryLength;
		if (grew && entry.index === undefined) {
			recordLegacyPush(entry);
			return;
		}
		const existing = findLegacyCursor(entry);
		if (existing >= 0) {
			legacyCursor = existing;
			legacyEntries[existing] = entry;
			legacyHistoryLength = window.history.length;
			return;
		}
		recordLegacyPush(entry);
		return;
	}

	const current = legacyEntries[legacyCursor];
	if (entry.index !== undefined && current.index !== undefined && entry.index === current.index) {
		// Same SvelteKit history slot updated via replaceState.
		legacyEntries[legacyCursor] = entry;
		legacyHistoryLength = window.history.length;
		return;
	}

	// New push navigation: truncate any stale forward entries past legacyCursor.
	recordLegacyPush(entry);
}

/**
 * Keep legacy (no Navigation API) history cursor in sync across client navigations
 * so Back and Forward across mixed route and fragment entries compute exact deltas.
 */
export function noteModalBackNavigation() {
	if (typeof window === 'undefined' || window.navigation) return;
	legacyReturning = undefined;
	syncLegacyEntry(currentLegacyEntry(), false);
}

/**
 * SvelteKit bypasses beforeNavigate for shallow history and while another load is running.
 * Install before its router so neither the dismissed Back nor its return publishes route state.
 */
export function installModalBack() {
	if (installed) return;
	installed = true;
	if (!window.navigation) {
		legacyEntries = [currentLegacyEntry()];
		legacyCursor = 0;
		legacyHistoryLength = window.history.length;
	}

	window.addEventListener(
		'hashchange',
		() => {
			if (window.navigation || onModalPopState || legacyReturning) return;
			syncLegacyEntry(currentLegacyEntry(), true);
		},
		{ capture: true }
	);

	window.addEventListener(
		'popstate',
		(event) => {
			if (returningTo && window.navigation?.currentEntry?.key === returningTo.key) {
				event.stopImmediatePropagation();
				returningTo.arrive();
				return;
			}
			if (legacyReturning && !window.navigation) {
				const pending = legacyReturning;
				const entry = currentLegacyEntry(event.state);
				const matchesReturn =
					entry.href === pending.href &&
					(pending.index === undefined || entry.index === pending.index);
				legacyReturning = undefined;
				if (matchesReturn) {
					event.stopImmediatePropagation();
					legacyCursor = pending.targetCursor;
					if (legacyCursor >= 0 && legacyCursor < legacyEntries.length) {
						legacyEntries[legacyCursor] = entry;
					}
					legacyHistoryLength = window.history.length;
					requestAnimationFrame(() => pending.close());
					return;
				}
				// A second popstate arrived before or instead of the expected return: cancel any
				// undispatched return and update cursor cleanly without swallowing this event.
				syncLegacyEntry(entry, true);
				return;
			}
			if (onModalPopState) {
				onModalPopState(event);
				return;
			}
			if (!window.navigation) {
				syncLegacyEntry(currentLegacyEntry(event.state), true);
			}
		},
		{ capture: true }
	);
}

export function registerModalBack(close: () => void) {
	const navigation = window.navigation;
	if (!navigation) {
		let savedState = window.history.state;
		syncLegacyEntry(currentLegacyEntry(savedState), false);

		function handleLegacyPopState(event: PopStateEvent) {
			if (legacyReturning) return;
			const entry = currentLegacyEntry(event.state);
			const grew = window.history.length > legacyHistoryLength;

			// A newly appended fragment while the modal is open moves forward, not back.
			if (grew && entry.index === undefined && window.location.hash) {
				recordLegacyPush(entry);
				savedState = event.state;
				return;
			}

			const currentCursor = legacyCursor;
			const currentEntry = legacyEntries[currentCursor] ?? entry;
			const nextCursor = findLegacyCursor(entry);
			let delta = 0;

			if (nextCursor >= 0) {
				if (nextCursor < currentCursor) {
					delta = currentCursor - nextCursor;
				} else {
					// Forward (or same entry): advance cursor, keep modal open.
					legacyCursor = nextCursor;
					legacyEntries[nextCursor] = entry;
					legacyHistoryLength = window.history.length;
					savedState = event.state;
					return;
				}
			} else {
				const sameRoute = samePathAndSearch(currentEntry.href, entry.href);
				const currentHash = (() => {
					try {
						return new URL(currentEntry.href).hash;
					} catch {
						return '';
					}
				})();
				if (typeof currentEntry.index === 'number' && typeof entry.index === 'number') {
					if (entry.index < currentEntry.index) {
						delta = currentEntry.index - entry.index;
					} else {
						recordLegacyPush(entry);
						savedState = event.state;
						return;
					}
				} else if (
					typeof currentEntry.index === 'number' &&
					entry.index === undefined &&
					(!sameRoute || (Boolean(currentHash) && !window.location.hash))
				) {
					delta = 1;
				} else if (
					currentEntry.index === undefined &&
					savedState !== null &&
					typeof entry.index === 'number'
				) {
					delta = 1;
				} else {
					recordLegacyPush(entry);
					savedState = event.state;
					return;
				}
			}

			if (delta <= 0) return;

			event.stopImmediatePropagation();
			let targetCursor = currentCursor;
			// Keep legacyCursor aligned with the browser's actual history position immediately.
			if (nextCursor >= 0) {
				legacyCursor = nextCursor;
				legacyEntries[nextCursor] = entry;
			} else {
				legacyEntries.unshift(entry);
				legacyCursor = 0;
				targetCursor = currentCursor + 1;
			}
			legacyHistoryLength = window.history.length;

			const pending: LegacyReturn = {
				href: currentEntry.href,
				index: currentEntry.index,
				targetCursor,
				close: () => {
					if (onModalPopState === handleLegacyPopState) close();
				}
			};
			legacyReturning = pending;
			requestAnimationFrame(() => {
				if (legacyReturning !== pending) return;
				window.history.go(delta);
			});
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
