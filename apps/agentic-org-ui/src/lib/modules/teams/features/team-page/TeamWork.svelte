<!--
  @component
  A team's Work tab (Prototype Release 1149b4f1, offices-and-teams-ux §7.2):
  its tasks as divided work rows with scoped search (`/` focuses, `Escape` keeps query intact and focuses heading).
-->
<script lang="ts">
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import ScopedSearchInput from '$lib/components/ScopedSearchInput.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { WorkRows } from '$lib/modules/members';
	import { teamsApi } from '../../api';
	import { teamPageContext } from './context';
	import { reloadOnChange } from './reload.svelte';

	const team = teamPageContext();

	const work = new Resource(() => teamsApi.work(team.organizationId, team.teamId));
	reloadOnChange(work, () => team.loads);

	let query = $state('');

	$effect(() => {
		if (work.state === 'ready' && work.data) {
			team.setOpenWorkCount(work.data.filter((r) => r.state !== 'done').length);
		}
	});

	function filterWorkRows<
		T extends { ref: string; title: string; project?: string; assignee?: string; state: string }
	>(rows: T[], rawQuery: string): T[] {
		const needle = rawQuery.trim().toLowerCase();
		if (!needle) return rows;
		return rows.filter((r) =>
			[r.ref, r.title, r.project, r.assignee, r.state]
				.filter((part): part is string => Boolean(part))
				.some((part) => part.toLowerCase().includes(needle))
		);
	}
</script>

<Loaded
	errorLevel={4}
	resource={work}
	label="the team's work"
	error="Couldn't load the team's work"
>
	{#snippet children(rows)}
		{#if rows.length === 0}
			<EmptyState
				level={4}
				title="No work yet"
				description="Tasks {team.teamName} is accountable for, or that are assigned to it, show here."
			/>
		{:else}
			{@const filtered = filterWorkRows(rows, query)}
			<div
				class="flex flex-wrap items-center justify-between gap-3 border-b border-line-subtle px-4 py-3 min-[640px]:px-6 min-[1024px]:px-8"
			>
				<ScopedSearchInput
					bind:value={query}
					label="Search work in {team.teamName}"
					placeholder="Search work…"
					headingSelector="#team-heading, #shell-page-title, main h1, main h2"
				/>
				<p class="text-xs/5 text-fg-muted tabular-nums" role="status">
					{#if query.trim()}
						{filtered.length} of {rows.length} {rows.length === 1 ? 'item' : 'items'}
					{:else}
						{rows.length} {rows.length === 1 ? 'item' : 'items'}
					{/if}
				</p>
			</div>
			{#if filtered.length === 0}
				<p class="px-4 py-8 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
					No work matches this search.
				</p>
			{:else}
				<WorkRows rows={filtered} linked={false} />
			{/if}
		{/if}
	{/snippet}
</Loaded>
