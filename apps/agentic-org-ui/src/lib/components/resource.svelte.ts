// One read for a screen part, with the states every place shows (app-shell-ux §6a): loading (a skeleton),
// unavailable (the production adapter does not serve it yet: never sample data), error (what failed, with Retry)
// and ready. It loads when created; a later `reload` keeps the shown data until the new answer arrives.
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
			this.message = '';
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
