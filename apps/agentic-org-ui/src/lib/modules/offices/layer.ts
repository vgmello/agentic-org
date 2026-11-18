// The offices the screen lists: the connection's own, with the offices that joined layered beside them and any new names laid over.
import type { OfficeInfo } from '$lib/proto';
import type { OfficesOverview, SatelliteDetails } from './types';
import { agoText } from './time';

export function layerOffices(
	base: OfficeInfo[],
	overview: Pick<OfficesOverview, 'satellites' | 'names'> | undefined
): OfficeInfo[] {
	if (!overview) return base;
	const known = new Set(base.map((o) => o.id));
	return [...base, ...overview.satellites.filter((o) => !known.has(o.id))].map((o) =>
		overview.names[o.id] ? { ...o, name: overview.names[o.id] } : o
	);
}

/** The line under an office that joined: where it is, with freshness (§3.1: "last seen 2 h ago", "copying records"). */
export function satelliteWhere(
	office: OfficeInfo,
	details: SatelliteDetails | undefined,
	now: number
): string {
	if (!details) return office.host;
	if (office.status === 'offline')
		return details.lastSeenAt ? `last seen ${agoText(details.lastSeenAt, now)}` : details.where;
	if (office.status === 'starting') return `${details.where} · copying records`;
	return details.where;
}

/** "Up to date" or "Catching up: 34 changes". */
export const syncText = (d: SatelliteDetails): string =>
	d.sync.kind === 'up_to_date'
		? 'Up to date'
		: `Catching up: ${d.sync.changes} ${d.sync.changes === 1 ? 'change' : 'changes'}`;
