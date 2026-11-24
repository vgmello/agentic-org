<!--
  @component
  The Inbox screen (our-org-in-the-app §6.1): tabs All, Approvals, Questions, Decisions, Problems and Handled,
  one Show menu for team and project, and the item list beside the selected item's card from 960 px. Below
  960 px an item opens as its own view with an Inbox back link. The tab, Show filters and selected item live in
  the address (`?tab=&team=&project=&item=`), so Back, Forward and a link from Home land on the same view.
  A resolved item moves to Handled; a refused action keeps it open and says why.
-->
<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { MediaQuery } from 'svelte/reactivity';
	import { toast } from 'svelte-sonner';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import ErrorState from '$lib/components/ErrorState.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import { Button } from '$lib/components/ui/button';
	import {
		DropdownMenu,
		DropdownMenuContent,
		DropdownMenuLabel,
		DropdownMenuRadioGroup,
		DropdownMenuRadioItem,
		DropdownMenuSeparator,
		DropdownMenuTrigger
	} from '$lib/components/ui/dropdown-menu';
	import { accountZone, ZoneNotice } from '$lib/modules/preferences';
	import { inboxApi } from '../../api';
	import { neighbour, openCount, scopeOptions, tabCounts, tabItems } from '../../items';
	import { TABS, type InboxItem, type InboxRequest, type InboxTab } from '../../types';
	import InboxItemCard from '../card/InboxItemCard.svelte';
	import InboxListRow from '../list/InboxListRow.svelte';

	let { organizationId }: { organizationId: string } = $props();

	const wide = new MediaQuery('min-width: 960px');
	let now = $state(new Date());
	let phase = $state<'loading' | 'ready' | 'error'>('loading');
	let failure = $state<string | undefined>();
	let retrying = $state(false);
	let items = $state.raw<InboxItem[]>([]);
	let notice = $state('');

	async function load(initial: boolean) {
		if (initial) phase = 'loading';
		try {
			const next = await inboxApi.list(organizationId);
			const before = new Set(items.filter((i) => i.state === 'open').map((i) => i.id));
			const cleared = next.filter((i) => i.state === 'handled' && before.has(i.id));
			items = next;
			phase = 'ready';
			if (!initial && cleared.length > 0)
				notice = `${cleared.length === 1 ? `“${cleared[0].title}”` : `${cleared.length} items`} was resolved elsewhere and moved to Handled.`;
		} catch (e) {
			if (!initial) return;
			failure = e instanceof Error ? e.message : undefined;
			phase = 'error';
		} finally {
			retrying = false;
		}
	}
	onMount(() => {
		void accountZone.ensure(organizationId);
		void load(true);
		const tick30 = setInterval(() => (now = new Date()), 30_000);
		let stop: (() => void) | undefined;
		let gone = false;
		inboxApi
			.subscribe(organizationId, () => void load(false))
			.then((unsubscribe) => (gone ? unsubscribe() : (stop = unsubscribe)))
			.catch(() => undefined);
		return () => {
			gone = true;
			stop?.();
			clearInterval(tick30);
		};
	});

	// The view lives in the address.
	const params = $derived(page.url.searchParams);
	const itemParam = $derived(params.get('item') ?? undefined);
	const scope = $derived({
		team: params.get('team') || undefined,
		project: params.get('project') || undefined
	});
	const selectedFromUrl = $derived(items.find((i) => i.id === itemParam));
	const tab = $derived.by<InboxTab>(() => {
		const asked = params.get('tab');
		const known = TABS.find((t) => t.id === asked)?.id;
		return known ?? (selectedFromUrl?.state === 'handled' ? 'handled' : 'all');
	});
	const counts = $derived(tabCounts(items, scope));
	const list = $derived(tabItems(items, tab, scope));
	const selected = $derived(selectedFromUrl ?? (wide.current ? list[0] : undefined));
	const options = $derived(scopeOptions(items));
	const filtersOn = $derived((scope.team ? 1 : 0) + (scope.project ? 1 : 0));
	const showingDetail = $derived(!wide.current && !!selectedFromUrl);

	function href(next: { tab?: InboxTab; item?: string; team?: string; project?: string }) {
		// eslint-disable-next-line svelte/prefer-svelte-reactivity -- a local value, not reactive state
		const q = new URLSearchParams();
		const t = 'tab' in next ? next.tab : tab;
		if (t && t !== 'all') q.set('tab', t);
		const team = 'team' in next ? next.team : scope.team;
		const project = 'project' in next ? next.project : scope.project;
		if (team) q.set('team', team);
		if (project) q.set('project', project);
		if (next.item) q.set('item', next.item);
		const text = q.toString();
		return `/inbox${text ? `?${text}` : ''}`;
	}
	const go = (target: string, replaceState: boolean) =>
		goto(resolve(target as '/'), { replaceState, keepFocus: true, noScroll: true });

	function choose(id: string) {
		void go(href({ tab: id as InboxTab, item: undefined }), true);
	}
	function tabKey(event: KeyboardEvent) {
		const index = TABS.findIndex((t) => t.id === tab);
		let next = -1;
		if (event.key === 'ArrowRight') next = (index + 1) % TABS.length;
		else if (event.key === 'ArrowLeft') next = (index - 1 + TABS.length) % TABS.length;
		else if (event.key === 'Home') next = 0;
		else if (event.key === 'End') next = TABS.length - 1;
		if (next < 0) return;
		event.preventDefault();
		choose(TABS[next].id);
		void tick().then(() => document.getElementById(`inbox-tab-${TABS[next].id}`)?.focus());
	}

	const PAST: Record<InboxRequest['type'], string> = {
		approve: 'Approved',
		deny: 'Denied',
		reply: 'Reply sent',
		choose: 'Decision recorded',
		dismiss: 'Dismissed'
	};

	async function act(item: InboxItem, request: InboxRequest) {
		const updated = await inboxApi.act(organizationId, item.id, request);
		const before = list;
		items = items.map((i) => (i.id === updated.id ? updated : i));
		toast.success(`${PAST[request.type]}. “${item.title}” moved to Handled.`);
		// Removing an item focuses the next one, else the previous, else the tabs (app-shell-ux §6a).
		const next = neighbour(before, item.id);
		await go(href({ item: wide.current ? next?.id : undefined }), true);
		await tick();
		const row = next && document.querySelector<HTMLElement>(`[data-item="${next.id}"] a`);
		(row ?? document.getElementById(`inbox-tab-${tab}`))?.focus();
	}

	const emptyCopy: Record<InboxTab, { title: string; description: string }> = {
		all: {
			title: 'Nothing needs you',
			description: 'When an agent needs your approval, an answer or a decision, it shows up here.'
		},
		approvals: {
			title: 'No approvals waiting',
			description: 'Runs, permissions and hand-offs that need your yes or no show up here.'
		},
		questions: {
			title: 'No questions waiting',
			description:
				'When an agent asks you something and is waiting for your answer, it shows up here.'
		},
		decisions: {
			title: 'No decisions waiting',
			description: 'Escalations and proposals with choices show up here.'
		},
		problems: {
			title: 'Nothing is blocked on you',
			description: 'Failed runs and work that is stuck on you show up here.'
		},
		snoozed: {
			title: 'Nothing snoozed',
			description: 'Items you snooze until later appear here with their return time.'
		},
		handled: {
			title: 'Nothing handled yet',
			description: 'Items you or someone else resolved move here.'
		}
	};
