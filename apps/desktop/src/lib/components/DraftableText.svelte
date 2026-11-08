<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Undo from '@lucide/svelte/icons/undo';
	import { Alert, AlertDescription, AlertTitle } from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { copilotApi, copilotFeatures, describeCopilotFailure, offers, type CopilotApi } from '$lib/copilot/api';
	import { createDraftFlow, isDrafting, type DraftPhase } from '$lib/copilot/draft-flow';
	import { COPILOT_LIMITS, type CopilotFeature, type TextDraft } from '$lib/copilot/types';

	/**
	 * A text field with the copilot's "Draft or improve" button. Use it for every text field. The
	 * button is not there at all when the copilot is unavailable, and the field works as a plain one.
	 * The copilot only proposes: its text is shown beside the field and replaces yours only when you
	 * press "Use this draft" (and "Undo" brings yours back).
	 */
	let {
		id,
		label,
		value = $bindable(''),
		multiline = true,
		rows = 4,
		maxlength,
		placeholder,
		disabled = false,
		error,
		api = copilotApi
	}: {
		id: string;
		label: string;
		value?: string;
		multiline?: boolean;
		rows?: number;
		maxlength?: number;
		placeholder?: string;
		disabled?: boolean;
		error?: string;
		api?: CopilotApi;
	} = $props();

	let features = $state<CopilotFeature[] | undefined>();
	const available = $derived(offers(features, 'text.improve'));

	let open = $state(false);
	let instruction = $state('');
	let phase = $state<DraftPhase<TextDraft>>({ kind: 'idle' });
	let previous = $state<string | undefined>();
	// `api` is chosen once, when the field is made (a test passes its own); it is not meant to change.
	// svelte-ignore state_referenced_locally
	const flow = createDraftFlow(api, 'text.improve', (next) => (phase = next));

	const mode = $derived(value.trim() ? 'improve' : 'draft');
	const drafting = $derived(isDrafting(phase));
	const failure = $derived(phase.kind === 'failed' ? describeCopilotFailure(phase.error) : undefined);

	onMount(() => {
		copilotFeatures(api).then((list) => (features = list));
	});
	onDestroy(() => flow.cancel());

	function toggle() {
		if (open) return close();
		open = true;
		previous = undefined;
	}

	function close() {
		flow.cancel();
		open = false;
		instruction = '';
	}

	function ask() {
		flow.run({ mode, field: label.slice(0, COPILOT_LIMITS.fieldMaxChars), text: value, instruction: instruction.trim() || undefined });
	}

	function use() {
		if (phase.kind !== 'ready') return;
		previous = value;
		value = phase.response.draft.text;
		close();
	}

	function undo() {
		if (previous === undefined) return;
		value = previous;
		previous = undefined;
	}

	const fieldClass =
		'block w-full rounded-md bg-white/5 px-3 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 placeholder:text-gray-500 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-500 disabled:opacity-50 aria-invalid:outline-red-400';
</script>

<div class="space-y-2" data-testid="draftable-{id}">
	<div class="flex items-center justify-between gap-x-3">
		<Label for={id}>{label}</Label>
		{#if available}
			<Button
				type="button"
				variant="ghost"
				size="xs"
				disabled={disabled}
				aria-expanded={open}
				aria-controls="{id}-copilot"
				onclick={toggle}
			>
				<Sparkles aria-hidden="true" /> Draft or improve<span class="sr-only"> {label}</span>
			</Button>
		{/if}
	</div>

	{#if multiline}
		<textarea
			{id}
			{rows}
			{maxlength}
			{placeholder}
			{disabled}
			bind:value
			class={fieldClass}
			aria-invalid={error ? true : undefined}
			aria-describedby={error ? `${id}-error` : undefined}
		></textarea>
	{:else}
		<Input
			{id}
			{maxlength}
			{placeholder}
			{disabled}
			autocomplete="off"
			bind:value
			aria-invalid={error ? true : undefined}
			aria-describedby={error ? `${id}-error` : undefined}
		/>
	{/if}
	{#if error}
		<p id="{id}-error" class="text-sm/6 text-red-400" role="alert">{error}</p>
	{/if}

	{#if previous !== undefined && !open}
		<p class="flex items-center gap-x-2 text-xs/5 text-gray-400">
			Draft used.
			<button type="button" class="inline-flex items-center gap-x-1 font-semibold text-indigo-300 underline-offset-2 hover:underline focus-visible:outline-2 focus-visible:outline-indigo-400" onclick={undo}>
				<Undo class="size-3" aria-hidden="true" /> Undo
			</button>
		</p>
	{/if}

	{#if available && open}
		<div id="{id}-copilot" role="group" aria-labelledby="{id}-copilot-title" data-testid="copilot-{id}" class="space-y-3 rounded-lg bg-white/5 p-4 outline-1 -outline-offset-1 outline-white/10">
			<div class="flex items-center justify-between gap-x-3">
				<p id="{id}-copilot-title" class="text-sm/6 font-semibold text-white">{mode === 'improve' ? 'Improve this text' : 'Draft this text'}</p>
				{#if api.sample}<Badge variant="secondary" data-testid="copilot-sample">Sample data</Badge>{/if}
			</div>

			{#if phase.kind !== 'ready'}
				<div class="space-y-2">
					<Label for="{id}-instruction">Instruction (optional)</Label>
					<Input
						id="{id}-instruction"
						autocomplete="off"
						maxlength={COPILOT_LIMITS.instructionMaxChars}
						placeholder={mode === 'improve' ? 'Make it shorter' : 'What should it say?'}
						disabled={drafting}
						bind:value={instruction}
					/>
				</div>
			{/if}

			<div aria-live="polite">
				{#if drafting}
					<p class="flex items-center gap-x-2 text-sm/6 text-gray-300"><LoaderCircle class="size-4 animate-spin text-indigo-400" aria-hidden="true" /> Drafting…</p>
				{/if}
				{#if phase.kind === 'ready'}
					<div class="space-y-2">
						<p class="text-xs/5 font-semibold text-gray-400">The copilot proposes</p>
						<p class="whitespace-pre-wrap rounded-md bg-gray-900/60 px-3 py-2 text-sm/6 text-white" data-testid="copilot-proposal">{phase.response.draft.text}</p>
						{#if phase.response.notes.length > 0}
							<ul class="list-disc space-y-1 pl-5 text-xs/5 text-gray-400" data-testid="copilot-notes">
								{#each phase.response.notes as note (note)}<li>{note}</li>{/each}
							</ul>
						{/if}
					</div>
				{/if}
			</div>

			{#if failure}
				<Alert variant="destructive">
					<CircleAlert aria-hidden="true" />
					<AlertTitle>{failure.title}</AlertTitle>
					<AlertDescription>{failure.hint}</AlertDescription>
				</Alert>
			{/if}

			<div class="flex items-center gap-x-2">
				{#if phase.kind === 'ready'}
					<Button type="button" size="sm" onclick={use}>Use this draft</Button>
					<Button type="button" size="sm" variant="outline" onclick={ask}>Try again</Button>
				{:else}
					<Button type="button" size="sm" disabled={drafting} onclick={ask}>
						{#if drafting}<LoaderCircle class="animate-spin" aria-hidden="true" />{/if}
						{mode === 'improve' ? 'Improve' : 'Draft'}
					</Button>
				{/if}
				<Button type="button" size="sm" variant="ghost" onclick={close}>{phase.kind === 'ready' ? 'Dismiss' : 'Cancel'}</Button>
			</div>
		</div>
	{/if}
</div>
