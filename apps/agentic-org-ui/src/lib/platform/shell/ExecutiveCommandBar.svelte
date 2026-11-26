<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { afterNavigate, goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import Activity from '@lucide/svelte/icons/activity';
	import Building2 from '@lucide/svelte/icons/building-2';
	import ClipboardList from '@lucide/svelte/icons/clipboard-list';
	import DollarSign from '@lucide/svelte/icons/dollar-sign';
	import Folder from '@lucide/svelte/icons/folder';
	import Hash from '@lucide/svelte/icons/hash';
	import House from '@lucide/svelte/icons/house';
	import Inbox from '@lucide/svelte/icons/inbox';
	import Keyboard from '@lucide/svelte/icons/keyboard';
	import MessageSquare from '@lucide/svelte/icons/message-square';
	import Palette from '@lucide/svelte/icons/palette';
	import PanelLeft from '@lucide/svelte/icons/panel-left';
	import Plus from '@lucide/svelte/icons/plus';
	import Search from '@lucide/svelte/icons/search';
	import Settings from '@lucide/svelte/icons/settings';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import UserPlus from '@lucide/svelte/icons/user-plus';
	import Users from '@lucide/svelte/icons/users';
	import * as Dialog from '$lib/components/ui/dialog';
	import { cn } from '$lib/utils';
	import type { Connection, OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import type { InboxItem } from '$lib/modules/inbox/types';
	import type { Conversation } from '$lib/modules/messaging/types';
	import { nonOwners } from '$lib/modules/organization/model';
	import NewMenu, { type NewMenuAction } from './NewMenu.svelte';
	import NotificationsButton from './NotificationsButton.svelte';
	import { areaCapability } from './capabilities';
	import type { ShellAiAskOptions } from './shell-ai';
	import { useShellData } from './shell-data.svelte';
	import { isEditableTarget, singleKeyShortcutsOn } from './shortcuts';
	import { UI_THEMES, uiPrefs } from './ui-preferences.svelte';

	let {
		connection,
		snapshot,
		items = [],
		inboxItems = [],
		offices = [],
		ownerId = 'owner',
		offline = false,
		onToggleTess,
		onToggleTessExpand,
		onOpenShortcuts,
		onAskTess
	}: {
		connection?: Connection;
		snapshot?: OrgSnapshot;
		items?: WorkItemSummary[];
		inboxItems?: readonly InboxItem[];
		offices?: OfficeInfo[];
		ownerId?: string;
		offline?: boolean;
		onToggleTess?: () => void;
		onToggleTessExpand?: () => void;
		onOpenShortcuts?: () => void;
		onAskTess?: (prompt: string, opts?: ShellAiAskOptions) => void;
	} = $props();

	let newMenu = $state<ReturnType<typeof NewMenu> | undefined>();

	export function openNewTask() {
		newMenu?.runAction('task');
	}

	type CommandIcon =
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

	interface CommandEntry {
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

	const kbdClass = 'exec-cmdk-foot-kbd';

	const shellData = useShellData();
	const organizationId = $derived(shellData.organizationId);
	const isHome = $derived(page.url.pathname === '/home');
	const isChat = $derived(page.url.pathname === '/chat' || page.url.pathname.startsWith('/chat/'));
	const openInboxCount = $derived(inboxItems.filter((item) => item.state === 'open').length);

	let chatConversations = $state.raw<Conversation[] | undefined>(undefined);
	let chatConversationsOrgId = $state<string | undefined>(undefined);
	let chatLoadSeq = 0;
	let destroyed = false;

	onDestroy(() => {
		destroyed = true;
	});

	// Keep unused props typed cleanly for callers while Tess panel reads the same shell context.
	$effect(() => {
		void offices;
		void ownerId;
	});

	async function loadChatConversations(targetOrgId = organizationId) {
		if (!targetOrgId) return;
		const seq = ++chatLoadSeq;
		try {
			const { chatApi } = await import('$lib/modules/messaging/api');
			const snap = await chatApi.snapshot(targetOrgId);
			if (destroyed || seq !== chatLoadSeq || targetOrgId !== organizationId) return;
			if (!activeEntryId && (open || inlineOpen) && query.trim().length > 0 && safeActiveIdx >= 0) {
				const currentEntry = entries[safeActiveIdx];
				if (currentEntry) activeEntryId = currentEntry.id;
			}
			chatConversations = snap.state.conversations;
			chatConversationsOrgId = targetOrgId;
		} catch {
			// Unserved or offline chat falls back to snapshot-derived channel suggestions.
		}
	}

	/** Matches messaging/engine.ts `slug()` (NFKD + diacritic stripping) for fallback before conversations load. */
	function channelSlug(value: string): string {
		return value
			.normalize('NFKD')
			.replace(/[\u0300-\u036f]/g, '')
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '-')
			.replace(/^-+|-+$/g, '');
	}

	function channelSubtitle(c: Conversation): string {
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

	const channelEntries = $derived.by<CommandEntry[]>(() => {
		if (chatConversations && chatConversationsOrgId === organizationId) {
			return chatConversations
				.filter((c) => c.kind === 'channel' && !c.archived && Boolean(c.name))
				.map((c) => {
					const name = c.name!;
					const sub = channelSubtitle(c);
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
	});

	let open = $state(false);
	let modalMode = $state<'cmd' | 'ai'>('cmd');
	let inlineOpen = $state(false);
	let query = $state('');
	const isChannelQuery = $derived(query.trimStart().startsWith('#'));
	let activeIdx = $state(0);
	let activeEntryId = $state<string | undefined>(undefined);
	let inputEl = $state<HTMLInputElement | null>(null);
	let homeInputEl = $state<HTMLInputElement | null>(null);
	let homeShellEl = $state<HTMLDivElement | null>(null);
	let skipRestoreFocus = false;

	function triggerNewAction(kind: NewMenuAction, fallbackHref?: string) {
		if (newMenu) {
			newMenu.runAction(kind);
			return;
		}
		if (fallbackHref) {
			void goto(resolve(fallbackHref as '/'));
		}
	}

	function handoffToTess(prompt: string, opts?: ShellAiAskOptions) {
		closeInline(true);
		closeModal(false);
		onAskTess?.(prompt, opts);
	}

	function buildActionEntries(): CommandEntry[] {
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
					action: () => triggerNewAction('task', '/work?new=1')
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
					action: () => triggerNewAction('project', '/projects?new=1')
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
					action: () => triggerNewAction('hire', '/teams/hire')
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
					action: () => triggerNewAction('message', '/chat')
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
					action: () => triggerNewAction('person', '/teams/members')
				}
			}
		];
		return defs.filter((d) => areaCapability(d.route).available).map((d) => d.entry);
	}

	function buildAskTessEntries(): CommandEntry[] {
		return [
			{
				id: 'tess-needs-me',
				group: 'Ask Tess',
				tag: 'AI',
				icon: 'sparkles',
				label: 'Ask: What needs me first?',
				sub: 'Summarize active blockers and pending sign-offs',
				keywords: 'tess chief of staff blockers inbox urgent approvals',
				action: () => handoffToTess('What needs me first?')
			},
			{
				id: 'tess-budget-offices',
				group: 'Ask Tess',
				tag: 'AI',
				icon: 'sparkles',
				label: 'Ask: Budget, offices & active agents',
				sub: 'Check monthly spend, offices & agent activity',
				keywords: 'tess spend cost budget capacity offices idle agents',
				action: () => handoffToTess('Budget, offices & active agents')
			},
			{
				id: 'tess-projects',
				group: 'Ask Tess',
				tag: 'AI',
				icon: 'sparkles',
				label: 'Ask: Which projects need attention?',
				sub: 'Review project health, blockers, and delivery pace',
				keywords: 'tess projects slow risk behind off track',
				action: () => handoffToTess('Which projects need attention?')
			},
			{
				id: 'tess-task-time',
				group: 'Ask Tess',
				tag: 'AI',
				icon: 'sparkles',
				label: 'Ask: Where is task time going?',
				sub: 'Inspect open work distribution and queue pressure',
				keywords: 'tess task time queue bottleneck capacity',
				action: () => handoffToTess('Where is task time going?')
			}
		];
	}

	function buildNavigateEntries(): CommandEntry[] {
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

	function buildWorkspaceEntries(): CommandEntry[] {
		return [
			{
				id: 'ws-toggle-sidebar',
				group: 'Workspace',
				tag: 'Command',
				icon: 'panel-left',
				label: 'Toggle sidebar',
				sub: uiPrefs.navCollapsed
					? 'Pin the navigation sidebar open'
					: 'Auto-hide the navigation rail',
				keywords: 'collapse expand sidebar rail navigation',
				action: () => uiPrefs.toggleNavCollapsed()
			},
			{
				id: 'ws-toggle-tess-pill',
				group: 'Workspace',
				tag: 'Command',
				icon: 'sparkles',
				label: uiPrefs.showTessPill ? 'Hide Tess button on screen' : 'Show Tess button on screen',
				sub: 'Toggle the floating bottom-right Tess pill (⌘/ still opens Tess anytime)',
				keywords: 'tess companion pill floating button show hide',
				action: () => uiPrefs.toggleShowTessPill()
			},
			...(onOpenShortcuts
				? [
						{
							id: 'ws-shortcuts',
							group: 'Workspace',
							tag: 'Help',
							icon: 'keyboard' as const,
							label: 'Keyboard shortcuts',
							sub: 'Show all global, navigation, and composer shortcuts (?)',
							keywords: 'help keys hotkeys cheatsheet ?',
							action: () => onOpenShortcuts()
						}
					]
				: []),
			...UI_THEMES.map((t): CommandEntry => ({
				id: `theme-${t.id}`,
				group: 'Workspace',
				tag: 'Theme',
				icon: 'palette',
				label: `Theme: ${t.label}`,
				sub: t.description,
				keywords: `appearance theme ${t.id} ${t.shortLabel}`,
				action: () => uiPrefs.setTheme(t.id)
			}))
		];
	}

	function buildDynamicEntries(): CommandEntry[] {
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

	function entryMatchesQuery(entry: CommandEntry, q: string): boolean {
		return (
			entry.label.toLowerCase().includes(q) ||
			entry.sub.toLowerCase().includes(q) ||
			entry.group.toLowerCase().includes(q) ||
			entry.tag.toLowerCase().includes(q) ||
			Boolean(entry.keywords?.toLowerCase().includes(q))
		);
	}

	const entries = $derived.by<CommandEntry[]>(() => {
		const trimmed = query.trim();
		if (trimmed.startsWith('#')) {
			const cq = trimmed.slice(1).trim().toLowerCase();
			return cq ? channelEntries.filter((e) => entryMatchesQuery(e, cq)) : channelEntries;
		}
		const baseList: CommandEntry[] = [
			...buildActionEntries(),
			...buildAskTessEntries(),
			...buildNavigateEntries(),
			...buildWorkspaceEntries()
		];
		const q = trimmed.toLowerCase();
		if (!q) return baseList;

		const dynamicEntries = buildDynamicEntries();
		const searchable = isChat
			? [...channelEntries, ...baseList, ...dynamicEntries]
			: [...baseList, ...dynamicEntries, ...channelEntries];
		const allMatched = searchable.filter((e) => entryMatchesQuery(e, q));
		const shown = !open && allMatched.length > 11 ? allMatched.slice(0, 11) : [...allMatched];

		if (!open && allMatched.length > 11) {
			shown.push({
				id: 'show-all-matches',
				group: 'Search',
				tag: 'All',
				icon: 'search',
				label: `Show all ${allMatched.length} matches`,
				sub: 'Open full searchable results list',
				action: () => openModalWithQuery(trimmed)
			});
		}
		shown.push({
			id: 'ask-tess-fallback',
			group: 'Ask Tess',
			tag: 'AI',
			icon: 'sparkles',
			label: `Ask Tess about '${trimmed}'`,
			sub: 'Open Tess Chief of Staff panel for this question',
			action: () => handoffToTess(trimmed)
		});
		return shown;
	});

	const safeActiveIdx = $derived.by(() => {
		if (entries.length === 0) return -1;
		if (activeEntryId) {
			return entries.findIndex((e) => e.id === activeEntryId);
		}
		return Math.min(activeIdx, entries.length - 1);
	});
	const activeOptionId = $derived(
		open && safeActiveIdx >= 0 ? `exec-cmdk-opt-${safeActiveIdx}` : undefined
	);
	const inlineListboxVisible = $derived(isHome && inlineOpen && entries.length > 0);
	const inlineActiveOptionId = $derived(
		inlineListboxVisible && safeActiveIdx >= 0 ? `exec-cmdk-home-opt-${safeActiveIdx}` : undefined
	);

	function setActiveIndex(idx: number) {
		activeIdx = idx;
		activeEntryId = entries[idx]?.id;
	}

	function resetActive() {
		activeIdx = 0;
		activeEntryId = undefined;
	}

	afterNavigate(() => {
		if (inlineOpen) {
			closeInline(true);
		}
	});

	function resetModalState() {
		query = '';
		modalMode = 'cmd';
		resetActive();
	}

	function closeInline(clearQuery = true) {
		inlineOpen = false;
		if (clearQuery) {
			query = '';
		}
		resetActive();
	}

	function openInline() {
		open = false;
		inlineOpen = true;
		resetActive();
		void loadChatConversations(organizationId);
		void tick().then(() => homeInputEl?.focus());
	}

	export function openPalette(prefill?: string) {
		if (prefill && prefill.trim().length > 0) {
			openModalWithQuery(prefill.trim());
		} else {
			openModal();
		}
	}

	async function findTessDmId(orgId: string): Promise<string | undefined> {
		try {
			const { chatApi } = await import('$lib/modules/messaging/api');
			const snap = await chatApi.snapshot(orgId);
			const tessMember = (snapshot?.members ?? []).find((m) => isTessMember(m));
			const tessDm = snap.state.conversations.find(
				(c) =>
					c.kind === 'dm' &&
					!c.archived &&
					(Boolean(tessMember && c.members.includes(tessMember.id)) ||
						Boolean(c.name?.toLowerCase().includes('tess')))
			);
			return tessDm?.id;
		} catch {
			return undefined;
		}
	}

	function isTessMember(m: NonNullable<OrgSnapshot>['members'][number]): boolean {
		return (
			m.name.toLowerCase().includes('tess') ||
			m.agent?.messaging_name?.toLowerCase() === 'tess' ||
			Boolean(m.agent?.role?.toLowerCase().includes('chief of staff'))
		);
	}

	export async function openTessChat() {
		const tessDmId = organizationId ? await findTessDmId(organizationId) : undefined;
		if (tessDmId) {
			const base = resolve('/chat');
			// eslint-disable-next-line svelte/no-navigation-without-resolve -- base passes through resolve() while query string stays intact
			await goto(`${base}?c=${encodeURIComponent(tessDmId)}`);
			return;
		}
		await goto(resolve('/chat'));
	}

	function openModal() {
		closeInline(false);
		skipRestoreFocus = false;
		open = true;
		resetActive();
		void loadChatConversations(organizationId);
	}

	function openModalWithQuery(nextQuery: string) {
		closeInline(false);
		skipRestoreFocus = false;
		query = nextQuery;
		open = true;
		resetActive();
		void loadChatConversations(organizationId);
		void tick().then(() => {
			if (!inputEl) return;
			inputEl.focus();
			inputEl.setSelectionRange(nextQuery.length, nextQuery.length);
		});
	}

	function closeModal(restoreFocus = true) {
		skipRestoreFocus = !restoreFocus;
		open = false;
		resetModalState();
	}

	function runEntry(entry: CommandEntry) {
		const fromInline = inlineOpen;
		if (fromInline) {
			closeInline(true);
		} else {
			closeModal(!entry.href);
		}
		if (entry.action) entry.action();
		else if (entry.href) void goto(resolve(entry.href as '/'));
	}

	function handleWindowPointerDown(event: PointerEvent) {
		if (!inlineOpen) return;
		const target = event.target;
		if (target instanceof Node && homeShellEl?.contains(target)) return;
		closeInline(false);
	}

	function hasBlockingOverlay(target: EventTarget | null): boolean {
		const overlaySelector =
			'[role="menu"], [role="dialog"]:not(.work-reading), [role="alertdialog"], [role="listbox"], aside[aria-label="Thread"], aside[aria-label="Members"], [data-chat-panel], [data-slot="sheet-content"]';
		if (document.querySelector(overlaySelector)) return true;
		return (
			target instanceof HTMLElement &&
			Boolean(
				target.closest(
					'[role="menu"], [role="dialog"], [role="alertdialog"], [role="listbox"], aside[aria-label], [data-chat-panel], [data-slot="sheet-content"]'
				)
			)
		);
	}

	function toggleCommandPaletteFromShortcut() {
		if (open) {
			closeModal();
			return;
		}
		if (isHome && inlineOpen && document.activeElement === homeInputEl) {
			closeInline(false);
			homeInputEl?.blur();
			return;
		}
		if (isHome) openInline();
		else openModal();
	}

	function toggleTessFromShortcut() {
		if (open) {
			modalMode = modalMode === 'cmd' ? 'ai' : 'cmd';
			void tick().then(() => inputEl?.focus());
			return;
		}
		if (inlineOpen) closeInline(false);
		onToggleTess?.();
	}

	function handleModShortcut(event: KeyboardEvent): boolean {
		if (!event.metaKey && !event.ctrlKey) return false;
		const lower = event.key.toLowerCase();
		if (lower === 'k') {
			event.preventDefault();
			toggleCommandPaletteFromShortcut();
			return true;
		}
		if (event.key === '/') {
			event.preventDefault();
			toggleTessFromShortcut();
			return true;
		}
		if (event.shiftKey && (lower === 'e' || lower === 'm')) {
			event.preventDefault();
			if (open) closeModal(false);
			if (inlineOpen) closeInline(false);
			onToggleTessExpand?.();
			return true;
		}
		return false;
	}

	function isChatHashShortcut(event: KeyboardEvent): boolean {
		if (!isChat || event.key !== '#' || event.metaKey) return false;
		const altGr = Boolean(event.getModifierState?.('AltGraph'));
		return altGr || (!event.ctrlKey && !event.altKey);
	}

	function handleSingleKeyShortcut(event: KeyboardEvent) {
		if (isChatHashShortcut(event)) {
			event.preventDefault();
			openModalWithQuery('#');
			return;
		}
		if (event.metaKey || event.ctrlKey || event.altKey) return;
		if (!inlineOpen && connection && event.key.toLowerCase() === 'n') {
			event.preventDefault();
			newMenu?.openMenu();
			return;
		}
		if (event.key === '/' && !event.shiftKey) {
			event.preventDefault();
			if (isHome) openInline();
			else openModal();
		}
	}

	function handleWindowKeydown(event: KeyboardEvent) {
		if (event.isComposing || event.keyCode === 229) return;
		if (handleModShortcut(event)) return;
		const blocked =
			open || event.repeat || isEditableTarget(event.target) || hasBlockingOverlay(event.target);
		if (blocked) return;
		if (event.key === '?' && !event.metaKey && !event.ctrlKey && !event.altKey) {
			event.preventDefault();
			onOpenShortcuts?.();
			return;
		}
		if (!singleKeyShortcutsOn()) return;
		handleSingleKeyShortcut(event);
	}

	$effect(() => {
		if (!open && !inlineListboxVisible) return;
		if (entries.length === 0 || safeActiveIdx < 0) return;
		void query;
		const targetId = inlineListboxVisible
			? `exec-cmdk-home-opt-${safeActiveIdx}`
			: `exec-cmdk-opt-${safeActiveIdx}`;
		let cancelled = false;
		void tick().then(() => {
			if (!cancelled) {
				document.getElementById(targetId)?.scrollIntoView({ block: 'nearest' });
			}
		});
		return () => {
			cancelled = true;
		};
	});

	function stepActiveIndex(delta: 1 | -1) {
		const defaultIdx = delta === 1 ? 0 : entries.length - 1;
		if (isHome && !open && !inlineOpen) {
			inlineOpen = true;
			if (entries.length > 0) setActiveIndex(defaultIdx);
			else resetActive();
			return;
		}
		if (entries.length === 0) return;
		const next =
			safeActiveIdx < 0 ? defaultIdx : (safeActiveIdx + delta + entries.length) % entries.length;
		setActiveIndex(next);
	}

	function handleEnterKey(event: KeyboardEvent) {
		if (open && modalMode === 'ai') {
			const q = query.trim();
			if (q) {
				event.preventDefault();
				handoffToTess(q);
			}
			return;
		}
		const canRun = open || (isHome && !open && inlineOpen);
		if (canRun && safeActiveIdx >= 0 && entries[safeActiveIdx]) {
			event.preventDefault();
			runEntry(entries[safeActiveIdx]);
		}
	}

	function exitHomeSearch() {
		const heading = document.querySelector<HTMLElement>('#shell-page-title, main h1, main h2');
		if (heading) {
			if (!heading.hasAttribute('tabindex')) heading.tabIndex = -1;
			heading.focus({ preventScroll: true });
		} else {
			homeInputEl?.blur();
		}
	}

	function handleInlineEscape(event: KeyboardEvent) {
		event.preventDefault();
		event.stopPropagation();
		if (inlineOpen) {
			closeInline(false);
			if (query.length === 0) exitHomeSearch();
			return;
		}
		exitHomeSearch();
	}

	function handleInputKeydown(event: KeyboardEvent) {
		if (event.isComposing || event.keyCode === 229) return;
		if (event.key === 'Tab' && isHome && !open && inlineOpen) {
			inlineOpen = false;
			return;
		}
		if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
			event.preventDefault();
			stepActiveIndex(event.key === 'ArrowDown' ? 1 : -1);
		} else if (event.key === 'Enter') {
			handleEnterKey(event);
		} else if (event.key === 'Escape' && isHome && !open) {
			handleInlineEscape(event);
		}
	}
</script>

{#snippet renderIcon(icon: CommandIcon)}
	{#if icon === 'plus'}
		<Plus class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'folder'}
		<Folder class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'sparkles'}
		<Sparkles class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'message'}
		<MessageSquare class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'user-plus'}
		<UserPlus class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'panel-left'}
		<PanelLeft class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'palette'}
		<Palette class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'keyboard'}
		<Keyboard class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'house'}
		<House class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'inbox'}
		<Inbox class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'clipboard'}
		<ClipboardList class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'activity'}
		<Activity class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'users'}
		<Users class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'building'}
		<Building2 class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'dollar'}
		<DollarSign class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'settings'}
		<Settings class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else if icon === 'hash'}
		<Hash class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{:else}
		<Search class="exec-cmdk-row-ico size-4 shrink-0" aria-hidden="true" />
	{/if}
{/snippet}

