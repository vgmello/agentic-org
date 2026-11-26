<!--
  @component
  Work as divided rows (offices-and-teams-ux §7.2): reference and title, then project · assignee · priority, and one
  trailing work-state pill. `hideAssignee` on a member's own Work tab, where the assignee is always them. `linked={false}`
  where the work item page can't open the rows yet: the title is then text, not a link to a page that isn't built.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { WORK_PRIORITY_LABELS, WORK_STATE_LABELS } from '../present';
	import type { WorkRow, WorkState } from '../types';

	let {
		rows,
		hideAssignee = false,
		linked = true,
		archivedRefs,
		onopen
	}: {
		rows: WorkRow[];
		hideAssignee?: boolean;
		linked?: boolean;
		archivedRefs?: ReadonlySet<string>;
		onopen?: (row: WorkRow, triggerEl: HTMLElement | null) => void;
	} = $props();

	function handleOpen(event: MouseEvent, row: WorkRow) {
		if (
			!onopen ||
			event.defaultPrevented ||
			event.button !== 0 ||
			event.metaKey ||
			event.ctrlKey ||
			event.shiftKey ||
			event.altKey
		) {
			return;
		}
		event.preventDefault();
		onopen(row, event.currentTarget as HTMLElement | null);
	}
	const STATE_CLASS: Record<WorkState, string> = {
		todo: '',
		in_progress: 'bg-brand/10 text-brand-text ring-brand/30',
		in_review: 'bg-brand/10 text-brand-text ring-brand/30',
		blocked: 't-red',
		done: 't-green'
	};
</script>

<ul role="list" class="divide-y divide-line-subtle">
	{#each rows as row (row.ref)}
		<li
			data-ref={row.ref}
			class={[
				'relative flex flex-wrap items-center justify-between gap-x-4 gap-y-2 px-4 py-4 transition-colors duration-150 motion-reduce:transition-none min-[640px]:px-6 min-[1024px]:px-8',
				linked && 'hover:bg-surface-raised/60'
			]}
		>
			<div class="min-w-0 flex-1 basis-60">
				<p class="text-sm/6 font-semibold text-fg">
					{#if linked}
						<a
							href={resolve(`/work/${row.ref}` as '/')}
							onclick={(e) => handleOpen(e, row)}
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
					{#each [row.project, hideAssignee ? undefined : (row.assignee ?? 'Unassigned'), row.priority && WORK_PRIORITY_LABELS[row.priority]].filter(Boolean) as part, i (i)}
						{#if i > 0}<span aria-hidden="true">·</span>{/if}<span>{part}</span>
					{/each}
				</p>
			</div>
			<div class="flex shrink-0 items-center gap-1.5">
				{#if archivedRefs?.has(row.ref)}
					<StatusPill label="Archived" class="opacity-80" />
				{/if}
				<StatusPill label={WORK_STATE_LABELS[row.state]} class={STATE_CLASS[row.state]} />
			</div>
		</li>
	{/each}
</ul>
