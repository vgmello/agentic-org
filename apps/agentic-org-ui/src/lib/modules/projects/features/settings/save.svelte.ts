// One Settings section's save (app-shell-ux §6a, offices-and-teams §7.10): busy while HQ answers; a refusal is kept
// with the field it names; a revision conflict re-reads the project, keeps what was typed and says so. Success is
// reported only after HQ accepted the change and the project was read again.
import { toAppError } from '$lib/modules/offices/office-api';
import { conflictText } from '../../present';
import type { ProjectField } from '../../types';

export interface Refusal {
	field?: ProjectField;
	message: string;
}

export class SectionSave {
	busy = $state(false);
	refusal = $state<Refusal | undefined>();
	/** The last save landed: "Saved." beside the button until the next edit. */
	saved = $state(false);

	readonly #reload: () => Promise<void>;

	constructor(reload: () => Promise<void>) {
		this.#reload = reload;
	}

	/** Runs `change`; true when HQ accepted it. */
	async run(change: () => Promise<void>, projectName: string): Promise<boolean> {
		if (this.busy) return false;
		this.busy = true;
		this.refusal = undefined;
		this.saved = false;
		try {
			await change();
			await this.#reload();
			this.saved = true;
			return true;
		} catch (e) {
			const failure = toAppError(e);
			const field = (failure as { field?: ProjectField }).field;
			// Whatever HQ answered, show the project as it is now, never as it was guessed to be: a refusal about one
			// field changed nothing, anything else is read again (the typed values are kept by the section).
			if (!field) await this.#reload().catch(() => {});
			this.refusal =
				failure.code === 'conflict'
					? { message: conflictText(projectName) }
					: { field, message: failure.message };
			return false;
		} finally {
			this.busy = false;
		}
	}

	edited() {
		this.saved = false;
		this.refusal = undefined;
	}
}
