import { createContext } from 'svelte';
import type { OfficeInfo, OrgSnapshot, ProjectTeam, WorkItemSummary } from '$lib/proto';
import type { InboxItem } from './inbox-types';
import type { ProjectHealthOverlay } from './project-health';

export interface ShellRefreshResult {
	inboxUpdated: boolean;
	workUpdated: boolean;
}

export interface ShellDataContext {
	readonly organizationId: string;
	readonly snapshot: OrgSnapshot | undefined;
	readonly workItems: WorkItemSummary[];
	readonly workError: unknown;
	readonly workErrorSeq: number;
	readonly inboxItems: InboxItem[];
	readonly inboxLoadedOrganizationId: string | undefined;
	readonly loaded: boolean;
	readonly loadedOrganizationId: string | undefined;
	readonly ownerId: string;
	readonly openInboxCount: number;
	readonly needsYouCount: number;
	readonly offices: OfficeInfo[];
	readonly validProjectTeams: readonly ProjectTeam[] | undefined;
	readonly projectHealthById: Readonly<Record<string, ProjectHealthOverlay>>;
	setProjectHealthOverlays(
		organizationId: string,
		overlays: Readonly<Record<string, ProjectHealthOverlay>>
	): void;
	load(id?: string, reset?: boolean): { promise: Promise<ShellRefreshResult>; cancel: () => void };
	refresh(): Promise<ShellRefreshResult>;
	dispose(): void;
}

const [getShellDataContext, setShellDataContext] = createContext<ShellDataContext>();

export { setShellDataContext };

export function useShellData(): ShellDataContext {
	return getShellDataContext();
}
