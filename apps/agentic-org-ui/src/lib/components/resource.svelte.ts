// One read for a screen part, with the states every place shows (app-shell-ux §6a): loading (a skeleton),
// unavailable (the production adapter does not serve it yet: never sample data), unsupported (the office answered
// `unsupported`: not available in this version, no Retry), error (what failed, with Retry) and ready. It loads when
// created; a later `reload` keeps the shown data until the new answer arrives.
import { onDestroy } from 'svelte';
import { toAppError } from '$lib/proto';
import { UNAVAILABLE_MESSAGE } from '$lib/platform/unavailable-api';

export type ResourceState = 'loading' | 'ready' | 'error' | 'unavailable' | 'unsupported';

function errorStateFor(failure: ReturnType<typeof toAppError>): ResourceState {
	if (failure.source !== 'api') return 'error';
	if (failure.code === 'unavailable' && failure.message === UNAVAILABLE_MESSAGE) {
		return 'unavailable';
	}
	return failure.code === 'unsupported' ? 'unsupported' : 'error';
}

export class Resource<T> {
	state = $state<ResourceState>('loading');
	data = $state.raw<T | undefined>(undefined);
	message = $state('');
	reloading = $state(false);
	#load: (signal?: AbortSignal) => Promise<T>;
	#run = 0;
	#activeAbort: AbortController | undefined;
	#disposed = false;

	constructor(load: (signal?: AbortSignal) => Promise<T>) {
		this.#load = load;
		try {
			onDestroy(() => this.dispose());
		} catch {
			/* Constructed outside component initialization; caller owns dispose(). */
		}
		void this.reload();
	}

	#isStale(run: number, signal: AbortSignal): boolean {
		return run !== this.#run || this.#disposed || signal.aborted;
	}

	async reload(): Promise<void> {
		if (this.#disposed) return;
		this.#activeAbort?.abort();
		const controller = new AbortController();
		this.#activeAbort = controller;
		const run = ++this.#run;
		this.reloading = true;
		try {
			const value = await this.#load(controller.signal);
			if (this.#isStale(run, controller.signal)) return;
			this.data = value;
			this.state = 'ready';
			this.message = '';
		} catch (e) {
			if (this.#isStale(run, controller.signal)) return;
			const failure = toAppError(e);
			this.message = failure.message;
			this.state = errorStateFor(failure);
			// An old answer is never shown beside a failure it no longer matches.
			this.data = undefined;
		} finally {
			if (this.#activeAbort === controller) this.#activeAbort = undefined;
			if (run === this.#run) this.reloading = false;
		}
	}

	dispose(): void {
		this.#disposed = true;
		this.#run++;
		this.#activeAbort?.abort();
		this.#activeAbort = undefined;
		this.reloading = false;
	}
}
