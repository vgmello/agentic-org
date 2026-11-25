/**
 * Pages with editable text register here (SIGN-15, SIGN-19). The registry answers only
 * whether any registered draft currently holds unsaved text; it never stores the text.
 */
export type DraftProbe = () => boolean;

export function createDraftRegistry() {
	const probes = new Set<DraftProbe>();
	const retainedProbes = new Set<DraftProbe>();
	return Object.freeze({
		/** Registers a mounted editable page/editor probe (counts toward both `hasEditable` and `hasUnsavedText`). */
		register(probe: DraftProbe): () => void {
			probes.add(probe);
			return () => {
				probes.delete(probe);
			};
		},
		/** Registers an off-screen retained-draft probe (counts toward SIGN-19 sign-out confirmation, not page `hasEditable`). */
		registerRetained(probe: DraftProbe): () => void {
			retainedProbes.add(probe);
			return () => {
				retainedProbes.delete(probe);
			};
		},
		/** Whether this page currently has mounted editable fields (the Not now bar's copy depends on it). */
		hasEditable: () => probes.size > 0,
		/** True only when a mounted editor or retained draft reports unsaved text; a throwing probe counts as unsaved. */
		hasUnsavedText(): boolean {
			for (const probe of [...probes, ...retainedProbes]) {
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
