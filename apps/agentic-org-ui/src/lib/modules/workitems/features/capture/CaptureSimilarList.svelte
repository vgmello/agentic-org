<!--
  @component
  Similar open bugs or requests warning box inside CaptureDialog (BUG-3, RQS-13).
-->
<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { mobileButton } from '$lib/components/classes';
	import type { TaskRow, TaskType } from '../../types';

	let {
		type,
		similar,
		addingId,
		busy,
		onadd
	}: {
		type: TaskType;
		similar: readonly TaskRow[];
		addingId: string | null;
		busy: boolean;
		onadd: (row: TaskRow) => void;
	} = $props();
</script>

{#if similar.length > 0}
	<div
		class="flex flex-col gap-2 rounded-lg bg-surface-raised p-3 ring-1 ring-line"
		data-testid="similar-items"
	>
		<p class="text-xs/5 font-semibold text-fg">
			{type === 'bug' ? 'Similar open bugs' : 'Similar open requests'}
		</p>
		<ul role="list" class="flex flex-col gap-2">
			{#each similar as item (item.id)}
				<li class="flex flex-wrap items-center justify-between gap-2 text-xs/5">
					<div class="min-w-0 flex-1">
						<span class="font-mono text-fg-muted">{item.ref}</span>
						<span class="ml-1.5 text-fg">{item.title}</span>
						<span class="ml-1.5 text-fg-muted">
							({item.project ? item.project.name : 'No project'} · {item.team.name})
						</span>
					</div>
					<Button
						type="button"
						variant="outline"
						size="sm"
						class={mobileButton}
						disabled={addingId !== null || busy}
						onclick={() => onadd(item)}
					>
						{#if addingId === item.id}
							Adding…
						{:else if type === 'bug'}
							This is the same bug
						{:else}
							Add to {item.ref}
						{/if}
					</Button>
				</li>
			{/each}
		</ul>
	</div>
{/if}
