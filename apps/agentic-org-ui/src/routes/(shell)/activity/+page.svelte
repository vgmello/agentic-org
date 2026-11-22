<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import Activity from '@lucide/svelte/icons/activity';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Clock from '@lucide/svelte/icons/clock';
	import type { WorkItemSummary } from '$lib/proto';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { isItemDone, relativeAge } from '$lib/home/model';
	import { ActivityFeed } from '$lib/modules/activity';
	import { pageTitle } from '$lib/nav';
	import { memberById } from '$lib/org/model';
	import { uiPrefs } from '$lib/platform/shell/ui-preferences.svelte';
	import { workApi } from '$lib/work';

	let { data } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const snapshot = $derived(data.snapshot);
	const projectsById = $derived(new Map((snapshot.projects ?? []).map((p) => [p.id, p.name])));

	let items = $state.raw<WorkItemSummary[] | undefined>(undefined);
	let kindFilter = $state<'all' | 'task' | 'bug' | 'request'>('all');
	let nowMs = $state(Date.now());

	const feed = $derived.by(() => {
		const list = (items ?? []).filter((i) => !i.deleted);
		const filtered = kindFilter === 'all' ? list : list.filter((i) => i.kind === kindFilter);
		return [...filtered].sort((a, b) => Date.parse(b.updated_at) - Date.parse(a.updated_at));
	});

	onMount(() => {
		if (!uiPrefs.executiveUi) return;
		void workApi
			.snapshot(organizationId)
			.then((res) => {
				items = res.items;
				nowMs = Date.now();
			})
			.catch(() => {
				items = [];
			});
	});
</script>

<svelte:head><title>{pageTitle('Activity')}</title></svelte:head>

{#if uiPrefs.executiveUi}
	<div class="mx-auto flex w-full max-w-[1100px] flex-col gap-5 px-6 py-6 sm:px-8">
		<div class="flex flex-wrap items-center justify-between gap-4">
			<div>
				<h1
					id="shell-page-title"
					tabindex="-1"
					class="text-2xl font-semibold tracking-tight focus:outline-none"
					style="color: var(--exec-ink);"
				>
					Activity
				</h1>
				<p class="mt-0.5 text-sm" style="color: var(--exec-muted);">
					Latest updates across tasks, bugs, and requests in {data.connection.organization.name}.
				</p>
			</div>
			<div
				class="exec-well inline-flex items-center gap-1 p-1"
				role="tablist"
				aria-label="Filter activity"
			>
				{#each [['all', 'All'], ['task', 'Tasks'], ['bug', 'Bugs'], ['request', 'Requests']] as [val, label] (val)}
					<button
						type="button"
						role="tab"
						aria-selected={kindFilter === val}
						onclick={() => (kindFilter = val as typeof kindFilter)}
						class={['act-tab rounded-lg px-3 py-1.5 text-xs font-semibold', kindFilter === val && 'is-on']}
					>
						{label}
					</button>
				{/each}
			</div>
		</div>

		{#if items === undefined}
			<div class="exec-card p-6 text-sm" style="color: var(--exec-muted);">Loading activity…</div>
		{:else if feed.length === 0}
			<div class="exec-card flex flex-col items-center gap-2 px-6 py-16 text-center">
				<Activity class="size-8" style="color: var(--exec-muted);" aria-hidden="true" />
				<h2 class="text-base font-semibold" style="color: var(--exec-ink);">No activity yet</h2>
				<p class="max-w-sm text-sm" style="color: var(--exec-muted);">
					Work updates from your agents and teammates will appear here chronologically.
				</p>
			</div>
		{:else}
			<ul class="exec-card divide-y divide-line-subtle overflow-hidden">
				{#each feed as item (item.id)}
					{@const actor = memberById(snapshot, item.assignee_id) ?? memberById(snapshot, item.created_by)}
					{@const proj = item.project_id ? projectsById.get(item.project_id) : undefined}
					{@const done = isItemDone(item)}
					{@const href = item.kind === 'request' ? `/work?request=${encodeURIComponent(item.id)}` : '/work'}
					<li>
						<a
							href={resolve(href as '/work')}
							class="flex items-start gap-3 px-5 py-3.5 transition-colors hover:bg-surface-raised"
						>
							<span class="mt-0.5 shrink-0">
								{#if done}
									<CheckCircle2 class="size-4" style="color: var(--exec-ok);" aria-hidden="true" />
								{:else if item.status === 'blocked'}
									<AlertTriangle class="size-4" style="color: var(--exec-warn);" aria-hidden="true" />
								{:else}
									<Clock class="size-4" style="color: var(--exec-accent-ink);" aria-hidden="true" />
								{/if}
							</span>
							<div class="min-w-0 flex-1">
								<div class="flex flex-wrap items-baseline gap-2">
									<span class="text-sm font-semibold" style="color: var(--exec-ink);">{item.title}</span>
									<span class="font-mono text-xs" style="color: var(--exec-dim);">{item.short_id}</span>
								</div>
								<p class="mt-0.5 text-xs" style="color: var(--exec-muted);">
									{actor?.name ?? 'Team'} · {item.status.replace('_', ' ')}{proj ? ` · ${proj}` : ''}
								</p>
							</div>
							<span class="shrink-0 text-xs tabular-nums" style="color: var(--exec-dim);">
								{relativeAge(item.updated_at, nowMs)}
							</span>
							<ChevronRight class="mt-0.5 size-4 shrink-0" style="color: var(--exec-dim);" aria-hidden="true" />
						</a>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
{:else}
	<PageHeader
		title="Activity"
		description="What your teams and agents are doing, newest first. Decisions show under the Decisions kind."
	/>
	<ActivityFeed organizationId={data.connection.organization.id} syncUrl />
{/if}

<style>
	.act-tab {
		color: var(--exec-muted);
	}
	.act-tab.is-on {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
</style>
