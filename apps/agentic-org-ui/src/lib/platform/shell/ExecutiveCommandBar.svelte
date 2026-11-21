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
	import type { OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import { answerExecutiveQuestion, type OrgAiAnswer } from '$lib/home/model';
	import { nonOwners } from '$lib/org/model';
	import { UI_THEMES, uiPrefs } from './ui-preferences.svelte';

	let {
		snapshot,
		items = [],
		offices = [],
		ownerId = 'owner'
	}: {
		snapshot?: OrgSnapshot;
		items?: WorkItemSummary[];
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

	let open = $state(false);
	let mode = $state<'cmd' | 'ai'>('cmd');
	let query = $state('');
	let activeIdx = $state(0);
	let aiHistory = $state<OrgAiAnswer[]>([]);
	let inputEl = $state<HTMLInputElement | null>(null);

	const AI_STARTERS = [
		'What needs me first?',
		'Which projects need attention?',
		'Team & office status',
		'Where is task time going?'
	];

	const entries = $derived.by<CommandEntry[]>(() => {
		const q = query.trim().toLowerCase();
		const list: CommandEntry[] = [
			{ id: 'go-home', group: 'Navigate', label: 'Go to Home', sub: 'Executive briefing & Spotlight', href: '/home' },
			{ id: 'go-inbox', group: 'Navigate', label: 'Go to Inbox', sub: 'Decisions waiting on you', href: '/inbox' },
			{ id: 'go-projects', group: 'Navigate', label: 'Go to Projects', sub: 'Portfolio health & teams', href: '/projects' },
			{ id: 'go-work', group: 'Navigate', label: 'Go to Work', sub: 'Tasks, bugs & requests', href: '/work' },
			{ id: 'go-offices', group: 'Navigate', label: 'Go to Offices', sub: 'Connected machines & status', href: '/offices' },
			{ id: 'go-settings', group: 'Navigate', label: 'Go to Settings', sub: 'Themes & feature flags', href: '/settings' },
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
					href: `/projects/${p.id}`
				});
			}
			for (const m of nonOwners(snapshot).slice(0, 12)) {
				list.push({
					id: `mem-${m.id}`,
					group: 'Team',
					label: m.name,
					sub: m.agent?.role ?? m.kind,
					href: '/org-chart'
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

		if (!q) return list.slice(0, 14);
		return list
			.filter((e) => e.label.toLowerCase().includes(q) || e.sub.toLowerCase().includes(q))
			.slice(0, 12);
	});

	const currentAi = $derived(aiHistory[0]);
	const earlierAi = $derived(aiHistory.slice(1, 5));
	const askedSet = $derived(new Set(aiHistory.map((h) => h.query.toLowerCase())));
	const followUps = $derived(AI_STARTERS.filter((s) => !askedSet.has(s.toLowerCase())));

	async function openModal(nextMode: 'cmd' | 'ai') {
		mode = nextMode;
		open = true;
		activeIdx = 0;
		await tick();
		inputEl?.focus();
	}

	function closeModal() {
		open = false;
		query = '';
	}

	function runEntry(entry: CommandEntry) {
		closeModal();
		if (entry.action) entry.action();
		else if (entry.href) void goto(resolve(entry.href as '/'));
	}

	function submitAi(raw: string) {
		const trimmed = raw.trim();
		if (!trimmed || !snapshot) return;
		const ans = answerExecutiveQuestion(trimmed, { snapshot, items, offices, ownerId });
		aiHistory = [ans, ...aiHistory.filter((x) => x.query.toLowerCase() !== ans.query.toLowerCase())];
		query = '';
	}

	function handleWindowKeydown(event: KeyboardEvent) {
		if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
			event.preventDefault();
			if (open) closeModal();
			else void openModal('cmd');
			return;
		}
		if (open && event.key === 'Escape') {
			event.preventDefault();
			closeModal();
			return;
		}
		if (open && (event.metaKey || event.ctrlKey) && event.key === '/') {
			event.preventDefault();
			mode = mode === 'cmd' ? 'ai' : 'cmd';
			return;
		}
		if (
			!open &&
			uiPrefs.singleKeyShortcuts &&
			event.key === '/' &&
			!event.metaKey &&
			!event.ctrlKey &&
			!event.altKey &&
			!(event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement)
		) {
			event.preventDefault();
			void openModal('cmd');
		}
	}

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
			activeIdx = entries.length > 0 ? (activeIdx + 1) % entries.length : 0;
		} else if (event.key === 'ArrowUp') {
			event.preventDefault();
			activeIdx = entries.length > 0 ? (activeIdx - 1 + entries.length) % entries.length : 0;
		} else if (event.key === 'Enter' && entries[activeIdx]) {
			event.preventDefault();
			runEntry(entries[activeIdx]);
		}
	}
