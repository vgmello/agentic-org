import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

interface FakeEvent {
	state: unknown;
	stopped: boolean;
	stopImmediatePropagation: () => void;
}

function createFakeWindow(initialHref: string, initialState: unknown, initialLength = 1) {
	let href = initialHref;
	let state = initialState;
	let length = initialLength;
	const goCalls: number[] = [];
	const listeners: Record<
		string,
		{ capture: ((e: FakeEvent) => void)[]; bubble: ((e: FakeEvent) => void)[] }
	> = {
		popstate: { capture: [], bubble: [] },
		hashchange: { capture: [], bubble: [] }
	};
	const rafQueue: (() => void)[] = [];

	const fakeWindow = {
		navigation: undefined,
		get location() {
			const u = new URL(href);
			return { href: u.href, hash: u.hash, pathname: u.pathname, search: u.search };
		},
		history: {
			get state() {
				return state;
			},
			get length() {
				return length;
			},
			go(delta: number) {
				goCalls.push(delta);
			},
			replaceState(nextState: unknown, _unused: string, url?: string | URL | null) {
				state = nextState;
				if (url != null) href = new URL(String(url), href).href;
			},
			pushState(nextState: unknown, _unused: string, url?: string | URL | null) {
				state = nextState;
				if (url != null) href = new URL(String(url), href).href;
				length += 1;
			}
		},
		addEventListener(
			type: string,
			fn: (event: FakeEvent) => void,
			options?: boolean | AddEventListenerOptions
		) {
			const bucket = listeners[type];
			if (!bucket) return;
			const capture = typeof options === 'boolean' ? options : Boolean(options?.capture);
			(capture ? bucket.capture : bucket.bubble).push(fn);
		}
	};

	function setEntry(nextHref: string, nextState: unknown, nextLength = length) {
		href = nextHref;
		state = nextState;
		length = nextLength;
	}

	function dispatch(
		type: 'popstate' | 'hashchange',
		nextHref: string,
		nextState: unknown,
		nextLength = length
	) {
		setEntry(nextHref, nextState, nextLength);
		let stopped = false;
		const event: FakeEvent = {
			state: nextState,
			get stopped() {
				return stopped;
			},
			stopImmediatePropagation() {
				stopped = true;
			}
		};
		for (const fn of listeners[type].capture) {
			fn(event);
			if (stopped) return event;
		}
		for (const fn of listeners[type].bubble) {
			fn(event);
			if (stopped) return event;
		}
		return event;
	}

	function flushRaf() {
		const batch = rafQueue.splice(0, rafQueue.length);
		for (const cb of batch) cb();
	}

	return {
		fakeWindow,
		goCalls,
		setEntry,
		dispatchPopState: (nextHref: string, nextState: unknown, nextLength = length) =>
			dispatch('popstate', nextHref, nextState, nextLength),
		dispatchHashChange: (nextHref: string, nextState: unknown, nextLength = length) =>
			dispatch('hashchange', nextHref, nextState, nextLength),
		flushRaf,
		requestAnimationFrame: (cb: () => void) => {
			rafQueue.push(cb);
			return rafQueue.length;
		}
	};
}

