<!--
  @component
  Profile › Work (Prototype Release 1149b4f1, offices-and-teams-ux §7.2):
  the work assigned to this member, with scoped search (`/` focuses, `Escape` leaves typed query intact and
  focuses heading) and All / Open / Done filter for agents.
-->
<script lang="ts">
	import Loaded from '$lib/components/Loaded.svelte';
	import ScopedSearchInput from '$lib/components/ScopedSearchInput.svelte';
	import { Button } from '$lib/components/ui/button';
	import { mobileButton } from '$lib/components/classes';
	import { membersApi } from '../../api';
	import WorkRows from '../../components/WorkRows.svelte';
	import { profileContext } from '../profile/context.svelte';
	import { tabResource } from './reload.svelte';

	const ctx = profileContext();
	const work = tabResource((o, m) => membersApi.work(o, m));

	let query = $state('');
	let taskFilter = $state<'all' | 'open' | 'done'>('all');
	const isAgent = $derived(ctx.profile.kind === 'agent');
</script>

<Loaded
	resource={work}
	label="work"
	error="Couldn't load {ctx.profile.name}'s work"
	isEmpty={(rows) => rows.length === 0}
	emptyTitle="No work assigned"
	emptyDescription="Work items assigned to {ctx.profile.name} show here."
>
	{#snippet emptyAction()}
		{#if ctx.profile.status !== 'inactive'}
			<Button
				variant="outline"
				class={mobileButton}
				onclick={(e) => ctx.act('assign', e.currentTarget)}>Assign task</Button
			>
		{/if}
	{/snippet}
	{#snippet children(rows)}
		{@const openCount = rows.filter((r) => r.state !== 'done').length}
		{@const doneCount = rows.length - openCount}
		{@const needle = query.trim().toLowerCase()}
		{@const byState =
			isAgent && taskFilter !== 'all'
				? rows.filter((r) => (taskFilter === 'open' ? r.state !== 'done' : r.state === 'done'))
				: rows}
		{@const filtered = needle
			? byState.filter((r) =>
					[r.ref, r.title, r.project, r.state]
						.filter((part): part is string => Boolean(part))
						.some((part) => part.toLowerCase().includes(needle))
				)
			: byState}
		<div
			class="flex flex-wrap items-center justify-between gap-3 border-b border-line-subtle px-4 py-3 min-[640px]:px-6 min-[1024px]:px-8"
		>
			<div class="flex min-w-0 flex-1 flex-wrap items-center gap-3">
				<ScopedSearchInput
					bind:value={query}
					label={isAgent
						? `Search tasks for ${ctx.profile.name}`
						: `Search work for ${ctx.profile.name}`}
					placeholder={isAgent ? 'Search tasks…' : 'Search work…'}
					headingSelector="#profile-heading, #shell-page-title, main h1, main h2"
				/>
				{#if isAgent}
					<div
						role="group"
						aria-label="Filter tasks"
						class="inline-flex rounded-md bg-surface-raised p-0.5 ring-1 ring-line"
					>
						{#each [{ id: 'all' as const, label: 'All', count: rows.length }, { id: 'open' as const, label: 'Open', count: openCount }, { id: 'done' as const, label: 'Done', count: doneCount }] as opt (opt.id)}
							<button
								type="button"
								aria-pressed={taskFilter === opt.id}
								onclick={() => (taskFilter = opt.id)}
								class="inline-flex items-center gap-1.5 rounded-sm px-2.5 py-1 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11 {taskFilter ===
								opt.id
									? 'bg-surface-overlay text-fg'
									: 'text-fg-secondary hover:text-fg'}"
							>
								<span>{opt.label}</span>
								<span class="tabular-nums opacity-75">{opt.count}</span>
							</button>
						{/each}
					</div>
				{/if}
			</div>
			<p class="text-xs/5 text-fg-muted tabular-nums" role="status">
				{filtered.length} of {rows.length}
				{rows.length === 1 ? 'item' : 'items'}
			</p>
		</div>
		{#if filtered.length === 0}
			<p class="px-4 py-8 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
				{needle
					? 'No work matches this search.'
					: isAgent
						? 'No tasks match this filter.'
						: 'No work assigned.'}
			</p>
		{:else}
			<WorkRows rows={filtered} hideAssignee />
		{/if}
	{/snippet}
</Loaded>
