import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

interface FakeEvent {
	state: unknown;
	stopped: boolean;
	stopImmediatePropagation: () => void;
}

function createFakeWindow(initialHref: string, initialState: unknown, initialLength = 2) {
	let href = initialHref;
	let state = initialState;
	let length = initialLength;
	const goCalls: number[] = [];
	const captureListeners: ((event: FakeEvent) => void)[] = [];
	const bubbleListeners: ((event: FakeEvent) => void)[] = [];
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
			if (type !== 'popstate') return;
			const capture = typeof options === 'boolean' ? options : Boolean(options?.capture);
			(capture ? captureListeners : bubbleListeners).push(fn);
		}
	};

	function setEntry(nextHref: string, nextState: unknown, nextLength = length) {
		href = nextHref;
		state = nextState;
		length = nextLength;
	}

	function dispatchPopState(nextHref: string, nextState: unknown, nextLength = length) {
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
		for (const fn of captureListeners) {
			fn(event);
			if (stopped) return event;
		}
		for (const fn of bubbleListeners) {
			fn(event);
			if (stopped) return event;
		}
		return event;
	}

	function flushRaf() {
		while (rafQueue.length > 0) rafQueue.shift()!();
	}

	return {
		fakeWindow,
		goCalls,
		setEntry,
		dispatchPopState,
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

	async function setup(href: string, state: unknown, length = 2) {
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

	it('intercepts Back using sveltekit:history, restores exact delta, and closes on matching return', async () => {
		const env = await setup('http://localhost/projects', { 'sveltekit:history': 3 }, 4);
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
		expect(env.goCalls).toEqual([2]);
		expect(close).not.toHaveBeenCalled();

		// Matching return popstate to /projects with sveltekit:history 3
		const returnEvent = env.dispatchPopState('http://localhost/projects', {
			'sveltekit:history': 3
		});
		expect(returnEvent.stopped).toBe(true);
		expect(routerPop).not.toHaveBeenCalled();
		env.flushRaf();
		expect(close).toHaveBeenCalledTimes(1);

		unregister();
	});

	it('lets Forward popstate pass through untouched', async () => {
		const env = await setup('http://localhost/projects', { 'sveltekit:history': 2 }, 4);
		const routerPop = vi.fn();
		env.fakeWindow.addEventListener('popstate', routerPop);

		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		const forwardEvent = env.dispatchPopState('http://localhost/work', {
			'sveltekit:history': 3
		});
		expect(forwardEvent.stopped).toBe(false);
		expect(routerPop).toHaveBeenCalledTimes(1);
		expect(env.goCalls).toEqual([]);
		expect(close).not.toHaveBeenCalled();

		unregister();
	});

	it('intercepts Back from a freshly appended native fragment and passes Forward through', async () => {
		const env = await setup('http://localhost/activity', { 'sveltekit:history': 2 }, 2);
		env.noteModalBackNavigation();

		// Freshly appended native fragment: state === null and history.length grew
		env.dispatchPopState('http://localhost/activity#section', null, 3);

		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		const backEvent = env.dispatchPopState('http://localhost/activity', {
			'sveltekit:history': 2
		});
		expect(backEvent.stopped).toBe(true);
		expect(env.goCalls).toEqual([1]);

		const returnEvent = env.dispatchPopState('http://localhost/activity#section', null, 3);
		expect(returnEvent.stopped).toBe(true);
		env.flushRaf();
		expect(close).toHaveBeenCalledTimes(1);
		unregister();

		// After going back without growing length, Forward across fragments passes through
		env.dispatchPopState('http://localhost/activity#one', null, 3);
		const close2 = vi.fn();
		const unregister2 = env.registerModalBack(close2);
		const forwardFrag = env.dispatchPopState('http://localhost/activity#two', null, 3);
		expect(forwardFrag.stopped).toBe(false);
		expect(close2).not.toHaveBeenCalled();
		unregister2();
	});

	it('intercepts Back from a shallow non-SvelteKit pushState entry', async () => {
		const env = await setup('http://localhost/projects?sheet=1', { customSheet: true }, 3);
		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		const backEvent = env.dispatchPopState('http://localhost/projects', {
			'sveltekit:history': 2
		});
		expect(backEvent.stopped).toBe(true);
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

	it('does not swallow an unrelated popstate (second Back) while a legacy return is pending', async () => {
		const env = await setup('http://localhost/projects', { 'sveltekit:history': 3 }, 4);
		const routerPop = vi.fn();
		env.fakeWindow.addEventListener('popstate', routerPop);

		const close = vi.fn();
		const unregister = env.registerModalBack(close);

		// First Back triggers pending return to /projects (sveltekit:history: 3)
		const firstBack = env.dispatchPopState('http://localhost/projects/beacon', {
			'sveltekit:history': 2
		});
		expect(firstBack.stopped).toBe(true);
		expect(env.goCalls).toEqual([1]);

		// Unrelated second Back lands on a native fragment / older entry before return settles
		const secondBack = env.dispatchPopState('http://localhost/teams#top', null, 4);
		expect(secondBack.stopped).toBe(false);
		expect(routerPop).toHaveBeenCalledTimes(1);
		env.flushRaf();
		expect(close).not.toHaveBeenCalled();

		unregister();
	});
});
