// One read for a screen part, with the states every place shows (app-shell-ux §6a): loading (a skeleton),
// unavailable (the web API does not serve it yet: never sample data), error (what failed, with Retry) and ready.
import { toAppError } from '$lib/office/api';
import { UNAVAILABLE_MESSAGE } from '$lib/platform/unavailable-api';

export type ResourceState = 'loading' | 'unavailable' | 'error' | 'ready';

export interface Resource<T> {
	readonly state: ResourceState;
	readonly value: T | undefined;
	readonly error: string | undefined;
	readonly retrying: boolean;
	/** Load (again). A reload keeps the shown value until the new one arrives. */
	load(): Promise<void>;
}

export function resource<T>(fetch: () => Promise<T>): Resource<T> {
	let state = $state<ResourceState>('loading');
	let value = $state.raw<T | undefined>();
	let error = $state<string | undefined>();
	let retrying = $state(false);
	let request = 0;
	return {
		get state() {
			return state;
		},
		get value() {
			return value;
		},
		get error() {
			return error;
		},
		get retrying() {
			return retrying;
		},
		async load() {
			const mine = ++request;
			if (state === 'error') retrying = true;
			else if (value === undefined) state = 'loading';
			try {
				const next = await fetch();
				if (mine !== request) return;
				value = next;
				state = 'ready';
				error = undefined;
			} catch (e) {
				if (mine !== request) return;
				const failure = toAppError(e);
				error = failure.message;
				state = failure.message === UNAVAILABLE_MESSAGE ? 'unavailable' : 'error';
			} finally {
				if (mine === request) retrying = false;
			}
		}
	};
}
