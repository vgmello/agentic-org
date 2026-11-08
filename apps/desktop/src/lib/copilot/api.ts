import type { AppError } from '$lib/proto';
import { mockCopilot } from './mock';
import type { CopilotFeature, DraftResponse, FeatureId, FeatureShapes } from './types';

/**
 * What the UI needs from the office's copilot. The copilot only drafts: a result is a value to show
 * in an editable form, and nothing is saved or sent until the person confirms in the normal way.
 */
export interface CopilotApi {
	/** True while the answers come from the TEMPORARY mock, not from the user's agent runtime. */
	readonly sample: boolean;
	/** `copilot.features`. Empty when no runtime is set up: the UI then hides every smart button. */
	features(): Promise<CopilotFeature[]>;
	/** `copilot.draft`. Rejects with an `AppError` whose code is one of `COPILOT_ERROR_CODES`. */
	draft<F extends FeatureId>(
		feature: F,
		input: FeatureShapes[F]['input']
	): Promise<DraftResponse<FeatureShapes[F]['draft']>>;
}

/**
 * TEMPORARY: until the office answers `copilot.*` (the Rust shell then calls it through `org-client`),
 * the browser and the app both use this TypeScript mock. `?copilot=off` in the URL turns the
 * copilot off, as on a Mac with no runtime set up.
 */
export const copilotApi: CopilotApi = mockCopilot;

const cache = new WeakMap<CopilotApi, Promise<CopilotFeature[]>>();

/**
 * The features the copilot offers, asked once per API and shared by every smart button on the
 * screen. A failure counts as "none" (the buttons stay hidden) and is not remembered, so the next
 * screen asks again.
 */
export function copilotFeatures(api: CopilotApi): Promise<CopilotFeature[]> {
	let pending = cache.get(api);
	if (!pending) {
		pending = api.features().catch(() => {
			cache.delete(api);
			return [];
		});
		cache.set(api, pending);
	}
	return pending;
}

/** Forget what `copilotFeatures` remembered, e.g. after the person sets up a runtime. */
export function forgetCopilotFeatures(api: CopilotApi): void {
	cache.delete(api);
}

export const offers = (features: CopilotFeature[] | undefined, id: FeatureId): boolean =>
	features?.some((f) => f.id === id) ?? false;

/** The headline and advice for a failed draft, in the person's words. */
export function describeCopilotFailure(error: AppError): { title: string; hint: string } {
	switch (error.code) {
		case 'copilot_unavailable':
			return { title: "The copilot isn't available", hint: 'No agent runtime is set up on this Mac. You can still write it yourself.' };
		case 'copilot_failed':
			return { title: "The copilot couldn't draft this", hint: "Its answer wasn't usable. Try again, or write it yourself." };
		case 'timeout':
			return { title: 'The copilot took too long', hint: 'Try again, or write it yourself.' };
		case 'copilot_busy':
			return { title: 'Still working on your last request', hint: 'Wait for it to finish, then try again.' };
		case 'invalid_request':
			return { title: "The copilot can't draft this", hint: error.message };
		default:
			return { title: 'Something went wrong', hint: error.message };
	}
}
