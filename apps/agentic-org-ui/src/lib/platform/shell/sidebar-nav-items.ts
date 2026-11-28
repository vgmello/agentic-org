import Activity from '@lucide/svelte/icons/activity';
import BarChart3 from '@lucide/svelte/icons/bar-chart-3';
import ClipboardList from '@lucide/svelte/icons/clipboard-list';
import Folder from '@lucide/svelte/icons/folder';
import MessageSquare from '@lucide/svelte/icons/message-square';
import Settings from '@lucide/svelte/icons/settings';
import Users from '@lucide/svelte/icons/users';
import Workflow from '@lucide/svelte/icons/workflow';
import {
	ORG_NAV_IDS,
	WORKSPACE_NAV_IDS,
	type OrgNavId,
	type WorkspaceNavId
} from '$lib/platform/workspace/ui-preferences.svelte';

export interface PinnableNavItem<TId extends string = string> {
	id: TId;
	href:
		| '/work'
		| '/chat'
		| '/activity'
		| '/projects'
		| '/insights'
		| '/teams'
		| '/settings'
		| '/settings/processes';
	label: string;
	Icon: typeof ClipboardList;
}

export interface PinnableNavSectionConfig<TId extends string = string> {
	sectionLabel: string;
	sectionTestId: string;
	moreToggleTestId: string;
	moreListTestId: string;
	moreListAriaLabel: string;
	zoneKind: 'ws' | 'org';
	allIds: readonly TId[];
	meta: Record<TId, PinnableNavItem<TId>>;
}

export const WORKSPACE_SECTION_CONFIG: PinnableNavSectionConfig<WorkspaceNavId> = {
	sectionLabel: 'Workspace',
	sectionTestId: 'workspace-nav-section',
	moreToggleTestId: 'workspace-more-toggle',
	moreListTestId: 'workspace-more-list',
	moreListAriaLabel: 'More workspace items',
	zoneKind: 'ws',
	allIds: WORKSPACE_NAV_IDS,
	meta: {
		work: { id: 'work', href: '/work', label: 'Work', Icon: ClipboardList },
		chat: { id: 'chat', href: '/chat', label: 'Chat', Icon: MessageSquare },
		activity: { id: 'activity', href: '/activity', label: 'Activity', Icon: Activity },
		projects: { id: 'projects', href: '/projects', label: 'Projects', Icon: Folder },
		insights: { id: 'insights', href: '/insights', label: 'Insights', Icon: BarChart3 }
	}
};

export const ORG_SECTION_CONFIG: PinnableNavSectionConfig<OrgNavId> = {
	sectionLabel: 'Organization',
	sectionTestId: 'organization-nav-section',
	moreToggleTestId: 'organization-more-toggle',
	moreListTestId: 'organization-more-list',
	moreListAriaLabel: 'More organization pages',
	zoneKind: 'org',
	allIds: ORG_NAV_IDS,
	meta: {
		teams: { id: 'teams', href: '/teams', label: 'Teams', Icon: Users },
		settings: { id: 'settings', href: '/settings', label: 'Settings', Icon: Settings },
		processes: {
			id: 'processes',
			href: '/settings/processes',
			label: 'Processes',
			Icon: Workflow
		}
	}
};

function matchesRoutePrefix(pathname: string, prefix: string): boolean {
	return pathname === prefix || pathname.startsWith(`${prefix}/`);
}

export function isOrgNavItemActive(id: OrgNavId, pathname: string): boolean {
	if (id === 'teams') {
		return matchesRoutePrefix(pathname, '/teams') || matchesRoutePrefix(pathname, '/members');
	}
	if (id === 'processes') {
		return matchesRoutePrefix(pathname, '/settings/processes');
	}
	if (matchesRoutePrefix(pathname, '/settings/processes')) return false;
	return matchesRoutePrefix(pathname, '/settings') || matchesRoutePrefix(pathname, '/preferences');
}
