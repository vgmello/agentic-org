import type { InboxItem } from '$lib/modules/inbox/types';
import { nonOwners } from '$lib/modules/organization/model';
import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
import { areaCapability } from './capabilities';
import type { NewMenuAction } from './NewMenu.svelte';
import type { ShellAiAskOptions } from './shell-ai';
import { UI_THEMES, type UiTheme } from './ui-preferences.svelte';

export interface PaletteConversation {
	id: string;
	kind: string;
	name?: string;
	archived?: boolean;
	auto?: { type: 'general' } | { type: 'project'; project_id: string } | { type: 'team'; team_id: string };
}

export type CommandIcon =
	| 'plus'
	| 'folder'
	| 'sparkles'
	| 'message'
	| 'user-plus'
	| 'panel-left'
	| 'palette'
	| 'keyboard'
	| 'house'
	| 'inbox'
	| 'clipboard'
	| 'activity'
	| 'users'
	| 'building'
	| 'dollar'
	| 'settings'
	| 'hash'
	| 'search';

export interface CommandEntry {
	id: string;
	group: string;
	tag: string;
	icon: CommandIcon;
	label: string;
	sub: string;
	keywords?: string;
	href?: string;
	action?: () => void;
}

/** Matches messaging/rules.ts `slug()` (NFKD + diacritic stripping) for fallback before conversations load. */
export function channelSlug(value: string): string {
	return value
		.normalize('NFKD')
		.replace(/[\u0300-\u036f]/g, '')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '');
}

function channelSubtitle(c: PaletteConversation, snapshot: OrgSnapshot | undefined): string {
	if (c.auto?.type === 'general') return 'Organization channel';
	if (c.auto?.type === 'project') {
		const projectId = c.auto.project_id;
		const name = (snapshot?.projects ?? []).find((p) => p.id === projectId)?.name;
		return name ? `Project channel · ${name}` : 'Project channel';
	}
	if (c.auto?.type === 'team') {
		const teamId = c.auto.team_id;
		const name = (snapshot?.teams ?? []).find((t) => t.id === teamId)?.name;
		return name ? `Team channel · ${name}` : 'Team channel';
	}
	return 'Channel';
}

export function buildChannelEntries(
	conversations: readonly PaletteConversation[] | undefined,
	conversationsMatchedOrg: boolean,
	snapshot: OrgSnapshot | undefined
): CommandEntry[] {
	if (conversations && conversationsMatchedOrg) {
		return conversations
			.filter((c) => c.kind === 'channel' && !c.archived && Boolean(c.name))
			.map((c) => {
				const name = c.name!;
				const sub = channelSubtitle(c, snapshot);
				return {
					id: `chan-${c.id}`,
					group: 'Channels',
					tag: 'Channel',
					icon: 'hash',
					label: `#${name}`,
					sub,
					keywords: `${c.id} ${name} ${sub}`,
					href: `/chat?c=${encodeURIComponent(c.id)}`
				};
			});
	}

	const channels: CommandEntry[] = [
		{
			id: 'chan-c-general',
			group: 'Channels',
			tag: 'Channel',
			icon: 'hash',
			label: '#general',
			sub: 'Organization channel',
			keywords: 'general organization all chat',
			href: '/chat?c=c-general'
		}
	];
	if (!snapshot) return channels;

	const usedNames: Record<string, true> = { general: true };
	const takeName = (raw: string, fallbackKind: 'project' | 'team') => {
		let name = raw || 'channel';
		if (usedNames[name]) {
			const base = `${name}-${fallbackKind}`;
			name = base;
			for (let n = 2; usedNames[name]; n++) name = `${base}-${n}`;
		}
		usedNames[name] = true;
		return name;
	};

	for (const p of (snapshot.projects ?? []).filter((x) => !x.archived)) {
		const name = takeName(channelSlug(p.name) || channelSlug(p.key ?? ''), 'project');
		channels.push({
			id: `chan-c-project-${p.id}`,
			group: 'Channels',
			tag: 'Channel',
			icon: 'hash',
			label: `#${name}`,
			sub: `Project channel · ${p.name}`,
			keywords: `${p.id} ${p.name} ${p.key ?? ''} ${name}`,
			href: `/chat?c=${encodeURIComponent(`c-project-${p.id}`)}`
		});
	}
	for (const t of snapshot.teams ?? []) {
		const name = takeName(channelSlug(t.name), 'team');
		channels.push({
			id: `chan-c-team-${t.id}`,
			group: 'Channels',
			tag: 'Channel',
			icon: 'hash',
			label: `#${name}`,
			sub: `Team channel · ${t.name}`,
			keywords: `${t.id} ${t.name} ${name}`,
			href: `/chat?c=${encodeURIComponent(`c-team-${t.id}`)}`
		});
	}
	return channels;
}

