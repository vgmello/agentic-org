/**
 * One Home section's load: loading, ready, or failed with the reason, and a Retry that loads again. A call the
 * web API does not serve yet (`code: 'unavailable'`) is its own state, so the section says so and offers no Retry.
 */
export class Loader<T> {
	phase = $state<'loading' | 'ready' | 'error' | 'unavailable'>('loading');
	value = $state.raw<T | undefined>();
	message = $state<string | undefined>();
	retrying = $state(false);
	#load: () => Promise<T>;
	#request = 0;

	constructor(load: () => Promise<T>) {
		this.#load = load;
	}

	async run(initial = true): Promise<void> {
		const mine = ++this.#request;
		if (initial) this.phase = 'loading';
		else this.retrying = true;
		try {
			const value = await this.#load();
			if (mine !== this.#request) return;
			this.value = value;
			this.phase = 'ready';
		} catch (e) {
			if (mine !== this.#request) return;
			// A silent refresh that fails keeps what is shown.
			if (!initial && this.phase === 'ready') return;
			const code = (e as { code?: string } | null)?.code;
			this.message = e instanceof Error ? e.message : undefined;
			this.phase = code === 'unavailable' ? 'unavailable' : 'error';
		} finally {
			if (mine === this.#request) this.retrying = false;
		}
	}

	retry = () => this.run(true);
}
