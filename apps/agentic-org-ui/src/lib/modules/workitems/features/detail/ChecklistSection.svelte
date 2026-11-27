<!--
  @component
  Task/bug checklist (work-items-ux §9.1; CHK-1..4, CHK-7):
  Inline + Add step supporting Enter for next step and pasted multi-line steps (1–200 chars), tick provenance,
  Required toggle with lock icon and "Required" word, reorder via drag, buttons and Alt+↑/↓ with live announcement.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import { toAppError } from '$lib/proto';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { errorClass, inputClass, mobileButton } from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import { linkText, splitSteps, stepError, taskHref, when } from '../../present';
	import type { ChecklistStep } from '../../types';
	import WorkIcon from '../../components/WorkIcon.svelte';

	let {
		organizationId,
		taskId,
		steps,
		canEdit = true,
		stale = false,
		onchanged
	}: {
		organizationId: string;
		taskId: string;
		steps: ChecklistStep[];
		canEdit?: boolean;
		stale?: boolean;
		onchanged: () => void;
	} = $props();

	const uid = $props.id();
	let draftText = $state('');
	let adding = $state(false);
	let addError = $state<string | undefined>();
	let stepBusy = $state<string | null>(null);
	let announcement = $state('');
	let dragIndex = $state<number | null>(null);
	let inputRef = $state<HTMLInputElement | null>(null);

	const editable = $derived(canEdit && !stale);
	const doneCount = $derived(steps.filter((s) => s.done).length);
	const requiredLeft = $derived(steps.filter((s) => s.required && !s.done).length);

	let announceTimer: ReturnType<typeof setTimeout> | undefined;

	function announce(text: string) {
		clearTimeout(announceTimer);
		announcement = '';
		announceTimer = setTimeout(() => {
			announcement = text;
		}, 10);
	}

	$effect(() => () => clearTimeout(announceTimer));

	async function submitLines(lines: string[], keepFocus = false) {
		addError = stepError(lines);
		if (addError) return;
		adding = true;
		try {
			await workItemsApi.addSteps(organizationId, taskId, lines);
			draftText = '';
			addError = undefined;
			announce(
				lines.length === 1 ? `Added step: ${lines[0]}` : `Added ${lines.length} checklist steps.`
			);
			onchanged();
			if (keepFocus) inputRef?.focus();
		} catch (e) {
			// Preserve unsaved text on conflict/failure (CHK-2 / §9.1).
			addError = toAppError(e).message;
		} finally {
			adding = false;
		}
	}

	async function handleAddSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (adding || !editable) return;
		await submitLines(splitSteps(draftText), true);
	}

	function handlePaste(event: ClipboardEvent) {
		const pasted = event.clipboardData?.getData('text') ?? '';
		if (!pasted.includes('\n')) return;
		event.preventDefault();
		const lines = splitSteps(draftText ? `${draftText}\n${pasted}` : pasted);
		void submitLines(lines, true);
	}

	async function toggleDone(step: ChecklistStep, nextDone: boolean) {
		if (!editable || stepBusy) return;
		stepBusy = step.id;
		try {
			await workItemsApi.setStep(organizationId, taskId, step.id, {
				done: nextDone,
				expectedRevision: step.revision
			});
			announce(nextDone ? `Checked: ${step.text}` : `Unchecked: ${step.text}`);
			onchanged();
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			stepBusy = null;
		}
	}

	async function toggleRequired(step: ChecklistStep) {
		if (!editable || stepBusy) return;
		stepBusy = step.id;
		try {
			await workItemsApi.setStep(organizationId, taskId, step.id, {
				required: !step.required,
				expectedRevision: step.revision
			});
			announce(
				!step.required ? `Marked required: ${step.text}` : `Unmarked required: ${step.text}`
			);
			onchanged();
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			stepBusy = null;
		}
	}

	async function reorder(from: number, to: number) {
		if (!editable || stepBusy || to < 0 || to >= steps.length || from === to) return;
		const step = steps[from];
		stepBusy = step.id;
		try {
			await workItemsApi.moveStep(organizationId, taskId, step.id, to);
			announce(`Moved "${step.text}" to position ${to + 1} of ${steps.length}.`);
			onchanged();
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			stepBusy = null;
		}
	}

	async function remove(step: ChecklistStep) {
		if (!editable || stepBusy) return;
		stepBusy = step.id;
		try {
			await workItemsApi.removeStep(organizationId, taskId, step.id);
			announce(`Removed step: ${step.text}`);
			onchanged();
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			stepBusy = null;
		}
	}

	function handleStepKeydown(event: KeyboardEvent, index: number) {
		if (!event.altKey || !editable) return;
		if (event.key === 'ArrowUp' && index > 0) {
			event.preventDefault();
			void reorder(index, index - 1);
		} else if (event.key === 'ArrowDown' && index < steps.length - 1) {
			event.preventDefault();
			void reorder(index, index + 1);
		}
	}
</script>

<div class="sr-only" aria-live="polite" aria-atomic="true" data-testid="checklist-live-region">
	{announcement}
</div>

<section
	aria-labelledby="{uid}-heading"
	class="flex flex-col gap-3 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
	data-testid="checklist-section"
