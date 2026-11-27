<!--
  @component
  Inline Acceptance criteria / checklist steps editor inside CaptureDialog.
-->
<script lang="ts">
	import Plus from '@lucide/svelte/icons/plus';
	import X from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { errorClass, inputClass } from '$lib/components/classes';
	import { splitSteps, stepError } from '../../present';

	let {
		uid,
		disabled,
		busy = false,
		draftSteps = $bindable<string[]>([]),
		stepInput = $bindable('')
	}: {
		uid: string;
		disabled: boolean;
		busy?: boolean;
		draftSteps: string[];
		stepInput: string;
	} = $props();

	let stepInputError = $state<string | undefined>();

	export function resetError() {
		stepInputError = undefined;
	}

	function appendDraftSteps(raw: string) {
		const lines = splitSteps(raw);
		const err = stepError(lines);
		if (err) {
			stepInputError = err;
			return;
		}
		draftSteps = [...draftSteps, ...lines];
		stepInput = '';
		stepInputError = undefined;
	}

	function handleStepKeydown(event: KeyboardEvent) {
		if (event.key !== 'Enter') return;
		event.preventDefault();
		if (stepInput.trim()) appendDraftSteps(stepInput);
	}

	function handleStepPaste(event: ClipboardEvent) {
		const pasted = event.clipboardData?.getData('text') ?? '';
		if (!pasted.includes('\n')) return;
		event.preventDefault();
		appendDraftSteps(stepInput ? `${stepInput}\n${pasted}` : pasted);
	}

	function removeDraftStep(index: number) {
		draftSteps = draftSteps.filter((_, i) => i !== index);
	}
</script>

<div class="flex flex-col gap-1.5">
	<Label for="{uid}-ac-step" class="block text-sm/6 font-medium text-fg">
		Acceptance criteria (optional)
	</Label>
	{#if draftSteps.length > 0}
		<ul role="list" class="flex flex-col gap-1" data-testid="composer-draft-steps">
			{#each draftSteps as stepText, idx (`${idx}-${stepText}`)}
				<li
					class="flex items-center justify-between gap-2 rounded-md bg-surface-raised px-2.5 py-1.5 text-xs/5 text-fg ring-1 ring-line"
				>
					<span class="min-w-0 flex-1 [overflow-wrap:anywhere]">{stepText}</span>
					<button
						type="button"
						aria-label="Remove step {idx + 1}"
						onclick={() => removeDraftStep(idx)}
						class="shrink-0 rounded p-0.5 text-fg-muted hover:text-fg"
					>
						<X class="size-3.5" aria-hidden="true" />
					</button>
				</li>
			{/each}
		</ul>
	{/if}
	<div class="flex items-center gap-2">
		<Input
			id="{uid}-ac-step"
			bind:value={stepInput}
			onkeydown={handleStepKeydown}
			onpaste={handleStepPaste}
			{disabled}
			placeholder="+ Add checklist step (Enter to add, paste lines for multiple)"
			class="flex-1 {inputClass}"
		/>
		<Button
			type="button"
			variant="outline"
			size="sm"
			disabled={busy || !stepInput.trim()}
			onclick={() => appendDraftSteps(stepInput)}
			class="shrink-0"
		>
			<Plus class="size-3.5" aria-hidden="true" />
			<span>Add</span>
		</Button>
	</div>
	{#if stepInputError}
		<p class={errorClass} role="alert">{stepInputError}</p>
	{/if}
</div>
