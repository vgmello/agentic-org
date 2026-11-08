import type { OrgChangedNotice, Position } from '$lib/proto';

/** Whether `a` is after `b` in the organization's log: `(epoch, seq)`, compared in that order. */
export const isAfter = (a: Position, b: Position): boolean => a.epoch > b.epoch || (a.epoch === b.epoch && a.seq > b.seq);
const same = (a: Position | undefined, b: Position | undefined) => a?.epoch === b?.epoch && a?.seq === b?.seq;

// Per organization: the position the screens last loaded, the newest the office has told us about, which reload
// to run when the screens are behind, and whether one is running now.
const shown = new Map<string, Position>();
const heard = new Map<string, Position>();
const reloaders = new Map<string, () => Promise<unknown>>();
const running = new Set<string>();
// When each organization's structure was last loaded.
const loadedAt = new Map<string, Date>();

/** When the screens last loaded the organization's structure, if they have. */
export const lastLoadedAt = (organizationId: string): Date | undefined => loadedAt.get(organizationId);

const behind = (organizationId: string): boolean => {
	const seen = shown.get(organizationId);
	const told = heard.get(organizationId);
	return seen !== undefined && told !== undefined && isAfter(told, seen);
};

/**
 * Reload until the screens show what the office last told us, one reload at a time: a burst of changes costs one
 * reload and perhaps a second, not one per change. A reload that shows nothing newer (it failed, or the office is
 * behind) ends the loop and forgets what it was told; the next notice starts it again.
 */
function catchUp(organizationId: string): void {
	const reload = reloaders.get(organizationId);
	if (!reload || running.has(organizationId)) return;
	running.add(organizationId);
	void (async () => {
		try {
			while (behind(organizationId)) {
				const before = shown.get(organizationId);
				const told = heard.get(organizationId);
				await reload();
				if (same(shown.get(organizationId), before) && same(heard.get(organizationId), told)) {
					// Nothing newer to show, and nothing newer told meanwhile: the office is behind what it told us (or was
					// restored). Take what is shown as the truth, or every later load would try again.
					heard.set(organizationId, shown.get(organizationId)!);
					break;
				}
			}
		} catch {
			// The next notice tries again.
		} finally {
			running.delete(organizationId);
		}
	})();
}

/** A screen loaded the organization's structure at this position (always the latest load: a restored office may be at an earlier one). */
export function noteShown(organizationId: string, position: Position): void {
	shown.set(organizationId, position);
	loadedAt.set(organizationId, new Date());
	// A change told while this was loading is not shown yet.
	if (behind(organizationId)) catchUp(organizationId);
}

/**
 * The office reported a change. Our own commands are reported too, and by then the screens usually show them: only a
 * position after the one shown is news. An organization never loaded has nothing to refresh. `reload` loads the
 * structure again (it ends in `noteShown`).
 */
export function noteChanged(notice: OrgChangedNotice, reload: () => Promise<unknown>): void {
	const { organization_id: id, position } = notice;
	reloaders.set(id, reload);
	const told = heard.get(id);
	if (!told || isAfter(position, told)) heard.set(id, position);
	if (behind(id)) catchUp(id);
}