>
	<div class="flex flex-wrap items-baseline justify-between gap-2">
		<h3 id="{uid}-heading" class="text-sm/6 font-semibold text-fg">
			Checklist
			{#if steps.length > 0}
				<span class="ml-1.5 text-xs/5 font-normal text-fg-muted tabular-nums">
					{doneCount}/{steps.length}
				</span>
			{/if}
		</h3>
		{#if requiredLeft > 0}
			<span class="inline-flex items-center gap-1 text-xs/5 text-fg-secondary">
				<WorkIcon name="lock" class="size-3.5 text-fg-muted" />
				<span>{requiredLeft} required left</span>
			</span>
		{/if}
	</div>

	{#if steps.length === 0}
		<p class="text-sm/6 text-fg-muted">No checklist steps yet.</p>
	{:else}
		<ol class="divide-y divide-line-subtle" data-testid="checklist-items">
			{#each steps as step, index (step.id)}
				<!-- drag-and-drop reorder paired with keyboard up/down buttons --> <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
				<li
					draggable={editable}
					ondragstart={() => (dragIndex = index)}
					ondragover={(e) => editable && e.preventDefault()}
					ondrop={(e) => {
						e.preventDefault();
						if (dragIndex !== null) void reorder(dragIndex, index);
						dragIndex = null;
					}}
					onkeydown={(e) => handleStepKeydown(e, index)}
					data-testid="checklist-step"
					data-step-id={step.id}
					class="flex flex-wrap items-start justify-between gap-3 py-2.5"
				>
					<div class="flex min-w-0 flex-1 items-start gap-2.5">
						<Checkbox
							id="{uid}-step-{step.id}"
							checked={step.done}
							disabled={!editable || stepBusy === step.id}
							onCheckedChange={(checked) => toggleDone(step, checked === true)}
							class="mt-1"
						/>
						<div class="flex min-w-0 flex-1 flex-col gap-1">
							<label
								for="{uid}-step-{step.id}"
								class="text-sm/6 [overflow-wrap:anywhere] {step.done
									? 'text-fg-muted line-through'
									: 'text-fg'}"
							>
								{step.text}
							</label>

							<div class="flex flex-wrap items-center gap-x-2.5 gap-y-1 text-xs/5 text-fg-muted">
								{#if step.required}
									<span
										class="inline-flex items-center gap-1 text-fg-secondary"
										data-testid="step-required-badge"
									>
										<WorkIcon name="lock" class="size-3 text-fg-muted" />
										<span>Required</span>
									</span>
								{/if}
								{#if step.checked}
									<span data-testid="step-checked-by">
										Checked by {step.checked.by.name} · {when(step.checked.at)}
									</span>
								{/if}
								{#if step.linkedTask}
									<a
										href={resolve(taskHref(step.linkedTask.ref) as '/')}
										class="text-link hover:text-brand-text hover:underline"
									>
										{linkText(step.linkedTask)}
									</a>
								{/if}
							</div>
						</div>
					</div>

					{#if editable}
						<div class="flex shrink-0 items-center gap-1">
							<button
								type="button"
								aria-label="Move step up: {step.text}"
								disabled={index === 0 || stepBusy !== null}
								onclick={() => reorder(index, index - 1)}
								class="rounded-md px-1.5 py-1 text-xs/5 text-fg-muted hover:bg-surface-overlay hover:text-fg disabled:opacity-40 max-[639px]:min-h-11"
							>
								<WorkIcon name="up" class="size-4" />
							</button>
							<button
								type="button"
								aria-label="Move step down: {step.text}"
								disabled={index === steps.length - 1 || stepBusy !== null}
								onclick={() => reorder(index, index + 1)}
								class="rounded-md px-1.5 py-1 text-xs/5 text-fg-muted hover:bg-surface-overlay hover:text-fg disabled:opacity-40 max-[639px]:min-h-11"
							>
								<WorkIcon name="down" class="size-4" />
							</button>
							<button
								type="button"
								disabled={stepBusy !== null}
								onclick={() => toggleRequired(step)}
								class="rounded-md px-2 py-1 text-xs/5 text-fg-secondary hover:bg-surface-overlay hover:text-fg max-[639px]:min-h-11"
							>
								{step.required ? 'Optional' : 'Mark required'}
							</button>
							<button
								type="button"
								aria-label="Remove step: {step.text}"
								disabled={stepBusy !== null}
								onclick={() => remove(step)}
								class="rounded-md px-2 py-1 text-xs/5 text-fg-muted hover:bg-surface-overlay hover:text-danger-text max-[639px]:min-h-11"
							>
								Remove
							</button>
						</div>
					{/if}
				</li>
			{/each}
		</ol>
	{/if}

	{#if editable}
		<form class="flex flex-col gap-2 pt-1" onsubmit={handleAddSubmit} novalidate>
			<div class="flex flex-wrap gap-2">
				<div class="min-w-0 flex-1">
					<label for="{uid}-add-step" class="sr-only">Add step</label>
					<Input
						id="{uid}-add-step"
						bind:ref={inputRef}
						bind:value={draftText}
						onpaste={handlePaste}
						disabled={adding}
						aria-invalid={addError ? 'true' : undefined}
						aria-describedby={addError ? `${uid}-add-err` : undefined}
						placeholder="+ Add step (Enter to add, paste lines for multiple)"
						class={inputClass}
					/>
				</div>
				<Button type="submit" variant="outline" class={mobileButton} disabled={adding}>
					{adding ? 'Adding…' : 'Add step'}
				</Button>
			</div>
			{#if addError}
				<p id="{uid}-add-err" class={errorClass} role="alert">{addError}</p>
			{/if}
		</form>
	{/if}
</section>
