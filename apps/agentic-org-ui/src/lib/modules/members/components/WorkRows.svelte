<!--
  @component
  Work as divided rows (offices-and-teams-ux §7.2): reference and title, then project · assignee · priority, and one
  trailing work-state pill. `hideAssignee` on a member's own Work tab, where the assignee is always them. `linked={false}`
  where the work item page can't open the rows yet: the title is then text, not a link to a page that isn't built.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import type { WorkRow, WorkState } from '../types';

	let {
		rows,
		hideAssignee = false,
		linked = true
	}: { rows: WorkRow[]; hideAssignee?: boolean; linked?: boolean } = $props();
	const STATE: Record<WorkState, { word: string; class: string }> = {
		todo: { word: 'To do', class: '' },
		in_progress: {
			word: 'In progress',
			class: 'bg-brand/10 text-brand-text ring-brand/30'
		},
		in_review: { word: 'In review', class: 'bg-brand/10 text-brand-text ring-brand/30' },
		blocked: { word: 'Blocked', class: 'bg-danger/10 text-danger-text ring-danger/20' },
		done: { word: 'Done', class: 'bg-success/10 text-success-text ring-success/20' }
	};
	const PRIORITY = {
		urgent: 'Urgent',
		high: 'High priority',
		medium: 'Medium priority',
		low: 'Low priority'
	} as const;
</script>

<ul role="list" class="divide-y divide-line-subtle">
	{#each rows as row (row.ref)}
		<li
			class="relative flex flex-wrap items-center justify-between gap-x-4 gap-y-2 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8"
		>
			<div class="min-w-0 flex-1 basis-60">
				<p class="text-sm/6 font-semibold text-fg">
					{#if linked}
						<a
							href={resolve(`/work/${row.ref}` as '/')}
							class="rounded-sm focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus"
							><span class="absolute inset-0" aria-hidden="true"></span><span
								class="font-mono font-normal text-fg-muted">{row.ref}</span
							>
							<span class="[overflow-wrap:anywhere]">{row.title}</span></a
						>
					{:else}
						<span class="font-mono font-normal text-fg-muted">{row.ref}</span>
						<span class="[overflow-wrap:anywhere]">{row.title}</span>
					{/if}
				</p>
				<p class="mt-1 flex flex-wrap gap-x-2 text-xs/5 text-fg-muted">
					{#each [row.project, hideAssignee ? undefined : (row.assignee ?? 'Unassigned'), row.priority && PRIORITY[row.priority]].filter(Boolean) as part, i (i)}
						{#if i > 0}<span aria-hidden="true">·</span>{/if}<span>{part}</span>
					{/each}
				</p>
			</div>
			<StatusPill label={STATE[row.state].word} class={STATE[row.state].class} />
		</li>
	{/each}
</ul>
