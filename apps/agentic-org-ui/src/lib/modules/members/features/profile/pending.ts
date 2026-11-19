// While a requested turn waits for the office (`accepted`), the profile is read again so the header's presence,
// Health and the Runs tab move together when the office starts it. Reads back off: 2 s, 4 s, 8 s… up to 30 s.
export const FIRST_POLL_MS = 2000;
export const MAX_POLL_MS = 30_000;

export const nextPollDelay = (previous: number) => Math.min(previous * 2, MAX_POLL_MS);

/** Whether the profile shows a turn the office hasn't started yet. */
export const awaitingOffice = (p: { openTurn?: { state: string } } | null | undefined) =>
	p?.openTurn?.state === 'accepted';

/**
 * Orders the profile's reads (the page's load, the pending-turn checks, Run now's answer): each gets a number when it
 * starts, and an answer is applied only if no newer one has been, so an older read finishing late never replaces a
 * newer profile.
 */
export class LatestRead {
	#issued = 0;
	#applied = 0;
	/** A read starts: its number. */
	start(): number {
		return ++this.#issued;
	}
	/** Whether read `n`'s answer may be shown; if so, it becomes the newest shown. */
	accept(n: number): boolean {
		if (n < this.#applied) return false;
		this.#applied = n;
		return true;
	}
	/** An answer that arrived outside a read (Run now's profile): newest from now on. */
	now(): void {
		this.accept(this.start());
	}
}
