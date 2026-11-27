<!--
  @component
  Bug-specific form fields for CaptureDialog (What happened, Severity radiogroup, Expected, Steps, Environment).
-->
<script lang="ts">
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { errorClass, inputClass, textareaClass } from '$lib/components/classes';
	import { SEVERITIES, SEVERITY_HELP, SEVERITY_WORD } from '../../present';
	import type { Severity } from '../../types';

	let {
		uid,
		disabled,
		checking = false,
		whatHappenedError,
		whatHappened = $bindable(''),
		expected = $bindable(''),
		steps = $bindable(''),
		environment = $bindable(''),
		severity = $bindable<Severity | null>(null),
		whatHappenedInput = $bindable<HTMLTextAreaElement | null>(null)
	}: {
		uid: string;
		disabled: boolean;
		checking?: boolean;
		whatHappenedError?: string;
		whatHappened: string;
		expected: string;
		steps: string;
		environment: string;
		severity: Severity | null;
		whatHappenedInput: HTMLTextAreaElement | null;
	} = $props();
</script>

<div>
	<Label for="{uid}-what" class="block text-sm/6 font-medium text-fg">What happened</Label>
	<textarea
		id="{uid}-what"
		bind:this={whatHappenedInput}
		rows={3}
		bind:value={whatHappened}
		disabled={disabled || checking}
		aria-invalid={whatHappenedError ? 'true' : undefined}
		aria-describedby={whatHappenedError ? `${uid}-what-err` : undefined}
		placeholder="What went wrong and where did you see it?"
		class="mt-1.5 {textareaClass}"></textarea>
	{#if whatHappenedError}
		<p id="{uid}-what-err" class={errorClass} role="alert">{whatHappenedError}</p>
	{/if}
</div>

<!-- Severity with all four explanations + Not set (BUG-4) -->
<fieldset class="flex flex-col gap-2">
	<legend class="text-sm/6 font-medium text-fg">Severity</legend>
	<div class="flex flex-col gap-1.5" role="radiogroup" aria-label="Severity">
		<label
			class="flex cursor-pointer items-start gap-2.5 rounded-md bg-surface-raised px-3 py-2 text-sm/6 ring-1 ring-line"
		>
			<input
				type="radio"
				name="{uid}-severity"
				value=""
				checked={severity === null}
				onchange={() => (severity = null)}
				{disabled}
				class="mt-1 accent-primary"
			/>
			<span>
				<span class="font-medium text-fg">Not set</span>
				<span class="block text-xs/5 text-fg-muted">Leave severity to triage.</span>
			</span>
		</label>
		{#each SEVERITIES as sev (sev)}
			<label
				class="flex cursor-pointer items-start gap-2.5 rounded-md bg-surface-raised px-3 py-2 text-sm/6 ring-1 ring-line"
			>
				<input
					type="radio"
					name="{uid}-severity"
					value={sev}
					checked={severity === sev}
					onchange={() => (severity = sev)}
					{disabled}
					class="mt-1 accent-primary"
				/>
				<span>
					<span class="font-medium text-fg">{SEVERITY_WORD[sev]}</span>
					<span class="block text-xs/5 text-fg-muted">{SEVERITY_HELP[sev]}</span>
				</span>
			</label>
		{/each}
	</div>
</fieldset>

<div class="grid gap-3 sm:grid-cols-2">
	<div>
		<Label for="{uid}-expected" class="block text-sm/6 font-medium text-fg">
			Expected result (optional)
		</Label>
		<textarea
			id="{uid}-expected"
			rows={2}
			bind:value={expected}
			{disabled}
			class="mt-1.5 {textareaClass}"></textarea>
	</div>
	<div>
		<Label for="{uid}-steps" class="block text-sm/6 font-medium text-fg">
			Steps to reproduce (optional)
		</Label>
		<textarea id="{uid}-steps" rows={2} bind:value={steps} {disabled} class="mt-1.5 {textareaClass}"
		></textarea>
	</div>
</div>

<div>
	<Label for="{uid}-env" class="block text-sm/6 font-medium text-fg">Environment (optional)</Label>
	<Input
		id="{uid}-env"
		bind:value={environment}
		{disabled}
		placeholder="Office, browser or branch/commit if known"
		class="mt-1.5 {inputClass}"
	/>
</div>
