<!--
  @component
  The Activity feed (our-org-in-the-app §6, §6.1): merged organization and work events, newest first, with
  team / member / project / kind filters and search, day headers, collapsing groups, runs that expand in place,
  live arrivals that never move the list while the owner is scrolled down ("n new · Jump to latest"), and the
  Open thread side panel. Every state is named: loading, error with Retry, empty organization, no match,
  messaging not connected, git paused.

  Embed it on a team page, project page or member profile by fixing the scope:
    <ActivityFeed {organizationId} scope={{ team: teamId }} />
  A key in `scope` narrows every row and its filter is not offered. `syncUrl` (the Activity page) keeps the
  filters in the address bar. Headings: day headers are `headingLevel` (3 by default, under the page's h2).
-->
<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import Callout from '$lib/components/Callout.svelte';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import ErrorState from '$lib/components/ErrorState.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { accountZone, ZoneNotice } from '$lib/modules/preferences';
	import { activityApi } from '../../api';
	import { matchesQuery, queryFromParams, queryToParams } from '../../filter';
	import { dayBlocks } from '../../group';
	import {
		KIND_FILTERS,
		type ActivityOptions,
		type ActivityPage,
		type ActivityQuery,
		type ActivityRow,
		type ActivityScope
	} from '../../types';
	import ActivityThreadPanel from '../thread/ActivityThreadPanel.svelte';
	import ActivityRowItem from './ActivityRowItem.svelte';
	import FilterMenu from './FilterMenu.svelte';

	let {
		organizationId,
		scope = {},
		syncUrl = false,
		pageSize = 25,
		headingLevel = 3
	}: {
		organizationId: string;
		scope?: ActivityScope;
		syncUrl?: boolean;
		pageSize?: number;
		headingLevel?: 3 | 4;
	} = $props();

	const uid = $props.id();
	const start = (): ActivityQuery => ({
		kind: 'all',
		...(syncUrl ? queryFromParams(page.url.searchParams) : {})
	});
	let filters = $state<ActivityQuery>(start());
	/** What is asked for: the filters narrowed by the fixed scope. */
	const query = $derived<ActivityQuery>({ ...filters, ...scope });

	// URL <-> filters, for the Activity page. Each side acts only when they differ, so they never loop.
	$effect(() => {
		if (!syncUrl) return;
		const fromUrl = queryToParams(queryFromParams(page.url.searchParams)).toString();
		if (fromUrl !== queryToParams(filters).toString())
			filters = queryFromParams(page.url.searchParams);
	});
	function changed(next: Partial<ActivityQuery>) {
		filters = { ...filters, ...next };
		if (!syncUrl) return;
		const params = queryToParams(filters).toString();
		if (params !== page.url.searchParams.toString())
			void goto(resolve(`/activity${params ? `?${params}` : ''}` as '/'), {
				replaceState: true,
				keepFocus: true,
				noScroll: true
			});
	}

	let now = $state(new Date());
	let phase = $state<'loading' | 'ready' | 'error'>('loading');
	let failure = $state<string | undefined>();
	let result = $state.raw<ActivityPage | undefined>();
	let options = $state.raw<ActivityOptions | undefined>();
	let rows = $state.raw<ActivityRow[]>([]);
	let reload = $state(0);
	const key = $derived(`${queryToParams(query)}|${reload}`);
	let grown = $state({ key: '', size: 0 });
	const limit = $derived(grown.key === key ? grown.size : pageSize);
	let loadingMore = $state(false);
	let retrying = $state(false);
	let request = 0;

	// Search waits for a pause in typing.
	let typed = $state(start().q ?? '');
	let searchTimer: ReturnType<typeof setTimeout> | undefined;
	$effect(() => {
		if ((filters.q ?? '') !== typed.trim() && document.activeElement?.id !== `${uid}-search`)
			typed = filters.q ?? '';
	});
	function typing(value: string) {
		typed = value;
		clearTimeout(searchTimer);
		searchTimer = setTimeout(() => changed({ q: value.trim() || undefined }), 250);
	}

	$effect(() => {
		const asked = { ...query };
		const size = limit;
		void reload;
		const mine = ++request;
		if (size <= pageSize) {
			phase = 'loading';
			pending = [];
		} else loadingMore = true;
		activityApi
			.feed(organizationId, asked, size)
			.then((page) => {
				if (mine !== request) return;
				result = page;
				rows = page.rows;
				phase = 'ready';
			})
			.catch((e) => {
				if (mine !== request) return;
				failure = e instanceof Error ? e.message : undefined;
				phase = 'error';
			})
			.finally(() => {
				if (mine === request) {
					loadingMore = false;
					retrying = false;
				}
			});
	});
	$effect(() => {
		activityApi
			.options(organizationId)
			.then((found) => (options = found))
			.catch(() => (options = undefined));
	});

	// Live arrivals. Scrolled down: queue them behind the pill and leave the list alone.
	let pending = $state.raw<ActivityRow[]>([]);
	let fresh = $state<Record<string, true>>({});
	let list = $state<HTMLElement | null>(null);
	function scroller(): HTMLElement | null {
		return list?.closest('main') ?? null;
	}
	function scrolledDown() {
		const holder = scroller();
		if (!list || !holder) return false;
		// The sticky top bar covers the first 64 px of the scroller.
		return list.getBoundingClientRect().top < holder.getBoundingClientRect().top + 56;
	}
	function arrived(row: ActivityRow) {
		if (!matchesQuery(row, query)) return;
		if (rows.some((r) => r.id === row.id) || pending.some((r) => r.id === row.id)) return;
		if (scrolledDown()) pending = [row, ...pending];
		else show([row]);
	}
	function show(incoming: ActivityRow[]) {
		rows = [...incoming, ...rows];
		for (const row of incoming) {
			fresh[row.id] = true;
			setTimeout(() => delete fresh[row.id], 2000);
		}
	}
	async function jump() {
		const incoming = pending;
		pending = [];
		show(incoming);
		const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
		scroller()?.scrollTo({ top: 0, behavior: reduce ? 'auto' : 'smooth' });
		await tick();
		list
			?.querySelector<HTMLElement>('[data-testid="activity-row"]')
			?.focus({ preventScroll: true });
	}
	onMount(() => {
		const tick30 = setInterval(() => (now = new Date()), 30_000);
		void accountZone.ensure(organizationId);
		let stop: (() => void) | undefined;
		let gone = false;
		activityApi
			.subscribe(organizationId, arrived)
			.then((unsubscribe) => (gone ? unsubscribe() : (stop = unsubscribe)))
			.catch(() => undefined);
		return () => {
			gone = true;
			stop?.();
			clearInterval(tick30);
			clearTimeout(searchTimer);
		};
	});

	// The Open thread panel.
	let threadOpen = $state(false);
	let thread = $state<{ id: string; title: string } | undefined>();
	let opener: HTMLElement | undefined;
	function openThread(row: ActivityRow, from: HTMLElement) {
		if (!row.thread) return;
		opener = from;
		thread = { id: row.thread.id, title: row.thread.with };
		threadOpen = true;
	}
	function returnFocus(event: Event) {
		if (opener?.isConnected) {
			event.preventDefault();
			opener.focus();
		}
	}

	// Groups the owner has opened.
	let openGroups = $state<Record<string, true>>({});

	const teamOptions = $derived([
		{ id: '', label: 'All teams' },
		...(options?.teams.map((t) => ({ id: t.id, label: t.name })) ?? [])
	]);
	const memberOptions = $derived([
		{ id: '', label: 'Everyone' },
		...(options?.members.map((m) => ({ id: m.id, label: m.name })) ?? [])
	]);
	const projectOptions = $derived([
		{ id: '', label: 'All projects' },
		...(options?.projects.map((p) => ({ id: p.id, label: p.name })) ?? [])
	]);
	const blocks = $derived(dayBlocks(rows, now, accountZone.zone));
	const filtered = $derived(
		filters.kind !== 'all' ||
			!!filters.q ||
			!!filters.team ||
			!!filters.member ||
			!!filters.project ||
			!!filters.includeStatus
	);
	const H = $derived(`h${headingLevel}` as 'h3' | 'h4');
</script>

<div data-testid="activity-feed">
	<div class="flex flex-wrap items-center gap-3 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8">
		{#if scope.team === undefined}
			<FilterMenu
				label="Team"
				value={filters.team ?? ''}
				options={teamOptions}
				onchange={(id) => changed({ team: id || undefined })}
			/>
		{/if}
		{#if scope.member === undefined}
			<FilterMenu
				label="Member"
				value={filters.member ?? ''}
				options={memberOptions}
				onchange={(id) => changed({ member: id || undefined })}
			/>
		{/if}
		{#if scope.project === undefined}
			<FilterMenu
				label="Project"
				value={filters.project ?? ''}
				options={projectOptions}
				onchange={(id) => changed({ project: id || undefined })}
			/>
		{/if}
		<FilterMenu
			label="Kind"
			value={filters.kind}
			options={KIND_FILTERS}
			onchange={(id) => changed({ kind: id as ActivityQuery['kind'] })}
		/>
		<div class="relative min-w-48 flex-1 max-[639px]:basis-full min-[640px]:max-w-xs">
			<Label for="{uid}-search" class="sr-only">Search activity</Label>
			<MiniIcon
				name="search"
				class="pointer-events-none absolute top-1/2 left-3 size-5 -translate-y-1/2 text-fg-muted"
			/>
			<Input
				id="{uid}-search"
				type="search"
				placeholder="Search activity"
				autocomplete="off"
				value={typed}
				oninput={(event) => typing(event.currentTarget.value)}
				class="max-[639px]:h-11 border-0 bg-surface-raised px-3 py-1.5 text-base text-fg shadow-none outline-1 outline-solid -outline-offset-1 outline-field-border sm:text-sm/6 md:text-sm/6 dark:bg-surface-raised placeholder:text-fg-muted focus-visible:ring-0 focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-focus pl-10"
			/>
		</div>
		<label
			class="flex min-h-9 items-center gap-x-2 text-sm/6 text-fg-secondary max-[639px]:min-h-11"
		>
			<input
				type="checkbox"
				checked={!!filters.includeStatus}
				onchange={(event) => changed({ includeStatus: event.currentTarget.checked || undefined })}
				class="size-4 accent-primary"
			/>
			Include status changes
		</label>
	</div>
	<ZoneNotice />

	{#if phase === 'loading'}
		<SkeletonRows label="activity" rows={6} avatar />
	{:else if phase === 'error'}
		<ErrorState
			title="Couldn't load activity"
			message={failure ?? 'The office did not answer. Try again.'}
			{retrying}
			onretry={() => {
				retrying = true;
				reload++;
			}}
		/>
	{:else if result}
		{#if result.sources.messaging === 'not-connected'}
			<div class="px-4 pt-2 pb-2 min-[640px]:px-6 min-[1024px]:px-8">
				<Callout data-testid="messaging-off">
					<p>Messages aren't connected yet, so agents' conversations don't show here.</p>
					<a
						href={resolve('/settings/offices')}
						class="mt-1 inline-block rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
						>How to connect</a
					>
				</Callout>
			</div>
		{/if}
		{#if result.sources.git === 'paused'}
			<p
				class="border-y border-line-subtle px-4 py-3 text-xs/5 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8"
				data-testid="git-paused"
			>
				Git activity paused: the project's repository isn't reachable.
			</p>
		{/if}
		<p class="sr-only" role="status">
			{#if filtered}{rows.length === 0
					? 'No activity matches.'
					: `${rows.length}${result.more ? ' or more' : ''} matching ${rows.length === 1 ? 'row' : 'rows'}.`}{/if}
		</p>

		{#if result.organizationEmpty}
			<EmptyState
				title="No activity yet"
				description="When your agents start working, everything they do shows up here."
			/>
		{:else}
			<div class="pointer-events-none sticky top-20 z-10 h-0 text-center">
				{#if pending.length > 0}
					<button
						type="button"
						onclick={jump}
						class="pointer-events-auto inline-flex min-h-9 items-center gap-x-2 rounded-full bg-primary px-4 text-sm/6 font-semibold text-on-emphasis hover:bg-primary/80 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
						data-testid="jump-latest"
					>
						{pending.length} new · Jump to latest
					</button>
				{/if}
			</div>
			<p class="sr-only" role="status" aria-live="polite">
				{pending.length > 0
					? `${pending.length} new ${pending.length === 1 ? 'row' : 'rows'} available`
					: ''}
			</p>

			{#if rows.length === 0}
				<div
					class="px-4 py-12 text-center min-[640px]:px-6 min-[1024px]:px-8"
					data-testid="no-match"
				>
					<p class="text-sm/6 text-fg-secondary">No activity matches these filters.</p>
					{#if filtered}
						<Button
							variant="outline"
							class="mt-4 max-[639px]:min-h-11"
							onclick={() => {
								typed = '';
								changed({
									kind: 'all',
									q: undefined,
									team: undefined,
									member: undefined,
									project: undefined,
									includeStatus: undefined
								});
							}}>Clear filters</Button
						>
					{/if}
				</div>
			{:else}
				<div bind:this={list} class="border-t border-line-subtle">
					{#each blocks as block (block.key)}
						<svelte:element
							this={H}
							class="border-b border-line-subtle bg-tint/[0.02] px-4 py-2 text-xs/5 font-semibold text-fg-secondary min-[640px]:px-6 min-[1024px]:px-8"
							>{block.label}</svelte:element
						>
						<ul
							role="list"
							class="divide-y divide-line-subtle border-b border-line-subtle"
							aria-label={block.label}
						>
							{#each block.entries as entry (entry.type === 'row' ? entry.row.id : entry.id)}
								{#if entry.type === 'row'}
									<ActivityRowItem
										row={entry.row}
										{now}
										fresh={!!fresh[entry.row.id]}
										onopenthread={openThread}
									/>
								{:else}
									<li
										data-testid="activity-group"
										class="px-4 py-3 min-[640px]:px-6 min-[1024px]:px-8"
									>
										<button
											type="button"
											aria-expanded={!!openGroups[entry.id]}
											onclick={() => {
												if (openGroups[entry.id]) delete openGroups[entry.id];
												else openGroups[entry.id] = true;
											}}
											class="flex min-h-9 w-full items-center gap-x-2 rounded-sm text-left text-sm/6 text-fg-secondary focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
										>
											<span class="min-w-0 flex-1 [overflow-wrap:anywhere]">{entry.summary}</span>
											<span class="shrink-0 font-semibold text-link"
												>{openGroups[entry.id] ? 'Hide' : 'Show'}</span
											>
											<MiniIcon
												name="chevronDown"
												class="size-4 shrink-0 text-fg-muted transition-transform motion-reduce:transition-none {openGroups[
													entry.id
												]
													? 'rotate-180'
													: ''}"
											/>
										</button>
										{#if openGroups[entry.id]}
											<ul
												role="list"
												class="mt-2 -mx-4 divide-y divide-line-subtle border-t border-line-subtle min-[640px]:-mx-6 min-[1024px]:-mx-8"
											>
												{#each entry.rows as row (row.id)}
													<ActivityRowItem {row} {now} onopenthread={openThread} />
												{/each}
											</ul>
										{/if}
									</li>
								{/if}
							{/each}
						</ul>
					{/each}
					{#if result.more > 0}
						<div
							class="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-4 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8"
							data-testid="earlier"
						>
							<span>Earlier: {result.more} more</span>
							<Button
								variant="outline"
								size="sm"
								class="max-[639px]:min-h-11"
								disabled={loadingMore}
								aria-busy={loadingMore}
								onclick={() => (grown = { key, size: limit + pageSize })}
								>{loadingMore ? 'Loading…' : 'Show'}</Button
							>
						</div>
					{/if}
				</div>
			{/if}
		{/if}
	{/if}
</div>

<ActivityThreadPanel
	bind:open={threadOpen}
	{organizationId}
	threadId={thread?.id}
	title={thread?.title ?? ''}
	onCloseAutoFocus={returnFocus}
/>