export function buildActionEntries(
	onNewAction: (kind: NewMenuAction, fallbackHref?: string) => void
): CommandEntry[] {
	const defs: Array<{ route: string; entry: CommandEntry }> = [
		{
			route: '/work',
			entry: {
				id: 'act-new-task',
				group: 'Actions',
				tag: 'Command',
				icon: 'plus',
				label: 'New task',
				sub: 'Create a work item, bug, or request',
				keywords: 'create work item task bug request new',
				action: () => onNewAction('task', '/work?new=1')
			}
		},
		{
			route: '/projects',
			entry: {
				id: 'act-new-project',
				group: 'Actions',
				tag: 'Wizard',
				icon: 'folder',
				label: 'New project',
				sub: 'Start a project wizard to group work',
				keywords: 'create project wizard new',
				action: () => onNewAction('project', '/projects?new=1')
			}
		},
		{
			route: '/teams/hire',
			entry: {
				id: 'act-hire-agent',
				group: 'Actions',
				tag: 'Wizard',
				icon: 'sparkles',
				label: 'Hire an agent',
				sub: 'Describe role, review suggestion, hire',
				keywords: 'hire agent hr employee role specialist new',
				action: () => onNewAction('hire', '/teams/hire')
			}
		},
		{
			route: '/chat',
			entry: {
				id: 'act-new-message',
				group: 'Actions',
				tag: 'Command',
				icon: 'message',
				label: 'New message',
				sub: 'Start a conversation in Chat',
				keywords: 'dm chat message send talk new',
				action: () => onNewAction('message', '/chat')
			}
		},
		{
			route: '/teams/members',
			entry: {
				id: 'act-add-person',
				group: 'Actions',
				tag: 'Command',
				icon: 'user-plus',
				label: 'Add a person',
				sub: 'Invite a teammate to this organization',
				keywords: 'invite teammate person member human new',
				action: () => onNewAction('person', '/teams/members')
			}
		}
	];
	return defs.filter((d) => areaCapability(d.route).available).map((d) => d.entry);
}

export function buildAskTessEntries(
	onAskTess: (prompt: string, opts?: ShellAiAskOptions) => void
): CommandEntry[] {
	return [
		{
			id: 'tess-needs-me',
			group: 'Ask Tess',
			tag: 'AI',
			icon: 'sparkles',
			label: 'Ask: What needs me first?',
			sub: 'Summarize active blockers and pending sign-offs',
			keywords: 'tess chief of staff blockers inbox urgent approvals',
			action: () => onAskTess('What needs me first?')
		},
		{
			id: 'tess-budget-offices',
			group: 'Ask Tess',
			tag: 'AI',
			icon: 'sparkles',
			label: 'Ask: Budget, offices & active agents',
			sub: 'Check monthly spend, offices & agent activity',
			keywords: 'tess spend cost budget capacity offices idle agents',
			action: () => onAskTess('Budget, offices & active agents')
		},
		{
			id: 'tess-projects',
			group: 'Ask Tess',
			tag: 'AI',
			icon: 'sparkles',
			label: 'Ask: Which projects need attention?',
			sub: 'Review project health, blockers, and delivery pace',
			keywords: 'tess projects slow risk behind off track',
			action: () => onAskTess('Which projects need attention?')
		},
		{
			id: 'tess-task-time',
			group: 'Ask Tess',
			tag: 'AI',
			icon: 'sparkles',
			label: 'Ask: Where is task time going?',
			sub: 'Inspect open work distribution and queue pressure',
			keywords: 'tess task time queue bottleneck capacity',
			action: () => onAskTess('Where is task time going?')
		}
	];
}

