/**
 * Pages with editable text register here (SIGN-15, SIGN-19). The registry answers only
 * whether any registered draft currently holds unsaved text; it never stores the text.
 */
export type DraftProbe = () => boolean;

export function createDraftRegistry() {
	const probes = new Set<DraftProbe>();
	return Object.freeze({
		/** Registers a probe; the returned function removes it (call it when the page goes away). */
		register(probe: DraftProbe): () => void {
			probes.add(probe);
			return () => {
				probes.delete(probe);
			};
		},
		/** Whether this page has editable fields at all (the Not now bar's copy depends on it). */
		hasEditable: () => probes.size > 0,
		/** True only when a registered page reports unsaved text; a throwing probe counts as unsaved. */
		hasUnsavedText(): boolean {
			for (const probe of probes) {
				try {
					if (probe()) return true;
				} catch {
					return true;
				}
			}
			return false;
		}
	});
}
export type DraftRegistry = ReturnType<typeof createDraftRegistry>;

/** The tab's registry. */
export const drafts = createDraftRegistry();