</script>

<svelte:window onkeydown={handleWindowKeydown} />

<header class="exec-topbar sticky top-0 z-30 flex h-[68px] shrink-0 items-center justify-center px-6">
	<div class="exec-cmdk-bar flex h-[46px] w-full max-w-[640px] items-center rounded-[14px] p-1.5">
		<button
			type="button"
			onclick={() => void openModal('cmd')}
			aria-haspopup="dialog"
			aria-expanded={open}
			class="exec-cmdk-trigger flex h-full min-w-0 flex-1 items-center gap-2.5 rounded-[10px] px-3 text-left text-sm"
		>
			<Search class="size-[17px] shrink-0" aria-hidden="true" />
			<span class="min-w-0 flex-1 truncate">Search, run a command, or ask Org AI…</span>
			<kbd class="exec-kbd hidden sm:inline-flex">⌘K</kbd>
		</button>
		<span class="exec-cmdk-sep mx-1.5 h-5 w-px shrink-0" aria-hidden="true"></span>
		<button
			type="button"
			onclick={() => void openModal('ai')}
			class="exec-ai-btn inline-flex h-[34px] shrink-0 items-center gap-1.5 rounded-[9px] px-3.5 text-xs font-semibold"
		>
			<Sparkles class="size-3.5 shrink-0" aria-hidden="true" />
			<span>Ask AI</span>
		</button>
	</div>
</header>