export function buildNavigateEntries(openInboxCount: number): CommandEntry[] {
	const all: CommandEntry[] = [
		{
			id: 'go-home',
			group: 'Navigate',
			tag: 'Page',
			icon: 'house',
			label: 'Home',
			sub: 'Executive briefing & Spotlight',
			keywords: 'go to home overview index',
			href: '/home'
		},
		{
			id: 'go-inbox',
			group: 'Navigate',
			tag: 'Page',
			icon: 'inbox',
			label: 'Inbox',
			sub: openInboxCount > 0 ? `${openInboxCount} waiting on you` : 'Decisions waiting on you',
			keywords: 'go to inbox approvals triage',
			href: '/inbox'
		},
		{
			id: 'go-work',
			group: 'Navigate',
			tag: 'Page',
			icon: 'clipboard',
			label: 'Work',
			sub: 'Tasks, bugs & requests',
			keywords: 'go to work tasks bugs requests board',
			href: '/work'
		},
		{
			id: 'go-chat',
			group: 'Navigate',
			tag: 'Page',
			icon: 'message',
			label: 'Chat',
			sub: 'Team channels & direct messages',
			keywords: 'go to chat messages channels dm',
			href: '/chat'
		},
		{
			id: 'go-activity',
			group: 'Navigate',
			tag: 'Page',
			icon: 'activity',
			label: 'Activity',
			sub: 'Live runs & organization timeline',
			keywords: 'go to activity runs feed log',
			href: '/activity'
		},
		{
			id: 'go-projects',
			group: 'Navigate',
			tag: 'Page',
			icon: 'folder',
			label: 'Projects',
			sub: 'Portfolio health & teams',
			keywords: 'go to projects portfolio',
			href: '/projects'
		},
		{
			id: 'go-teams',
			group: 'Navigate',
			tag: 'Page',
			icon: 'users',
			label: 'Teams',
			sub: 'Teams & project ownership',
			keywords: 'go to teams org chart',
			href: '/teams'
		},
		{
			id: 'go-members',
			group: 'Navigate',
			tag: 'Page',
			icon: 'users',
			label: 'Members',
			sub: 'People, agents & presence',
			keywords: 'go to members people agents directory',
			href: '/teams/members'
		},
		{
			id: 'go-offices',
			group: 'Navigate',
			tag: 'Page',
			icon: 'building',
			label: 'Offices',
			sub: 'Connected machines & status',
			keywords: 'go to offices machines hq satellite',
			href: '/settings/offices'
		},
		{
			id: 'go-costs',
			group: 'Navigate',
			tag: 'Page',
			icon: 'dollar',
			label: 'Costs',
			sub: 'Monthly spend & agent budgets',
			keywords: 'go to costs billing budgets spend',
			href: '/settings/costs'
		},
		{
			id: 'go-settings',
			group: 'Navigate',
			tag: 'Page',
			icon: 'settings',
			label: 'Settings',
			sub: 'Organization & appearance themes',
			keywords: 'go to settings preferences',
			href: '/settings'
		}
	];
	return all.filter((e) => !e.href || areaCapability(e.href).available);
}

export function buildWorkspaceEntries(input: {
	navCollapsed: boolean;
	showTessPill: boolean;
	onToggleNav: () => void;
	onToggleTessPill: () => void;
	onOpenShortcuts?: () => void;
	onSetTheme: (id: UiTheme) => void;
}): CommandEntry[] {
	return [
		{
			id: 'ws-toggle-sidebar',
			group: 'Workspace',
			tag: 'Command',
			icon: 'panel-left',
			label: 'Toggle sidebar',
			sub: input.navCollapsed
				? 'Pin the navigation sidebar open'
				: 'Auto-hide the navigation rail',
			keywords: 'collapse expand sidebar rail navigation',
			action: input.onToggleNav
		},
		{
			id: 'ws-toggle-tess-pill',
			group: 'Workspace',
			tag: 'Command',
			icon: 'sparkles',
			label: input.showTessPill ? 'Hide Tess button on screen' : 'Show Tess button on screen',
			sub: 'Toggle the floating bottom-right Tess pill (⌘/ still opens Tess anytime)',
			keywords: 'tess companion pill floating button show hide',
			action: input.onToggleTessPill
		},
		...(input.onOpenShortcuts
			? [
					{
						id: 'ws-shortcuts',
						group: 'Workspace',
						tag: 'Help',
						icon: 'keyboard' as const,
						label: 'Keyboard shortcuts',
						sub: 'Show all global, navigation, and composer shortcuts (?)',
						keywords: 'help keys hotkeys cheatsheet ?',
						action: input.onOpenShortcuts
					}
				]
			: []),
		...UI_THEMES.map(
			(t): CommandEntry => ({
				id: `theme-${t.id}`,
				group: 'Workspace',
				tag: 'Theme',
				icon: 'palette',
				label: `Theme: ${t.label}`,
				sub: t.description,
				keywords: `appearance theme ${t.id} ${t.shortLabel}`,
				action: () => input.onSetTheme(t.id)
			})
		)
	];
}

