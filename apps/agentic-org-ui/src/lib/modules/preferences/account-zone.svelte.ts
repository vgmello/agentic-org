import { preferencesApi } from './api';
import { detectZone, effectiveZone, type ZoneStatus } from './time';

/**
 * The viewing account's time zone, shared by every screen that shows times (Home, Inbox, Activity, Preferences).
 * Until the saved zone has loaded, or when it cannot be read, times show in the detected zone, or in UTC marked
 * unsaved when the browser gives no valid zone (app-shell-ux "Account time display").
 */
class AccountZone {
	saved = $state<string | null | undefined>(undefined);
	detected = detectZone();
	loaded = $state(false);
	#organization: string | undefined;
	#loadSeq = 0;

	get effective(): { zone: string; status: ZoneStatus } {
		return effectiveZone(this.saved, this.detected);
	}
	get zone(): string {
		return this.effective.zone;
	}
	get status(): ZoneStatus {
		return this.effective.status;
	}

	/** Reads the saved zone once per organization; a refusal leaves the detected zone in use. */
	async ensure(organizationId: string): Promise<void> {
		if (this.#organization === organizationId) return;
		this.#organization = organizationId;
		const seq = ++this.#loadSeq;
		this.loaded = false;
		try {
			const res = await preferencesApi.timeZone(organizationId);
			if (seq !== this.#loadSeq || this.#organization !== organizationId) return;
			this.saved = res.saved;
		} catch {
			if (seq !== this.#loadSeq || this.#organization !== organizationId) return;
			this.saved = null;
		}
		this.loaded = true;
	}

	/** Saves a zone; resolves when it is kept, rejects with the reason when it is not. */
	async save(organizationId: string, zone: string): Promise<void> {
		await preferencesApi.saveTimeZone(organizationId, zone);
		if (this.#organization === organizationId) {
			this.#loadSeq += 1;
			this.saved = zone;
			this.loaded = true;
		}
	}
}

export const accountZone = new AccountZone();
