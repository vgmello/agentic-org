// Reactive state controller for an agent's BYOA external sign-ins and lease lifecycle (`TASK-249`).
// Zero `$effect`s: callers drive `load()` explicitly and call `dispose()` on unmount.
import { appSession } from '$lib/platform/auth/app-session.svelte';
import { asExternalAuthError, externalAuthApi } from './external-auth-http';
import type {
	ExternalAuthApi,
	ExternalAuthError,
	ExternalAuthRenewInput,
	ExternalAuthRevokeInput,
	ExternalAuthRevokeResult,
	ExternalSessionView,
	MemberExternalSessionsView
} from './external-auth-types';

export type ExternalAuthLoadStatus = 'idle' | 'loading' | 'ready' | 'unauthorized' | 'error';
export type ExternalAuthBusyAction = 'renew' | 'revoke';

export interface ExternalAuthControllerOptions {
	api?: ExternalAuthApi;
	onSessionEnded?: (listener: () => void) => () => void;
	historyLimit?: number;
}

export class ExternalAuthController {
	status = $state<ExternalAuthLoadStatus>('idle');
	data = $state.raw<MemberExternalSessionsView | undefined>(undefined);
	error = $state.raw<ExternalAuthError | undefined>(undefined);
	reloading = $state(false);
	busyAction = $state<ExternalAuthBusyAction | undefined>(undefined);
	actionError = $state.raw<ExternalAuthError | undefined>(undefined);

	readonly #api: ExternalAuthApi;
	readonly #historyLimit: number;
	#gen = 0;
	#actionGen = 0;
	#loadAbort: AbortController | undefined;
	#actionAbort: AbortController | undefined;
	#disposed = false;
	#target: { organizationId: string; memberId: string } | undefined;
	#stopSessionListener: (() => void) | undefined;

	constructor(options: ExternalAuthControllerOptions = {}) {
		this.#api = options.api ?? externalAuthApi;
		this.#historyLimit = options.historyLimit ?? 50;
		const subscribeSessionEnd =
			options.onSessionEnded ?? ((listener) => appSession.onSessionEnded(() => listener()));
		this.#stopSessionListener = subscribeSessionEnd(() => this.#resetForSessionEnd());
	}

