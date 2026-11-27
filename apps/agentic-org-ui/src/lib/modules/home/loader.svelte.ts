/**
 * One Home section's load: loading, ready, or failed with the reason, and a Retry that loads again. A call the
 * web API does not serve yet (`code: 'unavailable'`) is its own state, so the section says so and offers no Retry.
 */
export class Loader<T> {
	phase = $state<'loading' | 'ready' | 'error' | 'unavailable'>('loading');
	value = $state.raw<T | undefined>();
	message = $state<string | undefined>();
	retrying = $state(false);
	#load: (signal?: AbortSignal) => Promise<T>;
	#request = 0;
	#activeAbort: AbortController | undefined;

	constructor(load: (signal?: AbortSignal) => Promise<T>) {
		this.#load = load;
	}

	async run(initial = true): Promise<void> {
		this.#activeAbort?.abort();
		const controller = new AbortController();
		this.#activeAbort = controller;
		const mine = ++this.#request;
		if (initial) this.phase = 'loading';
		else this.retrying = true;
		try {
			const value = await this.#load(controller.signal);
			if (mine !== this.#request || controller.signal.aborted) return;
			this.value = value;
			this.phase = 'ready';
		} catch (e) {
			if (mine !== this.#request || controller.signal.aborted) return;
			// A silent refresh that fails keeps what is shown.
			if (!initial && this.phase === 'ready') return;
			const code = (e as { code?: string } | null)?.code;
			this.message = e instanceof Error ? e.message : undefined;
			this.phase = code === 'unavailable' ? 'unavailable' : 'error';
		} finally {
			if (this.#activeAbort === controller) this.#activeAbort = undefined;
			if (mine === this.#request) this.retrying = false;
		}
	}

	dispose(): void {
		this.#request += 1;
		this.#activeAbort?.abort();
		this.#activeAbort = undefined;
	}

	retry = () => this.run(true);
}