export function buildDynamicEntries(
	snapshot: OrgSnapshot | undefined,
	inboxItems: readonly InboxItem[],
	items: readonly WorkItemSummary[]
): CommandEntry[] {
	const out: CommandEntry[] = [];
	if (snapshot) {
		for (const p of (snapshot.projects ?? []).filter((x) => !x.archived)) {
			out.push({
				id: `proj-${p.id}`,
				group: 'Projects',
				tag: 'Project',
				icon: 'folder',
				label: p.name,
				sub: p.key ? `Project · ${p.key.toUpperCase()}` : 'Project',
				keywords: `${p.id} ${p.key ?? ''}`,
				href: `/projects/${p.id}/work`
			});
		}
		for (const m of nonOwners(snapshot)) {
			out.push({
				id: `mem-${m.id}`,
				group: 'Team',
				tag: 'Member',
				icon: 'users',
				label: m.name,
				sub: m.agent?.title ?? m.agent?.role ?? m.kind,
				keywords: `${m.id} ${m.agent?.role ?? ''} ${m.agent?.messaging_name ?? ''}`,
				href: `/members/${m.id}`
			});
		}
	}
	for (const inboxItem of inboxItems) {
		out.push({
			id: `inbox-${inboxItem.id}`,
			group: 'Inbox',
			tag: 'Inbox',
			icon: 'inbox',
			label: inboxItem.title,
			sub: `${inboxItem.source} · ${inboxItem.who.name}`,
			keywords: `${inboxItem.id} ${inboxItem.project?.name ?? ''} ${inboxItem.why}`,
			href:
				inboxItem.state === 'handled'
					? `/inbox?tab=handled&item=${encodeURIComponent(inboxItem.id)}`
					: `/inbox?item=${encodeURIComponent(inboxItem.id)}`
		});
	}
	for (const item of items.filter((i) => !i.deleted)) {
		const dest =
			item.kind === 'request' || !item.project_id
				? `/work/${encodeURIComponent(item.short_id)}`
				: `/projects/${encodeURIComponent(item.project_id)}/work`;
		out.push({
			id: `work-${item.id}`,
			group: 'Work',
			tag: 'Work',
			icon: 'clipboard',
			label: item.title,
			sub: `${item.short_id} · ${item.status.replace('_', ' ')}`,
			keywords: `${item.short_id} ${item.kind} ${item.id}`,
			href: dest
		});
	}
	return out;
}

export function entryMatchesQuery(entry: CommandEntry, q: string): boolean {
	return (
		entry.label.toLowerCase().includes(q) ||
		entry.sub.toLowerCase().includes(q) ||
		entry.group.toLowerCase().includes(q) ||
		entry.tag.toLowerCase().includes(q) ||
		Boolean(entry.keywords?.toLowerCase().includes(q))
	);
}

export function computeCommandEntries(input: {
	query: string;
	isChat: boolean;
	modalOpen: boolean;
	channelEntries: readonly CommandEntry[];
	baseList: readonly CommandEntry[];
	dynamicEntries: readonly CommandEntry[];
	onOpenModalWithQuery: (q: string) => void;
	onAskTess: (prompt: string) => void;
}): CommandEntry[] {
	const trimmed = input.query.trim();
	if (trimmed.startsWith('#')) {
		const cq = trimmed.slice(1).trim().toLowerCase();
		return cq
			? input.channelEntries.filter((e) => entryMatchesQuery(e, cq))
			: [...input.channelEntries];
	}
	const q = trimmed.toLowerCase();
	if (!q) return [...input.baseList];

	const searchable = input.isChat
		? [...input.channelEntries, ...input.baseList, ...input.dynamicEntries]
		: [...input.baseList, ...input.dynamicEntries, ...input.channelEntries];
	const allMatched = searchable.filter((e) => entryMatchesQuery(e, q));
	const shown =
		!input.modalOpen && allMatched.length > 11 ? allMatched.slice(0, 11) : [...allMatched];

	if (!input.modalOpen && allMatched.length > 11) {
		shown.push({
			id: 'show-all-matches',
			group: 'Search',
			tag: 'All',
			icon: 'search',
			label: `Show all ${allMatched.length} matches`,
			sub: 'Open full searchable results list',
			action: () => input.onOpenModalWithQuery(trimmed)
		});
	}
	shown.push({
		id: 'ask-tess-fallback',
		group: 'Ask Tess',
		tag: 'AI',
		icon: 'sparkles',
		label: `Ask Tess about '${trimmed}'`,
		sub: 'Open Tess Chief of Staff panel for this question',
		action: () => input.onAskTess(trimmed)
	});
	return shown;
}