{#if open}
	<div
		role="presentation"
		class="exec-modal-backdrop fixed inset-0 z-50 flex items-start justify-center px-4 pt-[9vh]"
		onclick={(e) => {
			if (e.target === e.currentTarget) closeModal();
		}}
	>
		<div
			role="dialog"
			aria-modal="true"
			aria-label="Command palette and Org AI"
			class="exec-modal flex max-h-[78vh] w-full max-w-[650px] flex-col overflow-hidden rounded-[18px]"
		>
			<div class="exec-modal-hd flex items-center justify-between gap-3 px-4 py-3">
				<div class="exec-tab-group inline-flex items-center gap-1 rounded-[10px] p-0.5" role="tablist">
					<button
						type="button"
						role="tab"
						aria-selected={mode === 'cmd'}
						onclick={() => (mode = 'cmd')}
						class={['exec-tab inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-semibold', mode === 'cmd' && 'is-on']}
					>
						<Search class="size-3.5" aria-hidden="true" />
						<span>Search &amp; Commands</span>
					</button>
					<button
						type="button"
						role="tab"
						aria-selected={mode === 'ai'}
						onclick={() => (mode = 'ai')}
						class={['exec-tab inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-semibold', mode === 'ai' && 'is-on']}
					>
						<Sparkles class="size-3.5" aria-hidden="true" />
						<span>Ask Org AI</span>
					</button>
				</div>
				<button type="button" onclick={closeModal} class="exec-kbd px-2 py-1 text-[11px]">ESC</button>
			</div>

			<div class="exec-modal-in flex items-center gap-3 px-4 py-3.5">
				{#if mode === 'ai'}
					<Sparkles class="size-5 shrink-0" style="color: var(--exec-accent-ink);" aria-hidden="true" />
				{:else}
					<Search class="size-5 shrink-0" style="color: var(--exec-accent-ink);" aria-hidden="true" />
				{/if}
				<input
					bind:this={inputEl}
					bind:value={query}
					onkeydown={handleInputKeydown}
					type="search"
					placeholder={mode === 'ai'
						? 'Ask Org AI — what needs you, project risks, or office status…'
						: 'Search projects, work, team, or jump anywhere…'}
					class="min-w-0 flex-1 bg-transparent text-[15px] outline-none"
					style="color: var(--exec-ink);"
				/>
				{#if mode === 'ai'}
					<button
						type="button"
						disabled={!query.trim()}
						onclick={() => submitAi(query)}
						class="exec-ask-go rounded-lg px-3.5 py-1.5 text-xs font-semibold"
					>
						Ask
					</button>
				{/if}
			</div>

			<div class="min-h-0 flex-1 overflow-y-auto p-2.5">
				{#if mode === 'cmd'}
					{#if entries.length === 0}
						<p class="py-8 text-center text-sm" style="color: var(--exec-muted);">
							No matches for “{query}”. Switch to Ask Org AI for a live summary.
						</p>
					{:else}
						<ul class="flex flex-col gap-0.5" role="listbox" aria-label="Search results">
							{#each entries as entry, idx (entry.id)}
								<li>
									<button
										type="button"
										role="option"
										aria-selected={idx === activeIdx}
										onmouseenter={() => (activeIdx = idx)}
										onclick={() => runEntry(entry)}
										class={['exec-opt flex w-full items-center gap-3 rounded-[10px] px-3 py-2.5 text-left text-sm', idx === activeIdx && 'is-on']}
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
										<span class="hidden truncate text-xs sm:inline" style="color: var(--exec-dim);">{entry.sub}</span>
										<span class="exec-tag shrink-0">{entry.group}</span>
									</button>
								</li>
							{/each}
						</ul>
					{/if}
				{:else}
					<div class="flex flex-col gap-3 p-1">
						{#if currentAi}
							<div class="exec-ai-card relative overflow-hidden rounded-xl p-4 pl-5">
								<div class="mb-2 flex items-center justify-between gap-2 text-xs font-semibold" style="color: var(--exec-accent-ink);">
									<span class="inline-flex items-center gap-1.5">
										<Sparkles class="size-3.5" aria-hidden="true" />
										<span>Org AI · “{currentAi.query}”</span>
									</span>
									<button
										type="button"
										onclick={() => (aiHistory = [])}
										class="rounded px-1.5 py-0.5 text-[11px]"
										style="color: var(--exec-muted);"
									>
										Clear
									</button>
								</div>
								<p class="text-sm font-semibold" style="color: var(--exec-ink);">{currentAi.headline}</p>
								<ul class="mt-2 flex flex-col gap-1.5 text-sm" style="color: var(--exec-body);">
									{#each currentAi.bullets as bullet (bullet.text)}
										<li class="flex flex-wrap items-baseline gap-x-2">
											<span>• {bullet.text}</span>
											{#if bullet.href && bullet.linkLabel}
												<a
													href={resolve(bullet.href as '/')}
													onclick={closeModal}
													class="text-xs font-semibold underline underline-offset-2"
													style="color: var(--exec-accent-ink);"
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
								<span class="text-[11px] font-semibold tracking-wider uppercase" style="color: var(--exec-dim);">
									Earlier
								</span>
								{#each earlierAi as turn (turn.query)}
									<button
										type="button"
										onclick={() => submitAi(turn.query)}
										class="exec-chip inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium"
									>
										<Clock class="size-3" aria-hidden="true" />
										<span>{turn.query}</span>
									</button>
								{/each}
							</div>
						{/if}

						{#if followUps.length > 0}
							<div>
								<div class="mb-2 text-[11px] font-semibold tracking-wider uppercase" style="color: var(--exec-dim);">
									{aiHistory.length > 0 ? 'Ask a follow-up' : 'Suggested executive questions'}
								</div>
								<div class="flex flex-wrap gap-2">
									{#each followUps as starter (starter)}
										<button
											type="button"
											onclick={() => submitAi(starter)}
											class="exec-chip inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs font-medium"
										>
											<Sparkles class="size-3.5" style="color: var(--exec-accent-ink);" aria-hidden="true" />
											<span>{starter}</span>
										</button>
									{/each}
								</div>
							</div>
						{/if}
					</div>
				{/if}
			</div>

			<div class="exec-modal-foot flex flex-wrap items-center gap-4 px-4 py-2 text-[11px]" aria-hidden="true">
				<span><kbd class="exec-kbd mr-1">↵</kbd> Run / Ask</span>
				<span><kbd class="exec-kbd mr-1">⌘/</kbd> Switch Search &amp; AI</span>
				<span><kbd class="exec-kbd mr-1">Esc</kbd> Close</span>
			</div>
		</div>
	</div>
{/if}

<style>
	.exec-topbar {
		background: transparent;
	}
	.exec-cmdk-bar {
		background: var(--exec-surface);
		backdrop-filter: var(--exec-blur);
		-webkit-backdrop-filter: var(--exec-blur);
		box-shadow: var(--exec-shadow);
	}
	.exec-cmdk-trigger {
		color: var(--exec-muted);
	}
	.exec-cmdk-trigger:hover {
		color: var(--exec-ink);
	}
	.exec-cmdk-sep {
		background: var(--exec-border);
	}
	.exec-kbd {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		height: 22px;
		padding: 0 7px;
		border-radius: 6px;
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		font-family: inherit;
		font-size: 0.7rem;
		font-weight: 600;
		color: var(--exec-muted);
	}
	.exec-ai-btn {
		background: var(--exec-accent-soft);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
		color: var(--exec-accent-ink);
	}
	.exec-ai-btn:hover {
		background: var(--exec-accent);
		color: #ffffff;
	}
	.exec-modal-backdrop {
		background: rgba(4, 8, 18, 0.52);
		backdrop-filter: blur(8px);
		-webkit-backdrop-filter: blur(8px);
	}
	.exec-modal {
		background: var(--popover);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi), 0 24px 64px rgba(0, 0, 0, 0.38);
	}
	.exec-modal-hd,
	.exec-modal-foot {
		background: var(--exec-well);
		color: var(--exec-dim);
	}
	.exec-modal-hd {
		border-bottom: 1px solid var(--exec-border);
	}
	.exec-modal-foot {
		border-top: 1px solid var(--exec-border);
	}
	.exec-modal-in {
		border-bottom: 1px solid var(--exec-border);
	}
	.exec-tab-group {
		background: var(--exec-surface);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.exec-tab {
		color: var(--exec-muted);
	}
	.exec-tab.is-on {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.exec-ask-go {
		background: var(--exec-accent);
		color: #ffffff;
	}
	.exec-ask-go:disabled {
		opacity: 0.45;
		cursor: not-allowed;
	}
	.exec-opt {
		color: var(--exec-body);
	}
	.exec-opt.is-on,
	.exec-opt:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.exec-tag {
		padding: 1px 7px;
		border-radius: 5px;
		background: var(--exec-well);
		font-size: 0.6875rem;
		font-weight: 500;
		color: var(--exec-muted);
	}
	.exec-ai-card {
		background: var(--exec-accent-soft);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.exec-ai-card::before {
		content: '';
		position: absolute;
		left: 0;
		top: 0;
		bottom: 0;
		width: 3px;
		background: var(--exec-accent);
	}
	.exec-chip {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		color: var(--exec-body);
	}
	.exec-chip:hover {
		background: var(--exec-accent-soft);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
		color: var(--exec-ink);
	}
</style>
