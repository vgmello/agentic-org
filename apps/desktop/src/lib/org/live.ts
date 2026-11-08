import type { OrgChangedNotice, Position } from '$lib/proto';

/** Whether `a` is after `b` in the organization's log: `(epoch, seq)`, compared in that order. */
export const isAfter = (a: Position, b: Position): boolean => a.epoch > b.epoch || (a.epoch === b.epoch && a.seq > b.seq);

// The position of the structure each organization's screens last loaded.
const shown = new Map<string, Position>();

/** A screen loaded the organization's structure at this position (always the latest load: a restored office may be at an earlier one). */
export function noteShown(organizationId: string, position: Position): void {
	shown.set(organizationId, position);
}

/**
 * Whether the screens are behind a change the office reported. Our own commands are reported too, and by then the
 * screens usually show them: only a position after the one shown is news. An organization never loaded has nothing to refresh.
 */
export function shouldReload(notice: OrgChangedNotice): boolean {
	const position = shown.get(notice.organization_id);
	return position !== undefined && isAfter(notice.position, position);
}
