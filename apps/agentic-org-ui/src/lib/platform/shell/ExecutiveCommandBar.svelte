<script lang="ts">
	import { tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import Clock from '@lucide/svelte/icons/clock';
	import Folder from '@lucide/svelte/icons/folder';
	import Inbox from '@lucide/svelte/icons/inbox';
	import Search from '@lucide/svelte/icons/search';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Users from '@lucide/svelte/icons/users';
	import * as Dialog from '$lib/components/ui/dialog';
	import { cn } from '$lib/utils';
	import type { OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import type { InboxItem } from '$lib/modules/inbox/types';
	import { answerExecutiveQuestion, type OrgAiAnswer } from '$lib/home/model';
	import { nonOwners } from '$lib/org/model';
	import { isEditableTarget, singleKeyShortcutsOn } from './shortcuts';
	import { UI_THEMES, uiPrefs } from './ui-preferences.svelte';

	let {
		snapshot,
		items = [],
		inboxItems = [],
		offices = [],
		ownerId = 'owner'
	}: {
		snapshot?: OrgSnapshot;
		items?: WorkItemSummary[];
		inboxItems?: readonly InboxItem[];
		offices?: OfficeInfo[];
		ownerId?: string;
	} = $props();

	interface CommandEntry {
		id: string;
		group: string;
		label: string;
		sub: string;
		href?: string;
		action?: () => void;
	}

	const kbdClass =
		'inline-flex h-[22px] items-center justify-center rounded-md bg-[var(--exec-well)] px-[7px] text-[0.7rem] font-semibold text-[var(--exec-muted)] shadow-[inset_0_0_0_1px_var(--exec-border)]';

	let open = $state(false);
	let mode = $state<'cmd' | 'ai'>('cmd');
	let query = $state('');
	let activeIdx = $state(0);
	let aiHistory = $state<OrgAiAnswer[]>([]);
	let inputEl = $state<HTMLInputElement | null>(null);
	let skipRestoreFocus = false;

	const AI_STARTERS = [
		'What needs me first?',
		'Which projects need attention?',
		'Team & office status',
		'Where is task time going?'
	];

	const entries = $derived.by<CommandEntry[]>(() => {
		const q = query.trim().toLowerCase();
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
					href: `/projects/${p.id}/work`
				});
			}
			for (const m of nonOwners(snapshot).slice(0, 12)) {
				list.push({
					id: `mem-${m.id}`,
					group: 'Team',
					label: m.name,
					sub: m.agent?.role ?? m.kind,
					href: `/members/${m.id}`
				});
			}
		}

		for (const item of items.filter((i) => !i.deleted).slice(0, 12)) {
			list.push({
				id: `work-${item.id}`,
				group: 'Work',
				label: item.title,
				sub: `${item.short_id} · ${item.status.replace('_', ' ')}`,
				href: item.kind === 'request' ? `/work?request=${encodeURIComponent(item.id)}` : '/work'
			});
		}

		if (!q) return list.slice(0, 15);
		return list
			.filter((e) => e.label.toLowerCase().includes(q) || e.sub.toLowerCase().includes(q))
			.slice(0, 12);
	});

	const safeActiveIdx = $derived(entries.length > 0 ? Math.min(activeIdx, entries.length - 1) : 0);
	const activeOptionId = $derived(
		mode === 'cmd' && entries.length > 0 ? `exec-cmdk-opt-${safeActiveIdx}` : undefined
	);
	const currentAi = $derived(aiHistory[0]);
	const earlierAi = $derived(aiHistory.slice(1, 5));
	const askedSet = $derived(new Set(aiHistory.map((h) => h.query.toLowerCase())));
	const followUps = $derived(AI_STARTERS.filter((s) => !askedSet.has(s.toLowerCase())));

	function resetModalState() {
		query = '';
		activeIdx = 0;
	}

	function openModal(nextMode: 'cmd' | 'ai') {
		skipRestoreFocus = false;
		mode = nextMode;
		open = true;
		activeIdx = 0;
	}

	function closeModal(restoreFocus = true) {
		skipRestoreFocus = !restoreFocus;
		open = false;
		resetModalState();
	}

	function runEntry(entry: CommandEntry) {
		closeModal(!entry.href);
		if (entry.action) entry.action();
		else if (entry.href) void goto(resolve(entry.href as '/'));
	}

	function submitAi(raw: string) {
		const trimmed = raw.trim();
		if (!trimmed || !snapshot) return;
		const ans = answerExecutiveQuestion(trimmed, {
			snapshot,
			items,
			offices,
			ownerId,
			domain: { inboxItems }
		});
		aiHistory = [
			ans,
			...aiHistory.filter((x) => x.query.toLowerCase() !== ans.query.toLowerCase())
		];
		query = '';
	}

	function handleWindowKeydown(event: KeyboardEvent) {
		if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
			event.preventDefault();
			if (open) closeModal();
			else openModal('cmd');
			return;
		}
		if (open && (event.metaKey || event.ctrlKey) && event.key === '/') {
			event.preventDefault();
			mode = mode === 'cmd' ? 'ai' : 'cmd';
			void tick().then(() => inputEl?.focus());
			return;
		}
		if (
			!open &&
			singleKeyShortcutsOn() &&
			event.key === '/' &&
			!event.repeat &&
			!event.isComposing &&
			!event.metaKey &&
			!event.ctrlKey &&
			!event.altKey &&
			!event.shiftKey &&
			!isEditableTarget(event.target) &&
			!(
				event.target instanceof HTMLElement &&
				Boolean(
					event.target.closest(
						'[role="menu"], [role="dialog"], [role="alertdialog"], [role="listbox"]'
					)
				)
			)
		) {
			event.preventDefault();
			openModal('cmd');
		}
	}

	$effect(() => {
		if (!open || mode !== 'cmd' || entries.length === 0) return;
		void query;
		const targetId = `exec-cmdk-opt-${safeActiveIdx}`;
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
		if (mode === 'ai') {
			if (event.key === 'Enter' && query.trim()) {
				event.preventDefault();
				submitAi(query);
			}
			return;
		}
		if (event.key === 'ArrowDown') {
			event.preventDefault();
			activeIdx = entries.length > 0 ? (safeActiveIdx + 1) % entries.length : 0;
		} else if (event.key === 'ArrowUp') {
			event.preventDefault();
			activeIdx = entries.length > 0 ? (safeActiveIdx - 1 + entries.length) % entries.length : 0;
		} else if (event.key === 'Enter' && entries[safeActiveIdx]) {
			event.preventDefault();
			runEntry(entries[safeActiveIdx]);
		}
	}
</script>

<svelte:window onkeydown={handleWindowKeydown} />

<header
	class="exec-topbar relative z-30 flex shrink-0 items-center justify-center overflow-visible px-6"
>
	<div class="exec-cmdk-bar flex w-full max-w-[640px] items-center">
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
		<span class="exec-cmdk-sep mx-1.5 h-[22px] w-px shrink-0" aria-hidden="true"></span>
		<button
			type="button"
			onclick={() => openModal('ai')}
			class="exec-ai-btn inline-flex shrink-0 items-center px-3.5"
		>
			<Sparkles class="exec-ai-ico size-[15px] shrink-0" aria-hidden="true" />
			<span>Ask</span>
		</button>
	</div>
</header>

<Dialog.Root
	bind:open
	onOpenChange={(next) => {
		if (!next) resetModalState();
	}}
>
	<Dialog.Content
		showCloseButton={false}
		onOpenAutoFocus={(event) => {
			event.preventDefault();
			inputEl?.focus();
		}}
		onCloseAutoFocus={(event) => {
			if (skipRestoreFocus) {
				event.preventDefault();
				skipRestoreFocus = false;
			}
		}}
		class="top-[9vh] flex max-h-[78vh] w-full max-w-[calc(100%-2rem)] translate-y-0 flex-col gap-0 overflow-hidden rounded-[18px] bg-[var(--popover)] p-0 shadow-[inset_0_0_0_1px_var(--exec-border-hi),0_24px_64px_color-mix(in_srgb,var(--shade)_38%,transparent)] ring-0 motion-reduce:animate-none motion-reduce:transition-none motion-reduce:duration-0 motion-reduce:data-closed:animate-none motion-reduce:data-open:animate-none sm:max-w-[650px]"
	>
		<Dialog.Title class="sr-only" text="Command palette and Org AI" />
		<Dialog.Description class="sr-only">
			Search projects, work, team members, run commands, or ask Org AI.
		</Dialog.Description>

		<div
			class="flex items-center justify-between gap-3 border-b border-[var(--exec-border)] bg-[var(--exec-well)] px-4 py-3 text-[var(--exec-dim)]"
		>
			<div
				class="inline-flex items-center gap-1 rounded-[10px] bg-[var(--exec-surface)] p-0.5 shadow-[inset_0_0_0_1px_var(--exec-border)]"
				role="tablist"
			>
				<button
					type="button"
					role="tab"
					aria-selected={mode === 'cmd'}
					onclick={() => (mode = 'cmd')}
					class={[
						'inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-semibold transition-colors',
						mode === 'cmd'
							? 'bg-[var(--exec-accent-soft)] text-[var(--exec-ink)] shadow-[inset_0_0_0_1px_var(--exec-accent-ring)]'
							: 'text-[var(--exec-muted)] hover:text-[var(--exec-ink)]'
					]}
				>
					<Search class="size-3.5" aria-hidden="true" />
					<span>Search &amp; Commands</span>
				</button>
				<button
					type="button"
					role="tab"
					aria-selected={mode === 'ai'}
					onclick={() => (mode = 'ai')}
					class={[
						'inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-semibold transition-colors',
						mode === 'ai'
							? 'bg-[var(--exec-accent-soft)] text-[var(--exec-ink)] shadow-[inset_0_0_0_1px_var(--exec-accent-ring)]'
							: 'text-[var(--exec-muted)] hover:text-[var(--exec-ink)]'
					]}
				>
					<Sparkles class="size-3.5" aria-hidden="true" />
					<span>Ask Org AI</span>
				</button>
			</div>
			<Dialog.Close class={cn(kbdClass, 'px-2 py-1 text-[11px]')}>ESC</Dialog.Close>
		</div>

		<div class="flex items-center gap-3 border-b border-[var(--exec-border)] px-4 py-3.5">
			{#if mode === 'ai'}
				<Sparkles class="size-5 shrink-0 text-[var(--exec-accent-ink)]" aria-hidden="true" />
			{:else}
				<Search class="size-5 shrink-0 text-[var(--exec-accent-ink)]" aria-hidden="true" />
			{/if}
			<input
				bind:this={inputEl}
				bind:value={query}
				oninput={() => (activeIdx = 0)}
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
					class="rounded-lg bg-[var(--exec-accent)] px-3.5 py-1.5 text-xs font-semibold text-[var(--exec-on-accent)] disabled:cursor-not-allowed disabled:opacity-45"
				>
					Ask
				</button>
			{/if}
		</div>

		<div class="min-h-0 flex-1 overflow-y-auto p-2.5">
			{#if mode === 'cmd'}
				{#if entries.length === 0}
					<p class="py-8 text-center text-sm text-[var(--exec-muted)]">
						No matches for “{query}”. Switch to Ask Org AI for a live summary.
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
									onmouseenter={() => (activeIdx = idx)}
									onclick={() => runEntry(entry)}
									class={[
										'flex w-full items-center gap-3 rounded-[10px] px-3 py-2.5 text-left text-sm transition-colors',
										idx === safeActiveIdx
											? 'bg-[var(--exec-accent-soft)] text-[var(--exec-ink)] shadow-[inset_0_0_0_1px_var(--exec-accent-ring)]'
											: 'text-[var(--exec-body)] hover:bg-[var(--exec-accent-soft)] hover:text-[var(--exec-ink)] hover:shadow-[inset_0_0_0_1px_var(--exec-accent-ring)]'
									]}
								>
									{#if entry.group === 'Projects'}
										<Folder class="size-4 shrink-0" aria-hidden="true" />
									{:else if entry.group === 'Team'}
										<Users class="size-4 shrink-0" aria-hidden="true" />
									{:else if entry.group === 'Work'}
										<Inbox class="size-4 shrink-0" aria-hidden="true" />
									{:else}
										<Search class="size-4 shrink-0" aria-hidden="true" />
									{/if}
									<span class="min-w-0 flex-1 truncate font-medium">{entry.label}</span>
									<span class="hidden truncate text-xs text-[var(--exec-dim)] sm:inline"
										>{entry.sub}</span
									>
									<span
										class="shrink-0 rounded-[5px] bg-[var(--exec-well)] px-[7px] py-[1px] text-[0.6875rem] font-medium text-[var(--exec-muted)]"
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
									class="rounded px-1.5 py-0.5 text-[11px] text-[var(--exec-muted)] hover:text-[var(--exec-ink)]"
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
							<span
								class="text-[11px] font-semibold tracking-wider text-[var(--exec-dim)] uppercase"
							>
								Earlier
							</span>
							{#each earlierAi as turn (turn.query)}
								<button
									type="button"
									onclick={() => submitAi(turn.query)}
									class="inline-flex items-center gap-1.5 rounded-full bg-[var(--exec-well)] px-2.5 py-1 text-xs font-medium text-[var(--exec-body)] shadow-[inset_0_0_0_1px_var(--exec-border)] transition-colors hover:bg-[var(--exec-accent-soft)] hover:text-[var(--exec-ink)] hover:shadow-[inset_0_0_0_1px_var(--exec-accent-ring)]"
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
								class="mb-2 text-[11px] font-semibold tracking-wider text-[var(--exec-dim)] uppercase"
							>
								{aiHistory.length > 0 ? 'Ask a follow-up' : 'Suggested executive questions'}
							</div>
							<div class="flex flex-wrap gap-2">
								{#each followUps as starter (starter)}
									<button
										type="button"
										onclick={() => submitAi(starter)}
										class="inline-flex items-center gap-1.5 rounded-full bg-[var(--exec-well)] px-3 py-1.5 text-xs font-medium text-[var(--exec-body)] shadow-[inset_0_0_0_1px_var(--exec-border)] transition-colors hover:bg-[var(--exec-accent-soft)] hover:text-[var(--exec-ink)] hover:shadow-[inset_0_0_0_1px_var(--exec-accent-ring)]"
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
			class="flex flex-wrap items-center gap-4 border-t border-[var(--exec-border)] bg-[var(--exec-well)] px-4 py-2 text-[11px] text-[var(--exec-dim)]"
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
	.exec-cmdk-input::placeholder {
		color: var(--exec-muted);
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
		font-weight: 450;
		letter-spacing: -0.005em;
		transition:
			color 0.14s ease,
			background-color 0.14s ease;
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
			min-height: 60px;
			padding-block: 8px;
		}
		.exec-cmdk-bar {
			height: auto;
			min-height: 54px;
		}
		.exec-cmdk-trigger,
		.exec-ai-btn {
			height: auto;
			min-height: 44px;
		}
	}
</style>
