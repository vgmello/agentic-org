import type { PersonPanelDataSource, PersonPanelProfile, PersonPanelWorkItem } from './types';

function isAbortError(error: unknown): boolean {
	return (
		(error instanceof DOMException && error.name === 'AbortError') ||
		(error instanceof Error && error.name === 'AbortError')
	);
}

/**
 * Abort-safe, sequence-guarded loader for the lazily-mounted Person Side Panel.
 * Uses `$state.raw` for fetched profile/work snapshots and forwards `AbortSignal` end-to-end.
 */
export class PersonPanelLoader {
	#source: PersonPanelDataSource;
	profile = $state.raw<PersonPanelProfile | undefined>(undefined);
	openWork = $state.raw<readonly PersonPanelWorkItem[]>([]);
	totalOpenWork = $state(0);
	loading = $state(false);
	error = $state<string | undefined>(undefined);

	#seq = 0;
	#abort: AbortController | undefined;
	#disposed = false;

	constructor(source: PersonPanelDataSource) {
		this.#source = source;
	}

	get disposed(): boolean {
		return this.#disposed;
	}

	load(organizationId: string, memberId: string): { cancel: () => void; done: Promise<void> } {
		this.#abort?.abort();
		if (this.#disposed) {
			return { cancel: () => undefined, done: Promise.resolve() };
		}

		const controller = new AbortController();
		this.#abort = controller;
		const seq = ++this.#seq;
		this.profile = undefined;
		this.openWork = [];
		this.totalOpenWork = 0;
		this.error = undefined;
		this.loading = true;

		const done = this.#run(organizationId, memberId, seq, controller);
		return {
			cancel: () => {
				if (this.#abort === controller) {
					this.#abort = undefined;
				}
				controller.abort();
			},
			done
		};
	}

	async #run(
		organizationId: string,
		memberId: string,
		seq: number,
		controller: AbortController
	): Promise<void> {
		try {
			const result = await this.#source.loadMember(organizationId, memberId, controller.signal);
			if (this.#disposed || controller.signal.aborted || seq !== this.#seq) return;
			this.profile = result.profile;
			this.openWork = result.openWork;
			this.totalOpenWork = result.totalOpenWork;
			this.error = result.error;
		} catch (err) {
			if (this.#disposed || controller.signal.aborted || seq !== this.#seq || isAbortError(err)) {
				return;
			}
			this.profile = undefined;
			this.openWork = [];
			this.totalOpenWork = 0;
			this.error = err instanceof Error && err.message ? err.message : 'Could not load profile.';
		} finally {
			if (!this.#disposed && seq === this.#seq) {
				this.loading = false;
			}
			if (this.#abort === controller) {
				this.#abort = undefined;
			}
		}
	}

	cancel(): void {
		this.#seq += 1;
		this.#abort?.abort();
		this.#abort = undefined;
		this.loading = false;
	}

	dispose(): void {
		this.#disposed = true;
		this.cancel();
	}
}
