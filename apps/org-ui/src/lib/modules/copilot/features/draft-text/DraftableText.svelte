<script lang="ts">
	import { onDestroy, onMount, tick, untrack } from 'svelte';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Undo from '@lucide/svelte/icons/undo';
	import { Alert, AlertDescription, AlertTitle } from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { copilotApi, copilotFeatures, describeCopilotFailure, offers, type CopilotApi } from '$lib/modules/copilot/api';
	import { checkTextRequest, fitTo, undoTarget } from '$lib/modules/copilot/check';
	import { createDraftFlow, isDrafting, type DraftPhase } from '$lib/modules/copilot/draft-flow';
	import { COPILOT_LIMITS, type CopilotFeature, type TextDraft } from '$lib/modules/copilot/types';

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
		limit,
		hint,
		tag = false,
		showDraftedTag = false,
		onedit,
		onrun,
		resetKey,
		placeholder,
		disabled = false,
		error,
		onchange,
		onblur,
		api = copilotApi
	}: {
		id: string;
		label: string;
		value?: string;
		multiline?: boolean;
		rows?: number;
		maxlength?: number;
		/** The office's own cap, when the field's `maxlength` is looser (the field allows some slack). A draft is cut to the smaller. */
		limit?: number;
		/** Small grey text after the label, e.g. "(optional)". */
		hint?: string;
		/** Show the "Drafted" tag: the parent says the field's text came from a draft (it clears the tag when the person edits). */
		tag?: boolean;
		/** Tag the field "Drafted" itself after "Use this draft", until the person types in it. */
		showDraftedTag?: boolean;
		/** The person typed in the field (not a draft being used). */
		onedit?: () => void;
		/** The copilot panel of this field is about to run (so the screen can clear other errors it shows). */
		onrun?: () => void;
		/** When this changes, the panel closes (so only one panel on a screen shows an error at a time). */
		resetKey?: unknown;
		placeholder?: string;
		disabled?: boolean;
		error?: string;
		/** The text was replaced by a draft, or put back: the new value is already in `value`. */
		onchange?: () => void;
		onblur?: () => void;
		api?: CopilotApi;
	} = $props();

	let features = $state<CopilotFeature[] | undefined>();
	const available = $derived(offers(features, 'text.improve'));

	let open = $state(false);
	let instruction = $state('');
	let requestError = $state<string | undefined>();
	let phase = $state<DraftPhase<TextDraft>>({ kind: 'idle' });
	let previous = $state<string | undefined>();
	let clipped = $state<number | undefined>();
	// The draft last put in the field: while the field still holds it, Undo goes back past it.
	let lastApplied = $state<string | undefined>();
	// "Drafted": the field holds a draft the person hasn't typed in since (an edit clears it for good,
	// and so does Undo). Only where the screen asks for the tag.
	const drafted = $derived(tag || (showDraftedTag && lastApplied !== undefined));
	function typed() {
		lastApplied = undefined;
		onedit?.();
	}
	// `api` is chosen once, when the field is made (a test passes its own); it is not meant to change.
	// svelte-ignore state_referenced_locally
	const flow = createDraftFlow(api, 'text.improve', (next) => (phase = next));

	const mode = $derived<'draft' | 'improve'>(value.trim() ? 'improve' : 'draft');
	const drafting = $derived(isDrafting(phase));
	const failure = $derived(phase.kind === 'failed' ? describeCopilotFailure(phase.error) : undefined);

	let seenKey = untrack(() => resetKey);
	$effect(() => {
		const key = resetKey;
		untrack(() => {
			if (key === seenKey) return;
			seenKey = key;
			if (open) close();
		});
	});

	onMount(() => {
		copilotFeatures(api).then((list) => (features = list));
	});
	onDestroy(() => flow.cancel());

	function toggle() {
		if (open) return close();
		open = true;
	}

	function close() {
		flow.cancel();
		open = false;
		instruction = '';
		requestError = undefined;
	}

	function ask() {
		const request = { mode, field: label.slice(0, COPILOT_LIMITS.fieldMaxChars), text: value, instruction: instruction.trim() || undefined };
		requestError = checkTextRequest(request);
		if (requestError) return;
		onrun?.();
		flow.run(request);
	}

	// The smaller of the field's own `maxlength` and the office's cap.
	const cap = $derived(Math.min(maxlength ?? Infinity, limit ?? Infinity));

	async function use() {
		if (phase.kind !== 'ready') return;
		previous = undoTarget(previous, value, lastApplied);
		// A draft longer than the field allows is cut to fit, and the person is told.
		const fitted = fitTo(phase.response.draft.text, Number.isFinite(cap) ? cap : undefined);
		clipped = fitted.clipped ? cap : undefined;
		value = fitted.text;
		lastApplied = fitted.text;
		close();
		await tick();
		onchange?.();
	}

	async function undo() {
		if (previous === undefined) return;
		value = previous;
		previous = undefined;
		lastApplied = undefined;
		clipped = undefined;
		await tick();
		onchange?.();
	}

	const fieldClass =
		'block w-full rounded-md bg-gray-800 px-3 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 placeholder:text-gray-500 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-500 disabled:opacity-50 aria-invalid:outline-red-400';
</script>

<div class="space-y-2" data-testid="draftable-{id}">
	<div class="flex items-center justify-between gap-x-3">
		<div class="flex items-center gap-x-2">
			<Label for={id}>{label}{#if hint}<span class="font-normal text-gray-400"> {hint}</span>{/if}</Label>
			{#if drafted}<Badge variant="secondary" data-testid="drafted-tag-{id}">Drafted</Badge>{/if}
		</div>
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
			{onblur}
			oninput={typed}
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
			{onblur}
			oninput={typed}
			aria-invalid={error ? true : undefined}
			aria-describedby={error ? `${id}-error` : undefined}
		/>
	{/if}
	{#if error}
		<p id="{id}-error" class="text-sm/6 text-red-400" role="alert">{error}</p>
	{/if}

	{#if previous !== undefined && !open}
		<p class="flex items-center gap-x-2 text-xs/5 text-gray-400" data-testid="draft-used">
			Draft used.{#if clipped !== undefined} Shortened to fit the limit of {clipped} characters.{/if}
			<button type="button" class="inline-flex items-center gap-x-1 font-semibold text-indigo-300 underline-offset-2 hover:underline focus-visible:outline-2 focus-visible:outline-indigo-400" onclick={undo} {disabled}>
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
						oninput={() => (requestError = undefined)}
					onkeydown={(e) => {
						// Inside a form (a dialog), Enter here would submit the whole form.
						if (e.key === 'Enter' && !e.isComposing) {
							e.preventDefault();
							if (!drafting) ask();
						}
					}}
						aria-invalid={requestError ? true : undefined}
						aria-describedby={requestError ? `${id}-request-error` : undefined}
					/>
					{#if requestError}
						<p id="{id}-request-error" class="text-sm/6 text-red-400" role="alert">{requestError}</p>
					{/if}
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
								{#each phase.response.notes as note, i (i)}<li>{note}</li>{/each}
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
					<Button type="button" size="sm" {disabled} onclick={use}>Use this draft</Button>
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
