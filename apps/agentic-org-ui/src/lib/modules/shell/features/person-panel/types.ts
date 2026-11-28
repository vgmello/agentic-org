export type PersonPanelPresenceTone = 'ok' | 'warn' | 'accent' | 'muted';

export interface PersonPanelWorkItem {
	id: string;
	ref: string;
	title: string;
	statusLabel: string;
	href: string;
}

export interface PersonPanelProfile {
	id: string;
	name: string;
	kind: 'agent' | 'person';
	kindLabel: string;
	owner: boolean;
	isBuiltIn?: boolean;
	role?: string;
	status: string;
	isSelf: boolean;
	presenceLabel?: string;
	presenceTone?: PersonPanelPresenceTone;
	currentTaskTitle?: string;
	homeTeam?: { id: string; name: string };
	profileHref: string;
	workTabHref: string;
}

export interface PersonPanelLoadedData {
	profile?: PersonPanelProfile;
	openWork: readonly PersonPanelWorkItem[];
	totalOpenWork: number;
	openWorkUnavailable?: boolean;
	error?: string;
}

/**
 * Consumer-owned port for loading read-only member facts and open work items into PersonSidePanel.
 * Implemented in `platform/shell/person-panel-source.ts` via public non-UI module barrels so
 * `module:shell` stays cycle-free below `lib-root:nav`.
 */
export interface PersonPanelDataSource {
	loadMember(
		organizationId: string,
		memberId: string,
		signal: AbortSignal
	): Promise<PersonPanelLoadedData>;
}