</script>

{#if showingDetail && selected}
	<div class="px-4 pt-6 pb-8 min-[640px]:px-6" data-testid="inbox-detail">
		<a
			href={resolve(href({ item: undefined }) as '/')}
			class="mb-4 inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
			>Inbox</a
		>
		{#key selected.id}
			<InboxItemCard item={selected} {now} {act} headingLevel={2} />
		{/key}
	</div>
{:else}
	<PageHeader
		title="Inbox"
		count={phase === 'ready' ? openCount(items) : undefined}
		countLabel="open items"
		description="What your agents need from you. An item moves to Handled when it is resolved, wherever that happens."
	/>

	<div
		class="flex items-start gap-x-4 border-b border-line pr-4 min-[640px]:pr-6 min-[1024px]:pr-8"
	>
		<div
			role="tablist"
			aria-label="Inbox"
			class="flex min-w-0 flex-1 gap-x-6 overflow-x-auto px-4 min-[640px]:px-6 min-[1024px]:px-8"
		>
			{#each TABS as t (t.id)}
				<button
					type="button"
					role="tab"
					id="inbox-tab-{t.id}"
					aria-selected={tab === t.id}
					aria-controls="inbox-panel"
					tabindex={tab === t.id ? 0 : -1}
					onclick={() => choose(t.id)}
					onkeydown={tabKey}
					class="-mb-px flex min-h-11 items-center gap-x-2 border-b-2 py-3 text-sm/6 font-semibold whitespace-nowrap focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus {tab ===
					t.id
						? 'border-brand text-fg'
						: 'border-transparent text-fg-muted hover:border-line-strong hover:text-fg'}"
				>
					{t.label}
					{#if phase === 'ready'}<span
							class="rounded-full bg-surface-overlay px-2 text-xs/5 font-medium text-fg-secondary"
							><span class="sr-only">, </span>{counts[t.id]}</span
						>{/if}
				</button>
			{/each}
		</div>
		<div class="py-1.5">
			<DropdownMenu>
				<DropdownMenuTrigger>
					{#snippet child({ props })}
						<Button
							{...props}
							variant="outline"
							class="max-[639px]:min-h-11"
							aria-label={filtersOn
								? `Show, ${filtersOn} ${filtersOn === 1 ? 'filter' : 'filters'} on`
								: 'Show'}
						>
							Show{#if filtersOn}<span
									class="rounded-full bg-primary/20 px-1.5 text-xs/5 text-brand-text-strong"
									>{filtersOn}</span
								>{/if}
							<MiniIcon name="chevronDown" class="size-5 text-fg-muted" />
						</Button>
					{/snippet}
				</DropdownMenuTrigger>
				<DropdownMenuContent
					preventScroll={false}
					align="end"
					class="w-60 max-w-[calc(100vw-2rem)]"
				>
					<DropdownMenuLabel>Team</DropdownMenuLabel>
					<DropdownMenuRadioGroup
						value={scope.team ?? ''}
						onValueChange={(id) => void go(href({ team: id || undefined, item: undefined }), true)}
					>
						<DropdownMenuRadioItem value="" class="min-h-9 max-[639px]:min-h-11"
							>All teams</DropdownMenuRadioItem
						>
						{#each options.teams as team (team.id)}
							<DropdownMenuRadioItem value={team.id} class="min-h-9 max-[639px]:min-h-11"
								>{team.name}</DropdownMenuRadioItem
							>
						{/each}
					</DropdownMenuRadioGroup>
					<DropdownMenuSeparator />
					<DropdownMenuLabel>Project</DropdownMenuLabel>
					<DropdownMenuRadioGroup
						value={scope.project ?? ''}
						onValueChange={(id) =>
							void go(href({ project: id || undefined, item: undefined }), true)}
					>
						<DropdownMenuRadioItem value="" class="min-h-9 max-[639px]:min-h-11"
							>All projects</DropdownMenuRadioItem
						>
						{#each options.projects as project (project.id)}
							<DropdownMenuRadioItem value={project.id} class="min-h-9 max-[639px]:min-h-11"
								>{project.name}</DropdownMenuRadioItem
							>
						{/each}
					</DropdownMenuRadioGroup>
				</DropdownMenuContent>
			</DropdownMenu>
		</div>
	</div>
	<ZoneNotice />
	<p class="sr-only" role="status">{notice}</p>

	<div id="inbox-panel" role="tabpanel" aria-labelledby="inbox-tab-{tab}">
		{#if phase === 'loading'}
			<SkeletonRows label="the Inbox" rows={4} avatar />
		{:else if phase === 'error'}
			<ErrorState
				title="Couldn't load the Inbox"
				message={failure ?? 'The office did not answer. Try again.'}
				{retrying}
				onretry={() => {
					retrying = true;
					void load(true);
				}}
			/>
		{:else if list.length === 0}
			{#if filtersOn && items.length > 0}
				<div
					class="px-4 py-12 text-center min-[640px]:px-6 min-[1024px]:px-8"
					data-testid="no-match"
				>
					<p class="text-sm/6 text-fg-secondary">
						Nothing in {TABS.find((t) => t.id === tab)?.label} matches the Show filters.
					</p>
					<Button
						variant="outline"
						class="mt-4 max-[639px]:min-h-11"
						onclick={() =>
							void go(href({ team: undefined, project: undefined, item: undefined }), true)}
						>Clear filters</Button
					>
				</div>
			{:else}
				<EmptyState title={emptyCopy[tab].title} description={emptyCopy[tab].description} />
			{/if}
		{:else}
			<div class="min-[960px]:grid min-[960px]:grid-cols-[minmax(22rem,28rem)_minmax(0,1fr)]">
				<ul
					role="list"
					aria-label={TABS.find((t) => t.id === tab)?.label}
					class="divide-y divide-line-subtle border-line-subtle min-[960px]:border-r"
				>
					{#each list as item (item.id)}
						<InboxListRow
							{item}
							{now}
							href={href({ item: item.id })}
							current={wide.current && selected?.id === item.id}
						/>
					{/each}
				</ul>
				{#if wide.current && selected}
					<div
						class="px-6 py-6 min-[960px]:sticky min-[960px]:top-16 min-[960px]:self-start min-[1024px]:px-8"
						data-testid="inbox-detail"
					>
						{#key selected.id}
							<InboxItemCard item={selected} {now} {act} headingLevel={3} />
						{/key}
					</div>
				{/if}
			</div>
		{/if}
	</div>
{/if}
