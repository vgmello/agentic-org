<script lang="ts">
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { newId } from '$lib/modules/organization/commands';
	import { WORK_LIMITS, type WorkItem } from '$lib/proto';
	import type { WorkDraft } from '$lib/modules/work';

	let {
		item,
		busy,
		actionBtn,
		newStep = $bindable(''),
		newRequired = $bindable(false),
		onchange
	}: {
		item: WorkItem;
		busy: boolean;
		actionBtn: string;
		newStep: string;
		newRequired: boolean;
		onchange: (draft: WorkDraft) => Promise<boolean>;
	} = $props();

	async function addStep(e: SubmitEvent) {
		e.preventDefault();
		if (newStep.trim() === '') return;
		if (
			await onchange({
				method: 'checklist.add',
				item_id: item.summary.id,
				steps: [{ step_id: newId(), text: newStep.trim(), required: newRequired }]
			})
		) {
			newStep = '';
			newRequired = false;
		}
	}
</script>

<section class="flex flex-col gap-2" aria-labelledby="task-checklist">
	<h3 id="task-checklist" class="text-xs/5 font-semibold tracking-wider text-fg-muted uppercase">
		Checklist {#if item.checklist.length > 0}<span class="font-normal text-fg-muted tabular-nums"
				>{item.summary.checklist.done} of {item.summary.checklist.total}</span
			>{/if}
	</h3>
	<ul
		class="divide-y divide-line-subtle rounded-lg bg-surface-raised outline-1 -outline-offset-1 outline-line"
		data-testid="checklist"
	>
		{#each item.checklist as step (step.id)}
			<li class="flex items-center gap-x-3 px-3 py-2 text-sm/6">
				<Checkbox
					checked={step.done}
					disabled={busy || !!step.task_id}
					aria-label={step.text}
					onCheckedChange={(on) =>
						void onchange({ method: 'checklist.check', step_id: step.id, done: on === true })}
				/>
				<span
					class="min-w-0 flex-1 [overflow-wrap:anywhere] {step.done
						? 'text-fg-muted line-through'
						: 'text-fg'}">{step.text}</span
				>
				{#if step.required}<Badge variant="secondary">Required</Badge>{/if}
				<button
					type="button"
					class="rounded p-1 text-fg-muted hover:text-fg focus-visible:outline-2 focus-visible:outline-focus"
					aria-label={`Remove step ${step.text}`}
					disabled={busy}
					onclick={() =>
						void onchange({
							method: 'checklist.remove',
							step_id: step.id,
							expected_version: step.version
						})}><Trash2 class="size-4" aria-hidden="true" /></button
				>
			</li>
		{:else}
			<li class="px-3 py-2 text-sm/6 text-fg-muted">No steps yet.</li>
		{/each}
	</ul>
	<form class="flex gap-x-2" onsubmit={addStep}>
		<Input
			aria-label="New step"
			placeholder="Add a step"
			bind:value={newStep}
			maxlength={WORK_LIMITS.stepText * 2}
			disabled={busy}
			autocomplete="off"
		/>
		<label class="flex shrink-0 items-center gap-x-1.5 text-sm/6 text-fg-secondary"
			><Checkbox bind:checked={newRequired} aria-label="Required" /> Required</label
		>
		<Button
			type="submit"
			variant="outline"
			class={actionBtn}
			disabled={busy || newStep.trim() === ''}>Add step</Button
		>
	</form>
</section>
