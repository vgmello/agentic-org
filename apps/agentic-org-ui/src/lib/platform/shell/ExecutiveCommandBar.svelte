<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { afterNavigate, goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import Clock from '@lucide/svelte/icons/clock';
	import Folder from '@lucide/svelte/icons/folder';
	import Hash from '@lucide/svelte/icons/hash';
	import Inbox from '@lucide/svelte/icons/inbox';
	import Search from '@lucide/svelte/icons/search';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Users from '@lucide/svelte/icons/users';
	import * as Dialog from '$lib/components/ui/dialog';
	import { cn } from '$lib/utils';
	import type { Connection, OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import type { InboxItem } from '$lib/modules/inbox/types';
	import type { Conversation } from '$lib/modules/messaging';
	import { answerExecutiveQuestion, type OrgAiAnswer } from '$lib/home/model';
	import { nonOwners } from '$lib/modules/organization/model';
	import NewMenu from './NewMenu.svelte';
	import ScopedSearchInput from '$lib/components/ScopedSearchInput.svelte';
	import { useScopedSearchContext } from './scoped-search.svelte';
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
		registerAsk
	}: {
		connection?: Connection;
		snapshot?: OrgSnapshot;
		items?: WorkItemSummary[];
		inboxItems?: readonly InboxItem[];
		offices?: OfficeInfo[];
		ownerId?: string;
		registerAsk?: (fn: (prompt: string, opts?: ShellAiAskOptions) => void) => void;
	} = $props();

	let newMenu = $state<ReturnType<typeof NewMenu> | undefined>();

	interface CommandEntry {
		id: string;
		group: string;
		label: string;
		sub: string;
		keywords?: string;
		href?: string;
		action?: () => void;
	}

	const kbdClass =
		'inline-flex h-[22px] items-center justify-center rounded-md bg-[var(--exec-well)] px-[7px] text-xs font-semibold text-[var(--exec-muted)] shadow-[inset_0_0_0_1px_var(--exec-border)]';

	const shellData = useShellData();
	const scopedSearch = useScopedSearchContext();
	const activeScopedSearch = $derived(scopedSearch?.active);
	const organizationId = $derived(shellData.organizationId);
	const isHome = $derived(page.url.pathname === '/home');
	const isChat = $derived(page.url.pathname === '/chat' || page.url.pathname.startsWith('/chat/'));

	let chatConversations = $state.raw<Conversation[] | undefined>(undefined);
	let chatConversationsOrgId = $state<string | undefined>(undefined);
	let chatLoadSeq = 0;
	let destroyed = false;

	onDestroy(() => {
		destroyed = true;
	});

	async function loadChatConversations(targetOrgId = organizationId) {
		if (!targetOrgId) return;
		const seq = ++chatLoadSeq;
		try {
			const { chatApi } = await import('$lib/modules/messaging/api');
			const snap = await chatApi.snapshot(targetOrgId);
			if (destroyed || seq !== chatLoadSeq || targetOrgId !== organizationId) return;
			// Lock onto the currently highlighted entry ID before swapping results (for both `#` and
			// unprefixed searches that include channels) so Enter never runs a substituted item.
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
				label: `#${name}`,
				sub: `Team channel · ${t.name}`,
				keywords: `${t.id} ${t.name} ${name}`,
				href: `/chat?c=${encodeURIComponent(`c-team-${t.id}`)}`
			});
		}
		return channels;
	});

	let open = $state(false);
	let inlineOpen = $state(false);
	let mode = $state<'cmd' | 'ai'>('cmd');
	let query = $state('');
	const isChannelQuery = $derived(query.trimStart().startsWith('#'));
	let activeIdx = $state(0);
	let activeEntryId = $state<string | undefined>(undefined);
	let aiHistory = $state<OrgAiAnswer[]>([]);
	let inputEl = $state<HTMLInputElement | null>(null);
	let homeInputEl = $state<HTMLInputElement | null>(null);
	let homeShellEl = $state<HTMLDivElement | null>(null);
	let skipRestoreFocus = false;

	const AI_STARTERS = [
		'What needs me first?',
		'Which projects need attention?',
		'Team & office status',
		'Where is task time going?'
	];

	const entries = $derived.by<CommandEntry[]>(() => {
		const trimmed = query.trim();
		if (trimmed.startsWith('#')) {
			const cq = trimmed.slice(1).trim().toLowerCase();
			if (!cq) return channelEntries;
			return channelEntries.filter(
				(e) =>
					e.label.toLowerCase().includes(cq) ||
					e.sub.toLowerCase().includes(cq) ||
					(e.keywords ? e.keywords.toLowerCase().includes(cq) : false)
			);
		}
		const q = trimmed.toLowerCase();
		const list: CommandEntry[] = [
			{
				id: 'go-home',
				group: 'Navigate',
				label: 'Go to Home',
				sub: 'Executive briefing & Spotlight',
				href: '/home'
			},
			{
				id: 'go-inbox',
				group: 'Navigate',
				label: 'Go to Inbox',
				sub: 'Decisions waiting on you',
				href: '/inbox'
			},
			{
				id: 'go-projects',
				group: 'Navigate',
				label: 'Go to Projects',
				sub: 'Portfolio health & teams',
				href: '/projects'
			},
			{
				id: 'go-work',
				group: 'Navigate',
				label: 'Go to Work',
				sub: 'Tasks, bugs & requests',
				href: '/work'
			},
			{
				id: 'go-teams',
				group: 'Navigate',
				label: 'Go to Teams',
				sub: 'Teams & project ownership',
				href: '/teams'
			},
			{
				id: 'go-members',
				group: 'Navigate',
				label: 'Go to Members',
				sub: 'People, agents & presence',
				href: '/teams/members'
			},
			{
				id: 'go-offices',
				group: 'Navigate',
				label: 'Go to Offices',
				sub: 'Connected machines & status',
				href: '/settings/offices'
			},
			{
				id: 'go-costs',
				group: 'Navigate',
				label: 'Go to Costs',
				sub: 'Monthly spend & agent budgets',
				href: '/settings/costs'
			},
			{
				id: 'go-settings',
				group: 'Navigate',
				label: 'Go to Settings',
				sub: 'Organization & appearance themes',
				href: '/settings'
			},
			...UI_THEMES.map((t) => ({
				id: `theme-${t.id}`,
				group: 'Appearance',
				label: `Theme: ${t.label}`,
				sub: t.description,
				action: () => uiPrefs.setTheme(t.id)
			}))
		];

		if (snapshot) {
			for (const p of (snapshot.projects ?? []).filter((x) => !x.archived)) {
				list.push({
					id: `proj-${p.id}`,
					group: 'Projects',
					label: p.name,
					sub: p.key ? `Project · ${p.key.toUpperCase()}` : 'Project',
					keywords: `${p.id} ${p.key ?? ''}`,
					href: `/projects/${p.id}/work`
				});
			}
			for (const m of nonOwners(snapshot)) {
				list.push({
					id: `mem-${m.id}`,
					group: 'Team',
					label: m.name,
					sub: m.agent?.title ?? m.agent?.role ?? m.kind,
					keywords: `${m.id} ${m.agent?.role ?? ''} ${m.agent?.messaging_name ?? ''}`,
					href: `/members/${m.id}`
				});
			}
		}

		for (const inboxItem of inboxItems) {
			list.push({
				id: `inbox-${inboxItem.id}`,
				group: 'Inbox',
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
				item.kind === 'request'
					? `/work/${encodeURIComponent(item.short_id)}`
					: item.project_id
						? `/projects/${encodeURIComponent(item.project_id)}/work`
						: `/work/${encodeURIComponent(item.short_id)}`;
			list.push({
				id: `work-${item.id}`,
				group: 'Work',
				label: item.title,
				sub: `${item.short_id} · ${item.status.replace('_', ' ')}`,
				keywords: `${item.short_id} ${item.kind} ${item.id}`,
				href: dest
			});
		}

		if (!q) return list.slice(0, 15);
		const searchable = isChat ? [...channelEntries, ...list] : [...list, ...channelEntries];
		const allMatched = searchable.filter(
			(e) =>
				e.label.toLowerCase().includes(q) ||
				e.sub.toLowerCase().includes(q) ||
				e.group.toLowerCase().includes(q) ||
				(e.keywords ? e.keywords.toLowerCase().includes(q) : false)
		);
		const shown = !open && allMatched.length > 11 ? allMatched.slice(0, 11) : [...allMatched];

		if (!open && allMatched.length > 11) {
			shown.push({
				id: 'show-all-matches',
				group: 'Search',
				label: `Show all ${allMatched.length} matches`,
				sub: 'Open full searchable results list',
				action: () => openModalWithQuery('cmd', trimmed)
			});
		}

		shown.push({
			id: 'ask-org-ai-fallback',
			group: 'Org AI',
			label: `Ask Org AI about '${trimmed}'`,
			sub: 'Get an executive summary for this query',
			action: () => askQuestion(trimmed)
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
		mode === 'cmd' && safeActiveIdx >= 0 ? `exec-cmdk-opt-${safeActiveIdx}` : undefined
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
	const currentAi = $derived(aiHistory[0]);
	const earlierAi = $derived(aiHistory.slice(1, 5));
	const askedSet = $derived(new Set(aiHistory.map((h) => h.query.toLowerCase())));
	const followUps = $derived(AI_STARTERS.filter((s) => !askedSet.has(s.toLowerCase())));

	afterNavigate(() => {
		if (inlineOpen) {
			closeInline(true);
		}
	});

	function resetModalState() {
		query = '';
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
		mode = 'cmd';
		inlineOpen = true;
		resetActive();
		void loadChatConversations(organizationId);
		void tick().then(() => homeInputEl?.focus());
	}

	function openModal(nextMode: 'cmd' | 'ai') {
		closeInline(false);
		skipRestoreFocus = false;
		mode = nextMode;
		open = true;
		resetActive();
		if (nextMode === 'cmd') void loadChatConversations(organizationId);
	}

	function openModalWithQuery(nextMode: 'cmd' | 'ai', nextQuery: string) {
		closeInline(false);
		skipRestoreFocus = false;
		query = nextQuery;
		mode = nextMode;
		open = true;
		resetActive();
		if (nextMode === 'cmd') void loadChatConversations(organizationId);
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

	function submitAi(raw: string, opts?: ShellAiAskOptions) {
		const trimmed = raw.trim();
		if (!trimmed || (!snapshot && !opts?.decision)) return;
		const ans = answerExecutiveQuestion(trimmed, {
			snapshot,
			items,
			offices,
			ownerId,
			domain: { inboxItems },
			decisionId: opts?.decisionId,
			decision: opts?.decision
		});
		aiHistory = [
			ans,
			...aiHistory.filter((x) => x.query.toLowerCase() !== ans.query.toLowerCase())
		];
		query = '';
		resetActive();
	}

	function askQuestion(prompt: string, opts?: ShellAiAskOptions) {
		closeInline(true);
		skipRestoreFocus = false;
		mode = 'ai';
		open = true;
		resetActive();
		submitAi(prompt, opts);
		void tick().then(() => inputEl?.focus());
	}

	$effect(() => {
		registerAsk?.(askQuestion);
	});

	function handleWindowPointerDown(event: PointerEvent) {
		if (!inlineOpen) return;
		const target = event.target;
		if (target instanceof Node && homeShellEl?.contains(target)) return;
		closeInline(false);
	}

	function handleWindowKeydown(event: KeyboardEvent) {
		if (event.isComposing || event.keyCode === 229) return;
		if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
			event.preventDefault();
			if (open) {
				closeModal();
			} else if (isHome) {
				if (inlineOpen && document.activeElement === homeInputEl) {
					closeInline(false);
					homeInputEl?.blur();
				} else {
					openInline();
				}
			} else {
				openModal('cmd');
			}
			return;
		}
		if ((event.metaKey || event.ctrlKey) && event.key === '/') {
			event.preventDefault();
			if (open) {
				mode = mode === 'cmd' ? 'ai' : 'cmd';
				resetActive();
				if (mode === 'cmd') void loadChatConversations(organizationId);
				void tick().then(() => inputEl?.focus());
			} else {
				openModal('ai');
			}
			return;
		}
		const overlaySelector =
			'[role="menu"], [role="dialog"]:not(.work-reading), [role="alertdialog"], [role="listbox"], aside[aria-label="Thread"], aside[aria-label="Members"], [data-chat-panel], [data-slot="sheet-content"]';
		const overlayOpen =
			Boolean(document.querySelector(overlaySelector)) ||
			(event.target instanceof HTMLElement &&
				Boolean(
					event.target.closest(
						'[role="menu"], [role="dialog"], [role="alertdialog"], [role="listbox"], aside[aria-label], [data-chat-panel], [data-slot="sheet-content"]'
					)
				));
		const altGr = Boolean(event.getModifierState?.('AltGraph'));
		if (
			!open &&
			isChat &&
			singleKeyShortcutsOn() &&
			event.key === '#' &&
			!event.repeat &&
			!event.metaKey &&
			(altGr || (!event.ctrlKey && !event.altKey)) &&
			!isEditableTarget(event.target) &&
			!overlayOpen
		) {
			event.preventDefault();
			openModalWithQuery('cmd', '#');
			return;
		}
		if (
			!open &&
			!inlineOpen &&
			connection &&
			singleKeyShortcutsOn() &&
			(event.key === 'n' || event.key === 'N') &&
			!event.repeat &&
			!event.metaKey &&
			!event.ctrlKey &&
			!event.altKey &&
			!isEditableTarget(event.target) &&
			!overlayOpen
		) {
			event.preventDefault();
			newMenu?.openMenu();
			return;
		}
		if (
			!open &&
			singleKeyShortcutsOn() &&
			event.key === '/' &&
			!event.repeat &&
			!event.metaKey &&
			!event.ctrlKey &&
			!event.altKey &&
			!event.shiftKey &&
			!isEditableTarget(event.target) &&
			!overlayOpen
		) {
			event.preventDefault();
			if (isHome) openInline();
			else openModal('cmd');
		}
	}

	$effect(() => {
		if ((!open || mode !== 'cmd') && !inlineListboxVisible) return;
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

	function handleInputKeydown(event: KeyboardEvent) {
		if (event.isComposing || event.keyCode === 229) return;
		if (mode === 'ai' && open) {
			if (event.key === 'Enter' && query.trim()) {
				event.preventDefault();
				submitAi(query);
			}
			return;
		}
		if (event.key === 'Tab' && isHome && !open && inlineOpen) {
			inlineOpen = false;
			return;
		}
		if (event.key === 'ArrowDown') {
			event.preventDefault();
			if (isHome && !open && !inlineOpen) {
				inlineOpen = true;
				if (entries.length > 0) setActiveIndex(0);
				else resetActive();
				return;
			}
			if (entries.length > 0) {
				setActiveIndex(safeActiveIdx < 0 ? 0 : (safeActiveIdx + 1) % entries.length);
			}
		} else if (event.key === 'ArrowUp') {
			event.preventDefault();
			if (isHome && !open && !inlineOpen) {
				inlineOpen = true;
				if (entries.length > 0) setActiveIndex(entries.length - 1);
				else resetActive();
				return;
			}
			if (entries.length > 0) {
				setActiveIndex(
					safeActiveIdx < 0
						? entries.length - 1
						: (safeActiveIdx - 1 + entries.length) % entries.length
				);
			}
		} else if (event.key === 'Enter') {
			const canRun = (open && mode === 'cmd') || (isHome && !open && inlineOpen);
			if (canRun && safeActiveIdx >= 0 && entries[safeActiveIdx]) {
				event.preventDefault();
				runEntry(entries[safeActiveIdx]);
			}
		} else if (event.key === 'Escape' && isHome && !open) {
			if (inlineOpen) {
				event.preventDefault();
				closeInline(false);
			} else if (query.length > 0) {
				event.preventDefault();
				query = '';
				resetActive();
			}
		}
	}
</script>

<svelte:window onkeydown={handleWindowKeydown} onpointerdown={handleWindowPointerDown} />

<header
	class="exec-topbar relative z-30 flex shrink-0 flex-wrap items-center justify-center gap-2.5 overflow-visible px-4 sm:px-6"
>
	{#if activeScopedSearch}
		<div class="exec-scoped-search-host min-w-0 shrink" data-testid="header-scoped-search">
			<ScopedSearchInput
				value={activeScopedSearch.getValue()}
				onvaluechange={(next) => activeScopedSearch.setValue(next)}
				label={activeScopedSearch.label}
				placeholder={activeScopedSearch.placeholder}
				headingSelector={activeScopedSearch.headingSelector}
				class="w-full max-w-[18rem]"
			/>
		</div>
	{/if}
	<div
		bind:this={homeShellEl}
		class="exec-cmdk-bar relative flex min-w-0 flex-1 max-w-[640px] items-center"
	>
		{#if isHome}
			<label
				class="exec-cmdk-trigger exec-cmdk-home-field flex min-w-0 flex-1 items-center text-left"
			>
				<Search class="exec-cmdk-ico size-[18px] shrink-0" aria-hidden="true" />
				<input
					bind:this={homeInputEl}
					bind:value={query}
					type="search"
					autocomplete="off"
					placeholder="Search, run a command, or ask Org AI…"
					aria-label="Search, run a command, or ask Org AI"
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
		{:else}
			<button
				type="button"
				onclick={() => openModal('cmd')}
				aria-haspopup="dialog"
				aria-expanded={open}
				class="exec-cmdk-trigger flex min-w-0 flex-1 items-center text-left"
			>
				<Search class="exec-cmdk-ico size-[18px] shrink-0" aria-hidden="true" />
				<span class="min-w-0 flex-1 truncate">Search, run a command, or ask Org AI…</span>
				<kbd class="exec-cmdk-kbd hidden sm:inline-flex">⌘K</kbd>
			</button>
		{/if}
		<span class="exec-cmdk-sep mx-1.5 h-[22px] w-px shrink-0" aria-hidden="true"></span>
		<button
			type="button"
			onclick={() => {
				const pending = query.trim();
				if (isHome && pending) {
					askQuestion(pending);
				} else {
					openModal('ai');
				}
			}}
			class="exec-ai-btn inline-flex shrink-0 items-center px-3.5"
		>
			<Sparkles class="exec-ai-ico size-[15px] shrink-0" aria-hidden="true" />
			<span>Ask</span>
		</button>

		{#if isHome && inlineOpen}
			<div
				class="exec-cmdk-home-results flex flex-col overflow-hidden rounded-[14px] bg-[var(--popover)] shadow-[inset_0_0_0_1px_var(--exec-border-hi),0_18px_42px_color-mix(in_srgb,var(--shade)_36%,transparent)]"
			>
				<div class="min-h-0 flex-1 overflow-y-auto p-2">
					{#if entries.length === 0}
						<div class="flex flex-col items-center gap-2 py-6 text-center">
							<p class="text-sm text-[var(--exec-muted)]">
								{isChannelQuery
									? `No channels matching “${query.trim()}”.`
									: `No matches for “${query}”.`}
							</p>
							{#if query.trim() && !isChannelQuery}
								<button
									type="button"
									onmousedown={(e) => e.preventDefault()}
									onclick={() => askQuestion(query)}
									class="inline-flex min-h-9 items-center gap-1.5 rounded-lg bg-[var(--exec-accent-soft)] px-3 text-xs font-semibold text-[var(--exec-accent-ink)] shadow-[inset_0_0_0_1px_var(--exec-accent-ring)]"
								>
									<Sparkles class="size-3.5" aria-hidden="true" />
									<span>Ask Org AI “{query.trim()}”</span>
								</button>
							{/if}
						</div>
					{:else}
						<ul
							id="exec-cmdk-home-listbox"
							class="flex flex-col gap-0.5"
							role="listbox"
							aria-label="Search results"
						>
							{#each entries as entry, idx (entry.id)}
								<li>
									<button
										type="button"
										id="exec-cmdk-home-opt-{idx}"
										role="option"
										tabindex={-1}
										aria-selected={idx === safeActiveIdx}
										onmousedown={(e) => e.preventDefault()}
										onmouseenter={() => setActiveIndex(idx)}
										onclick={() => runEntry(entry)}
										class={[
											'exec-cmdk-home-opt flex min-h-9 w-full items-center gap-3 rounded-[10px] px-3 py-2 text-left text-sm transition-colors',
											idx === safeActiveIdx && 'is-active'
										]}
									>
										{#if entry.group === 'Channels'}
											<Hash class="size-4 shrink-0" aria-hidden="true" />
										{:else if entry.group === 'Projects'}
											<Folder class="size-4 shrink-0" aria-hidden="true" />
										{:else if entry.group === 'Team'}
											<Users class="size-4 shrink-0" aria-hidden="true" />
										{:else if entry.group === 'Work' || entry.group === 'Inbox'}
											<Inbox class="size-4 shrink-0" aria-hidden="true" />
										{:else}
											<Search class="size-4 shrink-0" aria-hidden="true" />
										{/if}
										<span class="min-w-0 flex-1 truncate font-medium">{entry.label}</span>
										<span class="hidden truncate text-xs text-[var(--exec-dim)] sm:inline">
											{entry.sub}
										</span>
										<span
											class="shrink-0 rounded-[5px] bg-[var(--exec-well)] px-[7px] py-[1px] text-xs font-medium text-[var(--exec-muted)]"
										>
											{entry.group}
										</span>
									</button>
								</li>
							{/each}
						</ul>
					{/if}
				</div>
				<div
					class="flex flex-wrap items-center justify-between gap-3 border-t border-[var(--exec-border)] bg-[var(--exec-well)] px-3.5 py-2 text-xs text-[var(--exec-dim)]"
					aria-hidden="true"
				>
					<div class="flex items-center gap-3">
						<span><kbd class={[kbdClass, 'mr-1']}>↑↓</kbd> Move</span>
						<span><kbd class={[kbdClass, 'mr-1']}>↵</kbd> Open</span>
						<span><kbd class={[kbdClass, 'mr-1']}>Esc</kbd> Close</span>
					</div>
					<span><kbd class={[kbdClass, 'mr-1']}>⌘/</kbd> Ask Org AI</span>
				</div>
			</div>
		{/if}
	</div>
	{#if connection}
		<div class="shrink-0">
			<NewMenu bind:this={newMenu} {connection} shortcutHint={singleKeyShortcutsOn()} />
		</div>
	{/if}
</header>

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
		class="exec-cmdk-dialog top-[9vh] flex max-h-[78vh] w-full max-w-[calc(100%-2rem)] translate-y-0 flex-col gap-0 overflow-hidden rounded-[18px] bg-[var(--popover)] p-0 shadow-[inset_0_0_0_1px_var(--exec-border-hi),0_24px_64px_color-mix(in_srgb,var(--shade)_38%,transparent)] ring-0 motion-reduce:animate-none motion-reduce:transition-none motion-reduce:duration-0 motion-reduce:data-closed:animate-none motion-reduce:data-open:animate-none sm:max-w-[650px]"
	>
		<Dialog.Title class="sr-only" text="Command palette and Org AI" />
		<Dialog.Description class="sr-only">
			Search projects, work, team members, run commands, or ask Org AI.
		</Dialog.Description>

		<div
			class="exec-cmdk-tabs flex items-center justify-between gap-3 border-b border-[var(--exec-border)] bg-[var(--exec-well)] px-4 py-3 text-[var(--exec-dim)]"
		>
			<div
				class="exec-cmdk-tab-grp inline-flex items-center gap-1 rounded-[10px] bg-[var(--exec-surface)] p-0.5 shadow-[inset_0_0_0_1px_var(--exec-border)]"
				role="tablist"
			>
				<button
					type="button"
					role="tab"
					aria-selected={mode === 'cmd'}
					onclick={() => {
						mode = 'cmd';
						resetActive();
						void loadChatConversations(organizationId);
					}}
					class="exec-cmdk-tab inline-flex min-h-9 items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-semibold transition-colors"
				>
					<Search class="size-3.5" aria-hidden="true" />
					<span>Search &amp; Commands</span>
				</button>
				<button
					type="button"
					role="tab"
					aria-selected={mode === 'ai'}
					onclick={() => (mode = 'ai')}
					class="exec-cmdk-tab inline-flex min-h-9 items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-semibold transition-colors"
				>
					<Sparkles class="size-3.5" aria-hidden="true" />
					<span>Ask Org AI</span>
				</button>
			</div>
			<Dialog.Close
				class={cn(
					kbdClass,
					'exec-cmdk-close inline-flex min-h-9 min-w-9 items-center justify-center px-2.5 text-xs'
				)}
			>
				ESC
			</Dialog.Close>
		</div>

		<div
			class="exec-cmdk-input-row flex items-center gap-3 border-b border-[var(--exec-border)] px-4 py-3.5"
		>
			{#if mode === 'ai'}
				<Sparkles class="size-5 shrink-0 text-[var(--exec-accent-ink)]" aria-hidden="true" />
			{:else}
				<Search class="size-5 shrink-0 text-[var(--exec-accent-ink)]" aria-hidden="true" />
			{/if}
			<input
				bind:this={inputEl}
				bind:value={query}
				oninput={(event) => {
					resetActive();
					if (
						event.currentTarget.value.trimStart().startsWith('#') &&
						chatConversationsOrgId !== organizationId
					) {
						void loadChatConversations(organizationId);
					}
				}}
				onkeydown={handleInputKeydown}
				type={mode === 'cmd' ? 'text' : 'search'}
				role={mode === 'cmd' ? 'combobox' : undefined}
				aria-autocomplete={mode === 'cmd' ? 'list' : undefined}
				aria-expanded={mode === 'cmd' ? entries.length > 0 : undefined}
				aria-controls={mode === 'cmd' && entries.length > 0 ? 'exec-cmdk-listbox' : undefined}
				aria-activedescendant={activeOptionId}
				placeholder={mode === 'ai'
					? 'Ask Org AI — what needs you, project risks, or office status…'
					: 'Search projects, work, team, or jump anywhere…'}
				class="exec-cmdk-input min-w-0 flex-1 bg-transparent text-[15px] text-[var(--exec-ink)] outline-none"
			/>
			{#if mode === 'ai'}
				<button
					type="button"
					disabled={!query.trim()}
					onclick={() => submitAi(query)}
					class="min-h-9 rounded-lg bg-[var(--exec-accent)] px-3.5 py-1.5 text-xs font-semibold text-[var(--exec-on-accent)] disabled:cursor-not-allowed disabled:opacity-45"
				>
					Ask
				</button>
			{/if}
		</div>

		<div class="min-h-0 flex-1 overflow-y-auto p-2.5">
			{#if mode === 'cmd'}
				{#if entries.length === 0}
					<p class="py-8 text-center text-sm text-[var(--exec-muted)]">
						{isChannelQuery
							? `No channels matching “${query.trim()}”.`
							: `No matches for “${query}”. Switch to Ask Org AI for a live summary.`}
					</p>
				{:else}
					<ul
						id="exec-cmdk-listbox"
						class="flex flex-col gap-0.5"
						role="listbox"
						aria-label="Search results"
					>
						{#each entries as entry, idx (entry.id)}
							<li>
								<button
									type="button"
									id="exec-cmdk-opt-{idx}"
									role="option"
									tabindex={-1}
									aria-selected={idx === safeActiveIdx}
									onmouseenter={() => setActiveIndex(idx)}
									onclick={() => runEntry(entry)}
									class={[
										'exec-cmdk-opt flex min-h-9 w-full items-center gap-3 rounded-[10px] px-3 py-2.5 text-left text-sm transition-colors',
										idx === safeActiveIdx && 'is-active'
									]}
								>
									{#if entry.group === 'Channels'}
										<Hash class="size-4 shrink-0" aria-hidden="true" />
									{:else if entry.group === 'Projects'}
										<Folder class="size-4 shrink-0" aria-hidden="true" />
									{:else if entry.group === 'Team'}
										<Users class="size-4 shrink-0" aria-hidden="true" />
									{:else if entry.group === 'Work' || entry.group === 'Inbox'}
										<Inbox class="size-4 shrink-0" aria-hidden="true" />
									{:else}
										<Search class="size-4 shrink-0" aria-hidden="true" />
									{/if}
									<span class="min-w-0 flex-1 truncate font-medium">{entry.label}</span>
									<span class="hidden truncate text-xs text-[var(--exec-dim)] sm:inline"
										>{entry.sub}</span
									>
									<span
										class="shrink-0 rounded-[5px] bg-[var(--exec-well)] px-[7px] py-[1px] text-xs font-medium text-[var(--exec-muted)]"
									>
										{entry.group}
									</span>
								</button>
							</li>
						{/each}
					</ul>
				{/if}
			{:else}
				<div class="flex flex-col gap-3 p-1">
					{#if currentAi}
						<div
							class="relative overflow-hidden rounded-xl bg-[var(--exec-accent-soft)] p-4 pl-5 shadow-[inset_0_0_0_1px_var(--exec-accent-ring)] before:absolute before:inset-y-0 before:left-0 before:w-[3px] before:bg-[var(--exec-accent)] before:content-['']"
						>
							<div
								class="mb-2 flex items-center justify-between gap-2 text-xs font-semibold text-[var(--exec-accent-ink)]"
							>
								<span class="inline-flex items-center gap-1.5">
									<Sparkles class="size-3.5" aria-hidden="true" />
									<span>Org AI · “{currentAi.query}”</span>
								</span>
								<button
									type="button"
									onclick={() => (aiHistory = [])}
									class="rounded px-1.5 py-0.5 text-xs text-[var(--exec-muted)] hover:text-[var(--exec-ink)]"
								>
									Clear
								</button>
							</div>
							<p class="text-sm font-semibold text-[var(--exec-ink)]">
								{currentAi.headline}
							</p>
							<ul class="mt-2 flex flex-col gap-1.5 text-sm text-[var(--exec-body)]">
								{#each currentAi.bullets as bullet (bullet.text)}
									<li class="flex flex-wrap items-baseline gap-x-2">
										<span>• {bullet.text}</span>
										{#if bullet.href && bullet.linkLabel}
											<a
												href={resolve(bullet.href as '/')}
												onclick={() => closeModal(false)}
												class="text-xs font-semibold text-[var(--exec-accent-ink)] underline underline-offset-2"
											>
												{bullet.linkLabel}
											</a>
										{/if}
									</li>
								{/each}
							</ul>
						</div>
					{/if}

					{#if earlierAi.length > 0}
						<div class="flex flex-wrap items-center gap-1.5">
							<span class="text-xs font-semibold tracking-wider text-[var(--exec-dim)] uppercase">
								Earlier
							</span>
							{#each earlierAi as turn (turn.query)}
								<button
									type="button"
									onclick={() => submitAi(turn.query)}
									class="inline-flex min-h-9 items-center gap-1.5 rounded-full bg-[var(--exec-well)] px-2.5 py-1 text-xs font-medium text-[var(--exec-body)] shadow-[inset_0_0_0_1px_var(--exec-border)] transition-colors hover:bg-[var(--exec-accent-soft)] hover:text-[var(--exec-ink)] hover:shadow-[inset_0_0_0_1.5px_var(--tone-accent-ink)]"
								>
									<Clock class="size-3" aria-hidden="true" />
									<span>{turn.query}</span>
								</button>
							{/each}
						</div>
					{/if}

					{#if followUps.length > 0}
						<div>
							<div
								class="mb-2 text-xs font-semibold tracking-wider text-[var(--exec-dim)] uppercase"
							>
								{aiHistory.length > 0 ? 'Ask a follow-up' : 'Suggested executive questions'}
							</div>
							<div class="flex flex-wrap gap-2">
								{#each followUps as starter (starter)}
									<button
										type="button"
										onclick={() => submitAi(starter)}
										class="inline-flex min-h-9 items-center gap-1.5 rounded-full bg-[var(--exec-well)] px-3 py-1.5 text-xs font-medium text-[var(--exec-body)] shadow-[inset_0_0_0_1px_var(--exec-border)] transition-colors hover:bg-[var(--exec-accent-soft)] hover:text-[var(--exec-ink)] hover:shadow-[inset_0_0_0_1.5px_var(--tone-accent-ink)]"
									>
										<Sparkles class="size-3.5 text-[var(--exec-accent-ink)]" aria-hidden="true" />
										<span>{starter}</span>
									</button>
								{/each}
							</div>
						</div>
					{/if}
				</div>
			{/if}
		</div>

		<div
			class="flex flex-wrap items-center gap-4 border-t border-[var(--exec-border)] bg-[var(--exec-well)] px-4 py-2 text-xs text-[var(--exec-dim)]"
			aria-hidden="true"
		>
			<span><kbd class={[kbdClass, 'mr-1']}>↵</kbd> Run / Ask</span>
			<span><kbd class={[kbdClass, 'mr-1']}>⌘/</kbd> Switch Search &amp; AI</span>
			<span><kbd class={[kbdClass, 'mr-1']}>Esc</kbd> Close</span>
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
	/* Visible >= 3.0:1 focus indicator across all 6 themes (var(--focus): 5.24:1 - 12.14:1) */
	.exec-cmdk-home-field:focus-within,
	.exec-cmdk-input-row:focus-within {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	.exec-cmdk-home-results {
		position: absolute;
		top: calc(100% + 6px);
		inset-inline: 0;
		z-index: 40;
		max-height: min(26rem, 55dvh);
		-webkit-backdrop-filter: none;
		backdrop-filter: none;
	}
	.exec-cmdk-home-opt,
	.exec-cmdk-opt {
		color: var(--exec-body);
	}
	:global(.exec-cmdk-tab) {
		color: var(--exec-muted);
	}
	:global(.exec-cmdk-tab:hover) {
		color: var(--exec-ink);
	}
	:global(.exec-cmdk-tab[aria-selected='true']) {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1.5px var(--tone-accent-ink);
	}
	.exec-cmdk-home-opt:hover,
	.exec-cmdk-opt:hover,
	.exec-cmdk-home-opt.is-active,
	.exec-cmdk-opt.is-active {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1.5px var(--tone-accent-ink);
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
	.exec-ai-btn {
		height: 36px;
		min-height: 36px;
		gap: 7px;
		border-radius: var(--exec-ai-radius);
		background: var(--exec-ai-bg);
		box-shadow: var(--exec-ai-shadow);
		color: var(--exec-ai-ink);
		font-size: 0.8125rem;
		line-height: 1.2;
		font-weight: 600;
		letter-spacing: -0.005em;
		transition:
			background 0.15s ease,
			box-shadow 0.15s ease,
			color 0.15s ease;
	}
	.exec-ai-btn :global(.exec-ai-ico) {
		color: var(--exec-ai-ico);
		transition: color 0.15s ease;
	}
	.exec-ai-btn:hover,
	.exec-ai-btn:focus-visible {
		background: var(--exec-ai-bg-hover);
		box-shadow: var(--exec-ai-shadow-hover);
		color: var(--exec-ai-ink-hover);
	}
	.exec-ai-btn:hover :global(.exec-ai-ico),
	.exec-ai-btn:focus-visible :global(.exec-ai-ico) {
		color: var(--exec-ai-ico-hover);
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
		.exec-ai-btn,
		.exec-cmdk-home-opt,
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
