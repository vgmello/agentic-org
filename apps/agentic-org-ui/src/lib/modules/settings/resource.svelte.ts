// One read of a settings screen's data, with the four states every screen shows: loading, ready, error (with Retry)
// and unavailable (the production adapter does not serve it yet). A later call to `reload` keeps the old data on screen.
import { toAppError } from '$lib/office/api';

export type ResourceState = 'loading' | 'ready' | 'error' | 'unavailable';

export class Resource<T> {
	state = $state<ResourceState>('loading');
	data = $state.raw<T | undefined>(undefined);
	message = $state('');
	reloading = $state(false);
	#load: () => Promise<T>;
	#run = 0;

	constructor(load: () => Promise<T>) {
		this.#load = load;
		void this.reload();
	}

	async reload(): Promise<void> {
		const run = ++this.#run;
		this.reloading = true;
		try {
			const value = await this.#load();
			if (run !== this.#run) return;
			this.data = value;
			this.state = 'ready';
		} catch (e) {
			if (run !== this.#run) return;
			const failure = toAppError(e);
			this.message = failure.message;
			this.state =
				failure.source === 'api' && failure.code === 'unavailable' ? 'unavailable' : 'error';
			// An old answer is never shown beside a failure it no longer matches.
			this.data = undefined;
		} finally {
			if (run === this.#run) this.reloading = false;
		}
	}
}

/** A failure the settings screens raise themselves (a refused save, a conflict): `toAppError` passes it through. */
export function fail(code: string, message: string, extra: Record<string, unknown> = {}): never {
	throw Object.assign(new Error(message), { source: 'app' as const, code, message, ...extra });
}
