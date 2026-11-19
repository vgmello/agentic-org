<!--
  @component
  A team's Work tab (§7.2), the default: its tasks as divided rows, reference and title, project · assignee ·
  priority, and one trailing work-state pill. Rows don't repeat the team. Rows don't link yet: the work item page
  can't open a task, and a link to a page that says it isn't built is a functioning-looking no-op (§6a). The members
  module's WorkRows always links; once the work item page exists, use it here again.
-->
<script lang="ts">
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import type { WorkState } from '$lib/modules/members';
	import { teamsApi } from '../../api';
	import { teamPageContext } from './context';
	import { fact, factsList, factsWrap } from './facts';
	import { reloadOnChange } from './reload.svelte';

	const team = teamPageContext();

	const work = new Resource(() => teamsApi.work(team.organizationId, team.teamId));
	reloadOnChange(work, () => team.loads);

	const STATE: Record<WorkState, { word: string; class: string }> = {
		todo: { word: 'To do', class: '' },
		in_progress: {
			word: 'In progress',
			class: 'bg-indigo-400/10 text-indigo-300 ring-indigo-400/30'
		},
		in_review: { word: 'In review', class: 'bg-indigo-400/10 text-indigo-300 ring-indigo-400/30' },
		blocked: { word: 'Blocked', class: 'bg-red-400/10 text-red-300 ring-red-400/20' },
		done: { word: 'Done', class: 'bg-green-400/10 text-green-300 ring-green-400/20' }
	};
	const PRIORITY = {
		urgent: 'Urgent',
		high: 'High priority',
		medium: 'Medium priority',
		low: 'Low priority'
	} as const;
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
			<ul role="list" class="divide-y divide-white/5">
				{#each rows as row (row.ref)}
					<li
						class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8"
					>
						<div class="min-w-0 flex-1 basis-60">
							<p class="text-sm/6 font-semibold text-white">
								<span class="font-mono font-normal text-gray-400">{row.ref}</span>
								<span class="[overflow-wrap:anywhere]">{row.title}</span>
							</p>
							<div class="mt-1">
								<div class={factsWrap}>
									<ul role="list" class="{factsList} text-xs/5 text-gray-400">
										{#if row.project}<li class={fact}>{row.project}</li>{/if}
										<li class={fact}>{row.assignee ?? 'Unassigned'}</li>
										{#if row.priority}<li class={fact}>{PRIORITY[row.priority]}</li>{/if}
									</ul>
								</div>
							</div>
						</div>
						<StatusPill label={STATE[row.state].word} class={STATE[row.state].class} />
					</li>
				{/each}
			</ul>
		{/if}
	{/snippet}
</Loaded>
