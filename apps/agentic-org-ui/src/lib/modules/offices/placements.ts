// The organization's agents with the placements this screen knows of laid over the record's own (an agent moved out of a
// removed office is Unplaced until someone places it, §6.4).
import type { OrgSnapshot } from '$lib/proto';

export function withPlacements(
	snapshot: OrgSnapshot,
	placements: Record<string, string | null>
): OrgSnapshot {
	if (Object.keys(placements).length === 0) return snapshot;
	return {
		...snapshot,
		members: snapshot.members.map((m) => {
			if (!Object.hasOwn(placements, m.id)) return m;
			const where = placements[m.id];
			const rest = { ...m };
			delete rest.office_id;
			return where ? { ...rest, office_id: where } : rest;
		})
	};
}