describe('modal-back legacy (!window.navigation) path', () => {
	const originalWindow = globalThis.window;
	const originalRaf = globalThis.requestAnimationFrame;

	beforeEach(() => {
		vi.resetModules();
	});

	afterEach(() => {
		Object.defineProperty(globalThis, 'window', {
			value: originalWindow,
			configurable: true,
			writable: true
		});
		Object.defineProperty(globalThis, 'requestAnimationFrame', {
			value: originalRaf,
			configurable: true,
			writable: true
		});
	});

	async function setup(href: string, state: unknown, length = 1) {
		const env = createFakeWindow(href, state, length);
		Object.defineProperty(globalThis, 'window', {
			value: env.fakeWindow,
			configurable: true,
			writable: true
		});
		Object.defineProperty(globalThis, 'requestAnimationFrame', {
			value: env.requestAnimationFrame,
			configurable: true,
			writable: true
		});
		const mod = await import('./modal-back');
		mod.installModalBack();
		return { ...env, ...mod };
	}

	it('intercepts Back using sveltekit:history, schedules return in rAF, and closes on matching return', async () => {
		const env = await setup('http://localhost/projects/beacon', { 'sveltekit:history': 1 }, 1);
		env.setEntry('http://localhost/projects', { 'sveltekit:history': 2 }, 2);
		env.noteModalBackNavigation();
		env.setEntry('http://localhost/work', { 'sveltekit:history': 3 }, 3);
		env.noteModalBackNavigation();

		const routerPop = vi.fn();
		env.fakeWindow.addEventListener('popstate', routerPop);

		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		// Multi-step Back from sveltekit:history 3 -> 1
		const backEvent = env.dispatchPopState('http://localhost/projects/beacon', {
			'sveltekit:history': 1
		});
		expect(backEvent.stopped).toBe(true);
		expect(routerPop).not.toHaveBeenCalled();
		expect(env.goCalls).toEqual([]);
		env.flushRaf();
		expect(env.goCalls).toEqual([2]);
		expect(close).not.toHaveBeenCalled();

		// Matching return popstate to /work with sveltekit:history 3
		const returnEvent = env.dispatchPopState('http://localhost/work', {
			'sveltekit:history': 3
		});
		expect(returnEvent.stopped).toBe(true);
		expect(routerPop).not.toHaveBeenCalled();
		env.flushRaf();
		expect(close).toHaveBeenCalledTimes(1);

		unregister();
	});

	it('handles open modal -> Forward -> Back cleanly by advancing modal baseline', async () => {
		const env = await setup('http://localhost/projects', { 'sveltekit:history': 1 }, 1);
		env.setEntry('http://localhost/work', { 'sveltekit:history': 2 }, 2);
		env.noteModalBackNavigation();
		// Navigate back to /projects so /work sits in Forward history.
		env.dispatchPopState('http://localhost/projects', { 'sveltekit:history': 1 }, 2);

		const routerPop = vi.fn();
		env.fakeWindow.addEventListener('popstate', routerPop);

		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		// Forward while modal is open passes through and keeps modal open.
		const forwardEvent = env.dispatchPopState(
			'http://localhost/work',
			{ 'sveltekit:history': 2 },
			2
		);
		expect(forwardEvent.stopped).toBe(false);
		expect(routerPop).toHaveBeenCalledTimes(1);
		expect(env.goCalls).toEqual([]);
		expect(close).not.toHaveBeenCalled();

		// Subsequent Back from /work -> /projects is intercepted, returns to /work, and closes modal.
		const backEvent = env.dispatchPopState(
			'http://localhost/projects',
			{ 'sveltekit:history': 1 },
			2
		);
		expect(backEvent.stopped).toBe(true);
		env.flushRaf();
		expect(env.goCalls).toEqual([1]);

		const returnEvent = env.dispatchPopState(
			'http://localhost/work',
			{ 'sveltekit:history': 2 },
			2
		);
		expect(returnEvent.stopped).toBe(true);
		env.flushRaf();
		expect(close).toHaveBeenCalledTimes(1);

		unregister();
	});

	it('computes exact delta across mixed history (/teams -> /teams#title -> /activity) on 2-step Back', async () => {
		const env = await setup('http://localhost/teams', { 'sveltekit:history': 0 }, 1);
		// Native fragment entry on /teams
		env.dispatchPopState('http://localhost/teams#title', null, 2);
		// Client route navigation to /activity
		env.setEntry('http://localhost/activity', { 'sveltekit:history': 1 }, 3);
		env.noteModalBackNavigation();

		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		// Back 2 steps directly to /teams (skipping over /teams#title)
		const backEvent = env.dispatchPopState('http://localhost/teams', { 'sveltekit:history': 0 }, 3);
		expect(backEvent.stopped).toBe(true);
		env.flushRaf();
		// Must jump 2 entries (over #title) back to /activity, not just 1!
		expect(env.goCalls).toEqual([2]);

		const returnEvent = env.dispatchPopState(
			'http://localhost/activity',
			{ 'sveltekit:history': 1 },
			3
		);
		expect(returnEvent.stopped).toBe(true);
		env.flushRaf();
		expect(close).toHaveBeenCalledTimes(1);

		unregister();
	});

	it('intercepts Back from a freshly appended native fragment and passes Forward through', async () => {
		const env = await setup('http://localhost/activity', { 'sveltekit:history': 2 }, 2);
		env.dispatchPopState('http://localhost/activity#section', null, 3);

		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		const backEvent = env.dispatchPopState('http://localhost/activity', {
			'sveltekit:history': 2
		});
		expect(backEvent.stopped).toBe(true);
		env.flushRaf();
		expect(env.goCalls).toEqual([1]);

		const returnEvent = env.dispatchPopState('http://localhost/activity#section', null, 3);
		expect(returnEvent.stopped).toBe(true);
		env.flushRaf();
		expect(close).toHaveBeenCalledTimes(1);
		unregister();

		// Push #one, #two, #three, go(-2) to #one, then open modal: Forward to #two passes through.
		env.dispatchPopState('http://localhost/activity#one', null, 4);
		env.dispatchPopState('http://localhost/activity#two', null, 5);
		env.dispatchPopState('http://localhost/activity#three', null, 6);
		env.dispatchPopState('http://localhost/activity#one', null, 6);
		const close2 = vi.fn();
		const unregister2 = env.registerModalBack(close2);
		const forwardFrag = env.dispatchPopState('http://localhost/activity#two', null, 6);
		expect(forwardFrag.stopped).toBe(false);
		expect(close2).not.toHaveBeenCalled();
		unregister2();
	});

	it('intercepts Back from a shallow non-SvelteKit pushState entry', async () => {
		const env = await setup('http://localhost/projects', { 'sveltekit:history': 2 }, 2);
		env.setEntry('http://localhost/projects?sheet=1', { customSheet: true }, 3);
		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		const backEvent = env.dispatchPopState('http://localhost/projects', {
			'sveltekit:history': 2
		});
		expect(backEvent.stopped).toBe(true);
		env.flushRaf();
		expect(env.goCalls).toEqual([1]);

		const returnEvent = env.dispatchPopState(
			'http://localhost/projects?sheet=1',
			{ customSheet: true },
			3
		);
		expect(returnEvent.stopped).toBe(true);
		env.flushRaf();
		expect(close).toHaveBeenCalledTimes(1);
		unregister();
	});

	it('cancels queued rAF return on rapid second Back so history.go is never called twice', async () => {
		const env = await setup('http://localhost/teams', { 'sveltekit:history': 1 }, 1);
		env.setEntry('http://localhost/projects/beacon', { 'sveltekit:history': 2 }, 2);
		env.noteModalBackNavigation();
		env.setEntry('http://localhost/projects', { 'sveltekit:history': 3 }, 3);
		env.noteModalBackNavigation();

		const routerPop = vi.fn();
		env.fakeWindow.addEventListener('popstate', routerPop);

		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		// First Back queues rAF for history.go(1) but has not dispatched yet.
		const firstBack = env.dispatchPopState('http://localhost/projects/beacon', {
			'sveltekit:history': 2
		});
		expect(firstBack.stopped).toBe(true);
		expect(env.goCalls).toEqual([]);

		// Second Back arrives before rAF runs: clears pending return and propagates cleanly.
		const secondBack = env.dispatchPopState('http://localhost/teams', {
			'sveltekit:history': 1
		});
		expect(secondBack.stopped).toBe(false);
		expect(routerPop).toHaveBeenCalledTimes(1);

		// Flushing queued rAF must NOT dispatch history.go or close stale modal.
		env.flushRaf();
		expect(env.goCalls).toEqual([]);
		expect(close).not.toHaveBeenCalled();

		unregister();
	});

	it('does not misclassify a late Forward return as a second Back after dispatched return is aborted', async () => {
		const env = await setup('http://localhost/teams', { 'sveltekit:history': 1 }, 1);
		env.setEntry('http://localhost/projects/beacon', { 'sveltekit:history': 2 }, 2);
		env.noteModalBackNavigation();
		env.setEntry('http://localhost/projects', { 'sveltekit:history': 3 }, 3);
		env.noteModalBackNavigation();

		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		// First Back -> rAF dispatches history.go(1)
		env.dispatchPopState('http://localhost/projects/beacon', { 'sveltekit:history': 2 });
		env.flushRaf();
		expect(env.goCalls).toEqual([1]);

		// Competing second Back lands on /teams before return settles
		const secondBack = env.dispatchPopState('http://localhost/teams', { 'sveltekit:history': 1 });
		expect(secondBack.stopped).toBe(false);

		// If a subsequent Forward to /projects/beacon arrives, cursor sees Forward (not Back)
		// and does not issue a second history.go!
		const lateForward = env.dispatchPopState('http://localhost/projects/beacon', {
			'sveltekit:history': 2
		});
		expect(lateForward.stopped).toBe(false);
		env.flushRaf();
		expect(env.goCalls).toEqual([1]);
		expect(close).not.toHaveBeenCalled();

		unregister();
	});

	it('truncates stale forward fragment entries when a new push reuses sveltekit:history index', async () => {
		const env = await setup('http://localhost/teams', { 'sveltekit:history': 0 }, 1);
		// Build mixed history: /teams (0) -> /teams#title (null) -> /activity (1)
		env.dispatchPopState('http://localhost/teams#title', null, 2);
		env.setEntry('http://localhost/activity', { 'sveltekit:history': 1 }, 3);
		env.noteModalBackNavigation();

		// Back 2 steps to /teams (0); SvelteKit afterNavigate calls noteModalBackNavigation()
		env.dispatchPopState('http://localhost/teams', { 'sveltekit:history': 0 }, 3);
		env.noteModalBackNavigation();

		// Navigate forward to a NEW route /projects reusing sveltekit:history index 1
		env.setEntry('http://localhost/projects', { 'sveltekit:history': 1 }, 2);
		env.noteModalBackNavigation();

		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		// Back from /projects (1) -> /teams (0) must compute delta 1 (not stale delta 2!)
		const backEvent = env.dispatchPopState('http://localhost/teams', { 'sveltekit:history': 0 }, 2);
		expect(backEvent.stopped).toBe(true);
		env.flushRaf();
		expect(env.goCalls).toEqual([1]);

		const returnEvent = env.dispatchPopState(
			'http://localhost/projects',
			{ 'sveltekit:history': 1 },
			2
		);
		expect(returnEvent.stopped).toBe(true);
		env.flushRaf();
		expect(close).toHaveBeenCalledTimes(1);

		unregister();
	});

	it('truncates deleted source entry when a route push with a new sveltekit:history index supersedes an intercepted Back', async () => {
		const env = await setup('http://localhost/teams', { 'sveltekit:history': 100 }, 1);
		env.setEntry('http://localhost/projects', { 'sveltekit:history': 101 }, 2);
		env.noteModalBackNavigation();

		const close1 = vi.fn();
		const unregister1 = env.registerModalBack(close1);

		// Back moves browser to /teams (100) and queues rAF return to /projects (101)
		const back1 = env.dispatchPopState('http://localhost/teams', { 'sveltekit:history': 100 }, 2);
		expect(back1.stopped).toBe(true);

		// Before rAF fires, app pushes a NEW incremented SvelteKit index (102) from /teams,
		// replacing the old forward entry (101), and modal 1 tears down.
		unregister1();
		env.setEntry('http://localhost/work', { 'sveltekit:history': 102 }, 2);
		env.noteModalBackNavigation();

		// Flushing queued rAF must NOT call history.go or close modal 1
		env.flushRaf();
		expect(env.goCalls).toEqual([]);
		expect(close1).not.toHaveBeenCalled();

		// Opening a new modal on /work (102) and pressing Back to /teams (100) must compute
		// delta 1 (NOT 2, because 101 was truncated when 102 was pushed at cursor 0 + 1).
		const close2 = vi.fn();
		const unregister2 = env.registerModalBack(close2);
		const back2 = env.dispatchPopState('http://localhost/teams', { 'sveltekit:history': 100 }, 2);
		expect(back2.stopped).toBe(true);
		env.flushRaf();
		expect(env.goCalls).toEqual([1]);

		const return2 = env.dispatchPopState('http://localhost/work', { 'sveltekit:history': 102 }, 2);
		expect(return2.stopped).toBe(true);
		env.flushRaf();
		expect(close2).toHaveBeenCalledTimes(1);

		unregister2();
	});

	it('still resyncs browser URL via queued rAF when a modal unmounts without a route push, without calling stale close', async () => {
		const env = await setup('http://localhost/teams', { 'sveltekit:history': 10 }, 1);
		env.setEntry('http://localhost/projects', { 'sveltekit:history': 11 }, 2);
		env.noteModalBackNavigation();

		const routerPop = vi.fn();
		env.fakeWindow.addEventListener('popstate', routerPop);

		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		// Back intercepts traversal to /teams (10) and queues rAF return to /projects (11)
		const backEvent = env.dispatchPopState(
			'http://localhost/teams',
			{ 'sveltekit:history': 10 },
			2
		);
		expect(backEvent.stopped).toBe(true);

		// Modal unmounts before rAF fires, with NO route push
		unregister();

		// Queued rAF still executes history.go(1) to keep browser URL aligned with SvelteKit's /projects
		env.flushRaf();
		expect(env.goCalls).toEqual([1]);

		// Matching return popstate is swallowed cleanly, and unmounted close() is NOT called
		const returnEvent = env.dispatchPopState(
			'http://localhost/projects',
			{ 'sveltekit:history': 11 },
			2
		);
		expect(returnEvent.stopped).toBe(true);
		expect(routerPop).not.toHaveBeenCalled();
		env.flushRaf();
		expect(close).not.toHaveBeenCalled();
	});

	it('handles repeated modal Back after an anchor link whose sveltekit:history index is assigned later', async () => {
		const env = await setup('http://localhost/teams', { 'sveltekit:history': 10 }, 1);
		// User clicks an anchor link; hashchange records fragment before SvelteKit stamps sveltekit:history
		env.dispatchHashChange('http://localhost/teams#section', null, 2);
		// SvelteKit later assigns index 11 to that fragment entry, then user navigates to /projects (12)
		env.setEntry('http://localhost/projects', { 'sveltekit:history': 12 }, 3);
		env.noteModalBackNavigation();

		// First modal open + Back to fragment (now carrying sveltekit:history: 11)
		const close1 = vi.fn();
		const unregister1 = env.registerModalBack(close1);
		const back1 = env.dispatchPopState(
			'http://localhost/teams#section',
			{ 'sveltekit:history': 11 },
			3
		);
		expect(back1.stopped).toBe(true);
		env.flushRaf();
		expect(env.goCalls).toEqual([1]);
		const return1 = env.dispatchPopState(
			'http://localhost/projects',
			{ 'sveltekit:history': 12 },
			3
		);
		expect(return1.stopped).toBe(true);
		env.flushRaf();
		expect(close1).toHaveBeenCalledTimes(1);
		unregister1();

		// Reopen modal + second Back must STILL compute delta 1 (not 3 from a duplicated fragment entry)
		const close2 = vi.fn();
		const unregister2 = env.registerModalBack(close2);
		const back2 = env.dispatchPopState(
			'http://localhost/teams#section',
			{ 'sveltekit:history': 11 },
			3
		);
		expect(back2.stopped).toBe(true);
		env.flushRaf();
		expect(env.goCalls).toEqual([1, 1]);
		const return2 = env.dispatchPopState(
			'http://localhost/projects',
			{ 'sveltekit:history': 12 },
			3
		);
		expect(return2.stopped).toBe(true);
		env.flushRaf();
		expect(close2).toHaveBeenCalledTimes(1);
		unregister2();
	});

	it('does not mistake Forward for Back when an anchor href repeats (#a -> #b -> #a)', async () => {
		const env = await setup('http://localhost/teams', { 'sveltekit:history': 10 }, 1);
		env.dispatchHashChange('http://localhost/teams#a', null, 2);
		env.dispatchHashChange('http://localhost/teams#b', null, 3);
		env.dispatchHashChange('http://localhost/teams#a', null, 4);

		// Go Back from second #a to #b (now stamped with index 12)
		env.dispatchPopState('http://localhost/teams#b', { 'sveltekit:history': 12 }, 4);

		const routerPop = vi.fn();
		env.fakeWindow.addEventListener('popstate', routerPop);

		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		// Press Forward to the later #a (stamped with index 13): must NOT be swallowed as Back
		const forwardEvent = env.dispatchPopState(
			'http://localhost/teams#a',
			{ 'sveltekit:history': 13 },
			4
		);
		expect(forwardEvent.stopped).toBe(false);
		expect(routerPop).toHaveBeenCalledTimes(1);
		env.flushRaf();
		expect(env.goCalls).toEqual([]);
		expect(close).not.toHaveBeenCalled();

		unregister();
	});

	it('captures bubble-phase SvelteKit hashchange replaceState stamps synchronously with zero timers', async () => {
		const env = await setup('http://localhost/teams', { 'sveltekit:history': 10 }, 1);

		// Model A: click #a (SvelteKit stamps 11 via replaceState in bubble phase), unindexed #b, click #a (stamps 12)
		env.dispatchHashChange('http://localhost/teams#a', null, 2);
		env.fakeWindow.history.replaceState(
			{ 'sveltekit:history': 11 },
			'',
			'http://localhost/teams#a'
		);

		env.dispatchHashChange('http://localhost/teams#b', null, 3);

		env.dispatchHashChange('http://localhost/teams#a', null, 4);
		env.fakeWindow.history.replaceState(
			{ 'sveltekit:history': 12 },
			'',
			'http://localhost/teams#a'
		);

		// Immediately Back to unindexed #b (no timer flush needed!)
		env.dispatchPopState('http://localhost/teams#b', null, 4);

		// Open modal on #b and press Back to earlier #a (11): MUST intercept and restore #b
		const close1 = vi.fn();
		const unregister1 = env.registerModalBack(close1);
		const backEvent = env.dispatchPopState(
			'http://localhost/teams#a',
			{ 'sveltekit:history': 11 },
			4
		);
		expect(backEvent.stopped).toBe(true);
		env.flushRaf();
		expect(env.goCalls).toEqual([1]);
		const returnEvent = env.dispatchPopState('http://localhost/teams#b', null, 4);
		expect(returnEvent.stopped).toBe(true);
		env.flushRaf();
		expect(close1).toHaveBeenCalledTimes(1);
		unregister1();

		// Reopen modal on #b and press Forward to later #a (12): MUST pass through unstopped
		const routerPop = vi.fn();
		env.fakeWindow.addEventListener('popstate', routerPop);
		const close2 = vi.fn();
		const unregister2 = env.registerModalBack(close2);
		const forwardEvent = env.dispatchPopState(
			'http://localhost/teams#a',
			{ 'sveltekit:history': 12 },
			4
		);
		expect(forwardEvent.stopped).toBe(false);
		expect(routerPop).toHaveBeenCalledTimes(1);
		env.flushRaf();
		expect(env.goCalls).toEqual([1]);
		expect(close2).not.toHaveBeenCalled();
		unregister2();
	});

	it('does not swallow Forward when earlier #a and #b are unindexed and later #a is SvelteKit-indexed synchronously', async () => {
		const env = await setup('http://localhost/teams', { 'sveltekit:history': 10 }, 1);

		// Model B: unindexed #a, unindexed #b, then clicked #a (SvelteKit stamps 11 via replaceState)
		env.dispatchHashChange('http://localhost/teams#a', null, 2);
		env.dispatchHashChange('http://localhost/teams#b', null, 3);
		env.dispatchHashChange('http://localhost/teams#a', null, 4);
		env.fakeWindow.history.replaceState(
			{ 'sveltekit:history': 11 },
			'',
			'http://localhost/teams#a'
		);

		// Immediately Back to unindexed #b, open modal, press Forward to indexed #a (11)
		env.dispatchPopState('http://localhost/teams#b', null, 4);
		const routerPop = vi.fn();
		env.fakeWindow.addEventListener('popstate', routerPop);
		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		const forwardEvent = env.dispatchPopState(
			'http://localhost/teams#a',
			{ 'sveltekit:history': 11 },
			4
		);
		expect(forwardEvent.stopped).toBe(false);
		expect(routerPop).toHaveBeenCalledTimes(1);
		env.flushRaf();
		expect(env.goCalls).toEqual([]);
		expect(close).not.toHaveBeenCalled();
		unregister();
	});

	it('does not duplicate the current anchor slot when a modal opens right after an anchor is stamped', async () => {
		const env = await setup('http://localhost/teams', { 'sveltekit:history': 10 }, 1);
		env.dispatchHashChange('http://localhost/teams#a', null, 2);
		// Even if state were updated without replaceState wrapper, syncLegacyEntry updates in place
		env.setEntry('http://localhost/teams#a', { 'sveltekit:history': 11 }, 2);

		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		// Pressing Back from #a (11) to /teams (10) must compute delta 1 (NOT 2)
		const backEvent = env.dispatchPopState(
			'http://localhost/teams',
			{ 'sveltekit:history': 10 },
			2
		);
		expect(backEvent.stopped).toBe(true);
		env.flushRaf();
		expect(env.goCalls).toEqual([1]);

		const returnEvent = env.dispatchPopState(
			'http://localhost/teams#a',
			{ 'sveltekit:history': 11 },
			2
		);
		expect(returnEvent.stopped).toBe(true);
		env.flushRaf();
		expect(close).toHaveBeenCalledTimes(1);
		unregister();
	});

	it('distinguishes BOTH Back and Forward across repeated unindexed fragments (#one -> #two -> #one)', async () => {
		const env = await setup('http://localhost/teams', { 'sveltekit:history': 10 }, 1);
		env.dispatchHashChange('http://localhost/teams#one', null, 2);
		const slotOneFirst = env.fakeWindow.history.state;
		env.dispatchHashChange('http://localhost/teams#two', null, 3);
		const slotTwo = env.fakeWindow.history.state;
		env.dispatchHashChange('http://localhost/teams#one', null, 4);
		const slotOneSecond = env.fakeWindow.history.state;

		// Go Back to unindexed #two (carrying its stamped slot)
		env.dispatchPopState('http://localhost/teams#two', slotTwo, 4);

		// 1) Open modal on #two and press Back to earlier #one: MUST intercept and close modal
		const close1 = vi.fn();
		const unregister1 = env.registerModalBack(close1);
		const backEvent = env.dispatchPopState('http://localhost/teams#one', slotOneFirst, 4);
		expect(backEvent.stopped).toBe(true);
		env.flushRaf();
		expect(env.goCalls).toEqual([1]);
		const returnEvent = env.dispatchPopState('http://localhost/teams#two', slotTwo, 4);
		expect(returnEvent.stopped).toBe(true);
		env.flushRaf();
		expect(close1).toHaveBeenCalledTimes(1);
		unregister1();

		// 2) Reopen modal on #two and press Forward to later #one: MUST pass through unstopped
		const routerPop = vi.fn();
		env.fakeWindow.addEventListener('popstate', routerPop);
		const close2 = vi.fn();
		const unregister2 = env.registerModalBack(close2);
		const forwardEvent = env.dispatchPopState('http://localhost/teams#one', slotOneSecond, 4);
		expect(forwardEvent.stopped).toBe(false);
		expect(routerPop).toHaveBeenCalledTimes(1);
		env.flushRaf();
		expect(env.goCalls).toEqual([1]);
		expect(close2).not.toHaveBeenCalled();

		unregister2();
	});

	it('swallows rapid second Back when modal is locked (e.g. pending creation) and returns to the locked entry', async () => {
		const env = await setup('http://localhost/home', { 'sveltekit:history': 1 }, 1);
		env.setEntry('http://localhost/teams', { 'sveltekit:history': 2 }, 2);
		env.noteModalBackNavigation();
		env.setEntry('http://localhost/projects', { 'sveltekit:history': 3 }, 3);
		env.noteModalBackNavigation();

		const routerPop = vi.fn();
		env.fakeWindow.addEventListener('popstate', routerPop);
		const close = vi.fn();
		const unregister = env.registerModalBack(close, { locked: () => true });

		// First Back moves to /teams (2), swallowed
		const back1 = env.dispatchPopState('http://localhost/teams', { 'sveltekit:history': 2 }, 3);
		expect(back1.stopped).toBe(true);

		// Rapid second Back before rAF moves to /home (1): MUST ALSO be swallowed and reschedule go(2)
		const back2 = env.dispatchPopState('http://localhost/home', { 'sveltekit:history': 1 }, 3);
		expect(back2.stopped).toBe(true);
		expect(routerPop).not.toHaveBeenCalled();

		env.flushRaf();
		expect(env.goCalls).toEqual([2]);

		const returned = env.dispatchPopState(
			'http://localhost/projects',
			{ 'sveltekit:history': 3 },
			3
		);
		expect(returned.stopped).toBe(true);
		env.flushRaf();
		expect(close).toHaveBeenCalledTimes(1);

		unregister();
	});

	it('treats a newly stamped duplicate fragment slot as a new push after replacing Forward history (#one -> #two -> #three -> Back -> #one)', async () => {
		const env = await setup('http://localhost/teams', { 'sveltekit:history': 10 }, 1);
		env.dispatchHashChange('http://localhost/teams#one', null, 2);
		env.dispatchHashChange('http://localhost/teams#two', null, 3);
		const slotTwo = env.fakeWindow.history.state;
		env.dispatchHashChange('http://localhost/teams#three', null, 4);

		// Go Back to #two, then append #one (replacing #three; length stays 4)
		env.dispatchPopState('http://localhost/teams#two', slotTwo, 4);
		env.dispatchHashChange('http://localhost/teams#one', null, 4);
		const slotOneNew = env.fakeWindow.history.state;

		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		// Press Back to #two: MUST intercept, compute delta 1, and close the modal
		const backEvent = env.dispatchPopState('http://localhost/teams#two', slotTwo, 4);
		expect(backEvent.stopped).toBe(true);
		env.flushRaf();
		expect(env.goCalls).toEqual([1]);

		const returnEvent = env.dispatchPopState('http://localhost/teams#one', slotOneNew, 4);
		expect(returnEvent.stopped).toBe(true);
		env.flushRaf();
		expect(close).toHaveBeenCalledTimes(1);

		unregister();
	});
});