{#snippet renderEntries(listboxId: string, optPrefix: string, preventBlur: boolean)}
	{#if entries.length === 0}
		<div class="flex flex-col items-center gap-2 py-7 text-center">
			<p class="text-sm text-[var(--exec-muted)]">
				{isChannelQuery ? `No channels matching “${query.trim()}”.` : `No matches for “${query}”.`}
			</p>
			{#if query.trim() && !isChannelQuery}
				<button
					type="button"
					onmousedown={preventBlur ? (e) => e.preventDefault() : undefined}
					onclick={() => handoffToTess(query)}
					class="inline-flex min-h-9 items-center gap-1.5 rounded-lg bg-[var(--exec-accent-soft)] px-3 text-xs font-semibold text-[var(--exec-accent-ink)] shadow-[inset_0_0_0_1px_var(--exec-accent-ring)]"
				>
					<Sparkles class="size-3.5" aria-hidden="true" />
					<span>Ask Tess “{query.trim()}”</span>
				</button>
			{/if}
		</div>
	{:else}
		<ul
			id={listboxId}
			class="exec-cmdk-list flex flex-col gap-[3px]"
			role="listbox"
			aria-label="Search results"
		>
			{#each entries as entry, idx (entry.id)}
				{@const showGroup = idx === 0 || entries[idx - 1].group !== entry.group}
				{#if showGroup}
					<li role="presentation" class={['exec-cmdk-grp', idx === 0 && 'is-first']}>
						{entry.group}
					</li>
				{/if}
				<li role="presentation">
					<button
						type="button"
						id="{optPrefix}-{idx}"
						role="option"
						tabindex={-1}
						aria-selected={idx === safeActiveIdx}
						onmousedown={preventBlur ? (e) => e.preventDefault() : undefined}
						onmouseenter={() => setActiveIndex(idx)}
						onclick={() => runEntry(entry)}
						class={[
							'exec-cmdk-opt flex w-full items-center text-left',
							idx === safeActiveIdx && 'is-active'
						]}
					>
						{@render renderIcon(entry.icon)}
						<span
							class="exec-cmdk-opt-txt flex min-w-0 flex-1 items-baseline gap-2 overflow-hidden"
						>
							<span class="exec-cmdk-opt-lbl shrink-0">{entry.label}</span>
							<span class="exec-cmdk-opt-sub hidden min-w-0 truncate sm:inline">
								{entry.sub}
							</span>
						</span>
						<span class="exec-cmdk-opt-tag shrink-0">
							{entry.tag}
						</span>
					</button>
				</li>
			{/each}
		</ul>
	{/if}
{/snippet}

<svelte:window onkeydown={handleWindowKeydown} onpointerdown={handleWindowPointerDown} />

{#if isHome}
	<header class="exec-topbar exec-home-topbar relative z-30 shrink-0 overflow-visible px-4 sm:px-6">
		<div
			bind:this={homeShellEl}
			class="exec-home-cmdk relative flex min-w-0 w-full max-w-[640px] flex-col"
		>
			<div class="exec-cmdk-bar flex w-full items-center">
				<label
					class="exec-cmdk-trigger exec-cmdk-home-field flex min-w-0 flex-1 items-center text-left"
				>
					<Search class="exec-cmdk-ico size-[18px] shrink-0" aria-hidden="true" />
					<input
						bind:this={homeInputEl}
						bind:value={query}
						type="search"
						autocomplete="off"
						placeholder="Search, run a command, or ask Tess…"
						aria-label="Search, run a command, or ask Tess"
						role="combobox"
						aria-autocomplete="list"
						aria-expanded={inlineListboxVisible}
						aria-controls={inlineListboxVisible ? 'exec-cmdk-home-listbox' : undefined}
						aria-activedescendant={inlineActiveOptionId}
						onfocus={() => {
							inlineOpen = true;
							resetActive();
							void loadChatConversations(organizationId);
						}}
						oninput={(event) => {
							inlineOpen = true;
							resetActive();
							if (
								event.currentTarget.value.trimStart().startsWith('#') &&
								chatConversationsOrgId !== organizationId
							) {
								void loadChatConversations(organizationId);
							}
						}}
						onkeydown={handleInputKeydown}
						class="exec-cmdk-home-input min-w-0 flex-1 bg-transparent text-sm text-[var(--exec-ink)] outline-none"
					/>
					<kbd class="exec-cmdk-kbd hidden sm:inline-flex" aria-hidden="true">⌘K</kbd>
				</label>
			</div>

			{#if inlineOpen}
				<div class="exec-cmdk-home-results exec-cmdk-surface flex flex-col overflow-hidden">
					<div class="min-h-0 flex-1 overflow-y-auto p-2.5">
						{@render renderEntries('exec-cmdk-home-listbox', 'exec-cmdk-home-opt', true)}
					</div>
					<div class="exec-cmdk-foot" aria-hidden="true">
						<span><kbd class={kbdClass}>↑</kbd><kbd class={kbdClass}>↓</kbd> Navigate</span>
						<span><kbd class={kbdClass}>↵</kbd> Run / Ask</span>
						<span><kbd class={kbdClass}>⌘/</kbd> Ask Tess</span>
						<span><kbd class={kbdClass}>Esc</kbd> Close</span>
					</div>
				</div>
			{/if}
		</div>
		<div class="exec-home-utils">
			<NotificationsButton {offline} />
		</div>
	</header>
{/if}
{#if connection}
	<NewMenu bind:this={newMenu} {connection} headless />
{/if}

<Dialog.Root
	bind:open
	onOpenChange={(next) => {
		if (!next) resetModalState();
	}}
>
	<Dialog.Content
		showCloseButton={false}
		overlayClass="exec-cmdk-overlay"
		onOpenAutoFocus={(event) => {
			event.preventDefault();
			inputEl?.focus();
			if (inputEl && query.length > 0) {
				inputEl.setSelectionRange(query.length, query.length);
			}
		}}
		onCloseAutoFocus={(event) => {
			if (skipRestoreFocus) {
				event.preventDefault();
				skipRestoreFocus = false;
			}
		}}
		class="exec-cmdk-dialog exec-cmdk-surface top-[9vh] flex max-h-[78vh] w-full max-w-[calc(100%-2rem)] translate-y-0 flex-col gap-0 overflow-hidden p-0 ring-0 motion-reduce:animate-none motion-reduce:transition-none motion-reduce:duration-0 motion-reduce:data-closed:animate-none motion-reduce:data-open:animate-none sm:max-w-[650px]"
	>
		<Dialog.Title class="sr-only" text="Command palette" />
		<Dialog.Description class="sr-only">
			Search projects, work, team members, run commands, or ask Tess.
		</Dialog.Description>

		<div class="exec-cmdk-tabs flex items-center justify-between gap-2.5">
			<div
				class="exec-cmdk-tab-grp inline-flex items-center gap-1"
				role="tablist"
				aria-label="Palette mode"
			>
				<button
					type="button"
					role="tab"
					aria-selected={modalMode === 'cmd'}
					onclick={() => {
						modalMode = 'cmd';
						void tick().then(() => inputEl?.focus());
					}}
					class="exec-cmdk-tab inline-flex items-center gap-1.5"
				>
					<Search class="size-3.5 shrink-0" aria-hidden="true" />
					<span>Search &amp; Commands</span>
				</button>
				<button
					type="button"
					role="tab"
					aria-selected={modalMode === 'ai'}
					onclick={() => {
						modalMode = 'ai';
						void tick().then(() => inputEl?.focus());
					}}
					class="exec-cmdk-tab inline-flex items-center gap-1.5"
				>
					<Sparkles class="size-3.5 shrink-0" aria-hidden="true" />
					<span>Ask Tess</span>
				</button>
			</div>
			<Dialog.Close class={cn('exec-cmdk-close inline-flex items-center justify-center')}>
				ESC
			</Dialog.Close>
		</div>

		<div class="exec-cmdk-input-row flex items-center gap-3">
			{#if modalMode === 'ai'}
				<Sparkles class="size-5 shrink-0 text-[var(--tone-accent-ink)]" aria-hidden="true" />
			{:else}
				<Search class="size-5 shrink-0 text-[var(--tone-accent-ink)]" aria-hidden="true" />
			{/if}
			<input
				bind:this={inputEl}
				bind:value={query}
				oninput={(event) => {
					resetActive();
					if (
						modalMode === 'cmd' &&
						event.currentTarget.value.trimStart().startsWith('#') &&
						chatConversationsOrgId !== organizationId
					) {
						void loadChatConversations(organizationId);
					}
				}}
				onkeydown={handleInputKeydown}
				type="text"
				role="combobox"
				aria-autocomplete="list"
				aria-expanded={modalMode === 'cmd' && entries.length > 0}
				aria-controls={modalMode === 'cmd' && entries.length > 0 ? 'exec-cmdk-listbox' : undefined}
				aria-activedescendant={modalMode === 'cmd' ? activeOptionId : undefined}
				placeholder={modalMode === 'ai'
					? "Ask Tess — what's blocked, release pace, budget, or idle agents…"
					: 'Type a command, search anything, or ask a question…'}
				class="exec-cmdk-input min-w-0 flex-1 bg-transparent text-base text-[var(--exec-ink)] outline-none"
			/>
			{#if modalMode === 'ai'}
				<button
					type="button"
					disabled={!query.trim()}
					onclick={() => {
						const q = query.trim();
						if (q) handoffToTess(q);
					}}
					class="exec-cmdk-ask-go inline-flex shrink-0 items-center gap-1.5"
				>
					Ask
				</button>
			{/if}
		</div>

		<div class="min-h-0 flex-1 overflow-y-auto p-2.5">
			{#if modalMode === 'ai'}
				<div class="exec-cmdk-ai-pane flex flex-col gap-3 p-1.5">
					<span class="exec-cmdk-grp is-first">Suggested executive questions</span>
					<div class="flex flex-wrap gap-2 px-1">
						{#each buildAskTessEntries() as entry (entry.id)}
							{@const promptText = entry.label.replace(/^Ask:\s*/i, '')}
							<button
								type="button"
								onclick={() => handoffToTess(promptText)}
								class="exec-cmdk-ai-chip inline-flex items-center gap-1.5"
							>
								<Sparkles
									class="size-3.5 shrink-0 text-[var(--tone-accent-ink)]"
									aria-hidden="true"
								/>
								<span>{promptText}</span>
							</button>
						{/each}
					</div>
				</div>
			{:else}
				{@render renderEntries('exec-cmdk-listbox', 'exec-cmdk-opt', false)}
			{/if}
		</div>

		<div class="exec-cmdk-foot" aria-hidden="true">
			{#if modalMode === 'cmd'}
				<span><kbd class={kbdClass}>↑</kbd><kbd class={kbdClass}>↓</kbd> Navigate</span>
			{/if}
			<span><kbd class={kbdClass}>↵</kbd> Run / Ask</span>
			<span><kbd class={kbdClass}>⌘/</kbd> Switch Search &amp; AI</span>
			<span><kbd class={kbdClass}>Esc</kbd> Close</span>
		</div>
	</Dialog.Content>
</Dialog.Root>

<style>
	.exec-topbar {
		height: var(--exec-topbar-height);
		background: var(--exec-topbar-bg);
		border-bottom: var(--exec-topbar-border);
		overflow: visible;
	}
	.exec-home-topbar {
		display: grid;
		grid-template-columns: 1fr minmax(0, 640px) 1fr;
		align-items: center;
		gap: 12px;
	}
	.exec-home-cmdk {
		grid-column: 2;
		justify-self: center;
	}
	.exec-home-utils {
		grid-column: 3;
		justify-self: end;
		display: inline-flex;
		align-items: center;
		gap: 6px;
	}
	@media (max-width: 959px) {
		.exec-home-topbar {
			display: flex;
			justify-content: space-between;
			gap: 10px;
		}
		.exec-home-cmdk {
			flex: 1 1 auto;
		}
	}
	:global(html[data-ui-theme='ocean']) .exec-topbar::before {
		bottom: -14px;
	}
	.exec-cmdk-bar {
		height: 46px;
		padding: 5px 6px;
		border-radius: var(--exec-cmdk-radius);
		background-color: var(--exec-cmdk-bg);
		-webkit-backdrop-filter: var(--exec-cmdk-blur);
		backdrop-filter: var(--exec-cmdk-blur);
		box-shadow: var(--exec-cmdk-shadow);
		transition:
			background-color 0.18s ease,
			box-shadow 0.18s ease,
			transform 0.18s ease;
	}
	.exec-cmdk-input::placeholder,
	.exec-cmdk-home-input::placeholder {
		color: var(--exec-cmdk-trigger-ink);
		opacity: 1;
	}
	.exec-cmdk-bar:hover {
		background-color: var(--exec-cmdk-bg-hover);
		box-shadow: var(--exec-cmdk-shadow-hover);
		transform: var(--exec-cmdk-hover-lift);
	}
	.exec-cmdk-trigger {
		height: 100%;
		gap: 11px;
		padding: 0 10px 0 12px;
		border-radius: var(--exec-cmdk-trigger-radius);
		color: var(--exec-cmdk-trigger-ink);
		font-size: 0.875rem;
		line-height: 1.2;
		font-weight: 450;
		letter-spacing: -0.005em;
		transition:
			color 0.14s ease,
			background-color 0.14s ease;
	}
	.exec-cmdk-home-field {
		cursor: text;
	}
	.exec-cmdk-home-field:focus-within {
		outline: 1px solid var(--exec-border-hi);
		outline-offset: -1px;
	}
	:global(.exec-cmdk-surface) {
		background: var(--exec-cmdk-menu-bg);
		-webkit-backdrop-filter: var(--exec-cmdk-menu-blur);
		backdrop-filter: var(--exec-cmdk-menu-blur);
		box-shadow: var(--exec-cmdk-menu-shadow);
		line-height: 1.714;
		letter-spacing: 0.008em;
		font-optical-sizing: none;
	}
	:global(.exec-cmdk-dialog) {
		border-radius: var(--exec-cmdk-dialog-radius, 18px);
	}
	.exec-cmdk-tabs {
		padding: 12px 16px 10px;
		border-bottom: 1px solid var(--exec-border);
		background: var(--exec-cmdk-tabs-bg, var(--exec-well));
	}
	.exec-cmdk-tab-grp {
		padding: 3px;
		border-radius: var(--exec-cmdk-tab-grp-radius, 10px);
		background: var(--exec-cmdk-tab-grp-bg, color-mix(in srgb, var(--exec-ink) 5%, transparent));
		box-shadow: var(--exec-cmdk-tab-grp-shadow, inset 0 0 0 1px var(--exec-border));
	}
	.exec-cmdk-tab {
		padding: 5px 12px;
		border-radius: var(--exec-cmdk-tab-radius, 8px);
		font-size: 0.78125rem;
		line-height: 1.5;
		font-weight: 600;
		color: var(--exec-muted);
		cursor: pointer;
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.exec-cmdk-tab:hover {
		color: var(--exec-ink);
	}
	.exec-cmdk-tab[aria-selected='true'] {
		background: var(--exec-cmdk-tab-active-bg, var(--exec-accent-soft));
		color: var(--exec-cmdk-tab-active-ink, var(--exec-ink));
		box-shadow: var(
			--exec-cmdk-tab-active-shadow,
			inset 0 0 0 1px var(--exec-cmdk-opt-ring, var(--exec-accent-ring))
		);
	}
	.exec-cmdk-tab[aria-selected='true'] :global(svg) {
		color: var(--exec-cmdk-tab-active-ico, var(--tone-accent-ink));
	}
	:global(.exec-cmdk-close) {
		padding: 3px 8px;
		border-radius: var(--exec-cmdk-esc-radius, 6px);
		background: var(--exec-cmdk-esc-bg, var(--exec-cmdk-tag-bg, var(--exec-well)));
		box-shadow: var(--exec-cmdk-esc-shadow, inset 0 0 0 1px var(--exec-border-hi));
		color: var(--exec-cmdk-esc-ink, var(--exec-muted));
		font-family: inherit;
		font-size: 0.6875rem;
		line-height: 1.5;
		font-weight: 600;
		cursor: pointer;
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	:global(.exec-cmdk-close:hover) {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
		box-shadow: var(--exec-cmdk-esc-shadow, inset 0 0 0 1px var(--exec-border-hi));
	}
	.exec-cmdk-input-row {
		padding: 16px 18px;
		border-bottom: 1px solid var(--exec-border);
	}
	.exec-cmdk-input {
		line-height: 1.45;
		letter-spacing: 0.008em;
	}
	.exec-cmdk-ask-go {
		height: 32px;
		padding: 0 14px;
		border-radius: 9px;
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-cmdk-opt-ring, var(--exec-accent-ring));
		font-size: 0.8125rem;
		font-weight: 600;
		cursor: pointer;
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.exec-cmdk-ask-go:hover:not(:disabled) {
		background: var(--exec-cta-bg);
		color: var(--exec-on-accent);
	}
	.exec-cmdk-ask-go:disabled {
		opacity: 0.42;
		cursor: default;
	}
	.exec-cmdk-ai-chip {
		padding: 7px 13px;
		border-radius: 999px;
		background: var(--exec-cmdk-tag-bg, var(--exec-well));
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		font-size: 0.8125rem;
		font-weight: 500;
		color: var(--exec-body);
		cursor: pointer;
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.exec-cmdk-ai-chip:hover {
		background: var(--exec-accent-soft);
		box-shadow: inset 0 0 0 1px var(--exec-cmdk-opt-ring, var(--exec-accent-ring));
		color: var(--exec-ink);
	}
	.exec-cmdk-home-results {
		position: absolute;
		top: calc(100% + 8px);
		inset-inline: 0;
		z-index: 40;
		max-height: min(27rem, 58dvh);
		border-radius: var(--exec-cmdk-dialog-radius, 14px);
	}
	.exec-cmdk-grp {
		padding: 9px 12px 4px;
		font-size: 0.6875rem;
		line-height: 1.714;
		font-weight: 650;
		letter-spacing: var(--exec-cmdk-grp-tracking, 0.08em);
		text-transform: uppercase;
		color: var(--exec-dim);
	}
	.exec-cmdk-grp.is-first {
		padding-top: 4px;
	}
	.exec-cmdk-opt {
		gap: 11px;
		padding: 10px 12px;
		border-radius: var(--exec-cmdk-opt-radius, 10px);
		color: var(--exec-body);
		font-size: 0.875rem;
		line-height: 1.714;
		letter-spacing: 0.008em;
		cursor: pointer;
		transition:
			background-color 0.1s ease,
			color 0.1s ease;
	}
	.exec-cmdk-opt-lbl {
		font-weight: 500;
		color: inherit;
	}
	.exec-cmdk-opt-sub {
		font-size: 0.75rem;
		line-height: 1.714;
		color: var(--exec-dim);
	}
	.exec-cmdk-opt :global(.exec-cmdk-row-ico) {
		width: 17px;
		height: 17px;
		color: var(--exec-muted);
		transition: color 0.14s ease;
	}
	.exec-cmdk-opt-tag {
		padding: 1px 7px;
		border-radius: var(--exec-cmdk-tag-radius, 5px);
		background: var(--exec-cmdk-tag-bg, var(--exec-well));
		color: var(--exec-muted);
		font-size: 0.6875rem;
		line-height: 1.714;
		font-weight: 500;
	}
	.exec-cmdk-opt:hover {
		background: var(--exec-cmdk-opt-hover-bg, var(--exec-accent-soft));
		color: var(--exec-ink);
		box-shadow: var(
			--exec-cmdk-opt-hover-shadow,
			inset 0 0 0 1px var(--exec-cmdk-opt-ring, var(--exec-accent-ring))
		);
	}
	.exec-cmdk-opt.is-active {
		background: var(--exec-cmdk-opt-active-bg, var(--exec-accent-soft));
		color: var(--exec-ink);
		box-shadow: var(
			--exec-cmdk-opt-active-shadow,
			inset 0 0 0 1px var(--exec-cmdk-opt-ring, var(--exec-accent-ring))
		);
	}
	.exec-cmdk-opt:hover :global(.exec-cmdk-row-ico),
	.exec-cmdk-opt.is-active :global(.exec-cmdk-row-ico) {
		color: var(--exec-cmdk-opt-active-ico, var(--tone-accent-ink));
	}
	.exec-cmdk-foot {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 8px 16px;
		padding: 9px 16px;
		border-top: 1px solid var(--exec-border);
		background: var(--exec-cmdk-foot-bg, var(--exec-well));
		font-size: 0.72rem;
		line-height: 1.714;
		color: var(--exec-dim);
	}
	.exec-cmdk-foot span {
		display: inline-flex;
		align-items: center;
		gap: 5px;
	}
	.exec-cmdk-foot-kbd {
		padding: 1px 5px;
		border-radius: var(--exec-cmdk-kbd-small-radius, 4px);
		background: var(--exec-cmdk-foot-kbd-bg, var(--exec-cmdk-tag-bg, var(--exec-well)));
		box-shadow: inset 0 0 0 1px var(--exec-cmdk-foot-kbd-ring, var(--exec-border-hi));
		font-family: inherit;
		font-size: 0.6875rem;
		line-height: 1.45;
		color: var(--exec-cmdk-foot-kbd-ink, var(--exec-muted));
	}
	.exec-cmdk-trigger:hover {
		color: var(--exec-ink);
		background: var(--exec-cmdk-trigger-hover-bg);
	}
	.exec-cmdk-trigger :global(.exec-cmdk-ico) {
		color: var(--exec-cmdk-icon);
		transition: color 0.14s ease;
	}
	.exec-cmdk-trigger:hover :global(.exec-cmdk-ico) {
		color: var(--exec-cmdk-icon-hover);
	}
	.exec-cmdk-kbd {
		align-items: center;
		justify-content: center;
		height: 24px;
		padding: 0 8px;
		border-radius: var(--exec-cmdk-kbd-radius);
		background: var(--exec-cmdk-kbd-bg);
		box-shadow: var(--exec-cmdk-kbd-shadow);
		color: var(--exec-cmdk-kbd-ink);
		font-family: inherit;
		font-size: 0.72rem;
		font-weight: 600;
		flex: none;
	}
	.exec-cmdk-sep {
		background: var(--exec-cmdk-sep);
	}
	@media (max-width: 720px), (pointer: coarse) {
		.exec-topbar {
			height: auto;
			min-height: max(68px, var(--exec-topbar-height));
			padding-block: 7px;
		}
		.exec-cmdk-bar {
			height: auto;
			min-height: 52px;
			padding: 4px 6px;
		}
		.exec-cmdk-trigger,
		.exec-cmdk-opt,
		:global(.exec-cmdk-close) {
			height: auto;
			min-height: 44px;
		}
		:global(.exec-cmdk-close) {
			min-width: 44px;
		}
	}
	@media (height <= 320px) {
		.exec-topbar {
			height: auto;
			min-height: 44px;
			padding-block: 0;
			border-bottom: 0;
		}
		.exec-cmdk-bar {
			height: auto;
			min-height: 44px;
			padding: 0 4px;
		}
		.exec-cmdk-trigger,
		.exec-ai-btn {
			height: auto;
			min-height: 44px;
		}
	}
</style>