	#isStaleLoad(gen: number, signal: AbortSignal): boolean {
		return this.#disposed || gen !== this.#gen || signal.aborted;
	}

	#isStaleAction(actionGen: number, signal: AbortSignal): boolean {
		return this.#disposed || actionGen !== this.#actionGen || signal.aborted;
	}

	#resetForSessionEnd(): void {
		this.#gen += 1;
		this.#actionGen += 1;
		this.#loadAbort?.abort();
		this.#loadAbort = undefined;
		this.#actionAbort?.abort();
		this.#actionAbort = undefined;
		this.#target = undefined;
		this.data = undefined;
		this.error = undefined;
		this.actionError = undefined;
		this.reloading = false;
		this.busyAction = undefined;
		this.status = 'idle';
	}

	clearActionError(): void {
		this.actionError = undefined;
	}

	#prepareTarget(organizationId: string, memberId: string, silent: boolean): void {
		const changed =
			this.#target?.organizationId !== organizationId || this.#target?.memberId !== memberId;
		this.#target = { organizationId, memberId };
		if (changed) {
			this.#actionGen += 1;
			this.#actionAbort?.abort();
			this.#actionAbort = undefined;
			this.busyAction = undefined;
			this.data = undefined;
			this.error = undefined;
			this.actionError = undefined;
			this.status = 'loading';
		} else if (!silent && this.data === undefined) {
			this.error = undefined;
			this.status = 'loading';
		}
	}

	#targetMatches(expected: { organizationId: string; memberId?: string }): boolean {
		if (!this.#target || this.#target.organizationId !== expected.organizationId) return false;
		return expected.memberId === undefined || this.#target.memberId === expected.memberId;
	}

	#applyLoadFailure(err: unknown): void {
		const parsed = asExternalAuthError(err, 'External sessions could not be loaded.');
		this.error = parsed;
		this.data = undefined;
		this.status =
			parsed.code === 'not_authorized' || parsed.code === 'owner_protected'
				? 'unauthorized'
				: 'error';
	}

	async load(
		organizationId: string,
		memberId: string,
		options?: { silent?: boolean }
	): Promise<void> {
		if (this.#disposed) return;
		this.#loadAbort?.abort();
		const abort = new AbortController();
		this.#loadAbort = abort;
		const gen = ++this.#gen;
		this.#prepareTarget(organizationId, memberId, Boolean(options?.silent));
		this.reloading = true;
		try {
			const next = await this.#api.memberSessions(organizationId, memberId, {
				limit: this.#historyLimit,
				signal: abort.signal
			});
			if (this.#isStaleLoad(gen, abort.signal)) return;
			this.data = Object.freeze({
				...next,
				pending: Object.freeze([...next.pending]) as MemberExternalSessionsView['pending'],
				history: Object.freeze([...next.history]) as MemberExternalSessionsView['history']
			});
			this.error = undefined;
			this.status = 'ready';
		} catch (err) {
			if (!this.#isStaleLoad(gen, abort.signal)) this.#applyLoadFailure(err);
		} finally {
			if (this.#loadAbort === abort) this.#loadAbort = undefined;
			if (gen === this.#gen) this.reloading = false;
		}
	}

	async reload(): Promise<void> {
		if (this.#disposed || !this.#target) return;
		await this.load(this.#target.organizationId, this.#target.memberId, {
			silent: this.data !== undefined
		});
	}

	async #runAction<T>(
		kind: ExternalAuthBusyAction,
		fallbackMessage: string,
		expectedTarget: { organizationId: string; memberId?: string },
		execute: (signal: AbortSignal) => Promise<T>,
		options?: { reload?: boolean; recordActionError?: boolean }
	): Promise<T> {
		if (this.#disposed) {
			throw {
				source: 'api',
				code: 'unavailable',
				message: 'Controller has been disposed.'
			} satisfies ExternalAuthError;
		}
		this.#actionAbort?.abort();
		const abort = new AbortController();
		this.#actionAbort = abort;
		const actionGen = ++this.#actionGen;
		this.busyAction = kind;
		this.actionError = undefined;
		const shouldReload = options?.reload ?? true;
		const recordError = options?.recordActionError ?? true;
		try {
			const result = await execute(abort.signal);
			if (
				shouldReload &&
				!this.#isStaleAction(actionGen, abort.signal) &&
				this.#targetMatches(expectedTarget)
			) {
				await this.reload();
			}
			return result;
		} catch (err) {
			const parsed = asExternalAuthError(err, fallbackMessage);
			if (
				recordError &&
				!this.#isStaleAction(actionGen, abort.signal) &&
				this.#targetMatches(expectedTarget)
			) {
				this.actionError = parsed;
			}
			throw parsed;
		} finally {
			if (this.#actionAbort === abort) this.#actionAbort = undefined;
			if (actionGen === this.#actionGen) this.busyAction = undefined;
		}
	}

	renew(
		organizationId: string,
		memberId: string,
		input?: ExternalAuthRenewInput,
		options?: { reload?: boolean; recordActionError?: boolean }
	): Promise<ExternalSessionView> {
		return this.#runAction(
			'renew',
			'External lease renewal was refused.',
			{ organizationId, memberId },
			(signal) => this.#api.renew(organizationId, memberId, input, signal),
			options
		);
	}

	revoke(
		organizationId: string,
		memberId: string,
		input?: ExternalAuthRevokeInput,
		options?: { reload?: boolean; recordActionError?: boolean }
	): Promise<ExternalAuthRevokeResult> {
		return this.#runAction(
			'revoke',
			'External lease revocation was refused.',
			{ organizationId, memberId },
			(signal) => this.#api.revoke(organizationId, memberId, input, signal),
			options
		);
	}

	dispose(): void {
		this.#disposed = true;
		this.#gen += 1;
		this.#actionGen += 1;
		this.#loadAbort?.abort();
		this.#loadAbort = undefined;
		this.#actionAbort?.abort();
		this.#actionAbort = undefined;
		this.#stopSessionListener?.();
		this.#stopSessionListener = undefined;
		this.reloading = false;
		this.busyAction = undefined;
	}
}
