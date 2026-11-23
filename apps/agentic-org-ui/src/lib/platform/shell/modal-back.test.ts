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
});
