import { toAppError } from '$lib/office/api';
import type { AppError } from '$lib/proto';
import type { CopilotApi } from './api';
import type { DraftResponse, FeatureId, FeatureShapes } from './types';

/** Where one ask of the copilot is. Every failure ends in `failed`; nothing is thrown. */
export type DraftPhase<D> =
	| { kind: 'idle' }
	| { kind: 'drafting' }
	| { kind: 'ready'; response: DraftResponse<D> }
	| { kind: 'failed'; error: AppError };

export const isDrafting = (p: DraftPhase<unknown>) => p.kind === 'drafting';

/**
 * One ask at a time for a feature. `run` is ignored while one is going (even if called twice in the
 * same tick); `cancel` forgets the ask in flight, so its late answer is dropped instead of
 * appearing on a screen that has moved on.
 */
export function createDraftFlow<F extends FeatureId>(
	api: CopilotApi,
	feature: F,
	report: (phase: DraftPhase<FeatureShapes[F]['draft']>) => void
) {
	let current = 0;
	let running = false;
	return {
		async run(input: FeatureShapes[F]['input']): Promise<void> {
			if (running) return;
			running = true;
			const mine = ++current;
			report({ kind: 'drafting' });
			try {
				const response = await api.draft(feature, input);
				if (mine === current) report({ kind: 'ready', response });
			} catch (e) {
				if (mine === current) report({ kind: 'failed', error: toAppError(e) });
			} finally {
				if (mine === current) running = false;
			}
		},
		/** Drop the ask in flight (its answer is ignored) and go back to `idle`. */
		cancel(): void {
			current++;
			running = false;
			report({ kind: 'idle' });
		}
	};
}
