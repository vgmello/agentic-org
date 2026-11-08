<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import Check from '@lucide/svelte/icons/check';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Undo from '@lucide/svelte/icons/undo';
	import DraftableText from '$lib/components/DraftableText.svelte';
	import { Alert, AlertDescription, AlertTitle } from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import { copilotApi, copilotFeatures, describeCopilotFailure, offers } from '$lib/copilot/api';
	import { createDraftFlow, isDrafting, type DraftPhase } from '$lib/copilot/draft-flow';
	import { COPILOT_LIMITS, SEVERITIES, type BugDraft, type CopilotFeature } from '$lib/copilot/types';
	import {
		BUG_LIMITS,
		bugApi,
		emptyForm,
		formFromDraft,
		isBlank,
		submitReport,
		type BugForm,
		type FormErrors
	} from '$lib/bugs/report';
	import { describeFailure } from '$lib/create-flow';

	let form = $state<BugForm>(emptyForm());
	let errors = $state<FormErrors>({});
	let sentence = $state('');
	let features = $state<CopilotFeature[] | undefined>();
	let phase = $state<DraftPhase<BugDraft>>({ kind: 'idle' });
	// The form as it was before the copilot filled it, and the notes that came with the draft.
	let before = $state<BugForm | undefined>();
	let drafted = $state<{ notes: string[] } | undefined>();
	let sending = $state(false);
	let sendError = $state<string | undefined>();
	let sent = $state<{ id: string; title: string } | undefined>();

	const flow = createDraftFlow(copilotApi, 'bug.report', (next) => {
		phase = next;
		if (next.kind === 'ready') {
			before = form;
			form = formFromDraft(next.response.draft);
			errors = {};
			drafted = { notes: next.response.notes };
		}
	});
	const canDraft = $derived(offers(features, 'bug.report'));
	const drafting = $derived(isDrafting(phase));
	const failure = $derived(phase.kind === 'failed' ? describeCopilotFailure(phase.error) : undefined);

	onMount(() => {
		copilotFeatures(copilotApi).then((list) => (features = list));
	});
	onDestroy(() => flow.cancel());

	function draft() {
		flow.run({ sentence });
	}

	function undoDraft() {
		if (before) form = before;
		before = undefined;
		drafted = undefined;
		phase = { kind: 'idle' };
	}

	async function report(e: SubmitEvent) {
		e.preventDefault();
		if (sending) return;
		sending = true;
		sendError = undefined;
		try {
			const result = await submitReport(bugApi, form);
			if (result.kind === 'invalid') errors = result.errors;
			else if (result.kind === 'failed') sendError = describeFailure(result.error).hint;
			else {
				errors = {};
				sent = { id: result.id, title: result.report.title };
			}
		} finally {
			sending = false;
		}
	}

	function another() {
		form = emptyForm();
		sentence = '';
		errors = {};
		before = undefined;
		drafted = undefined;
		sent = undefined;
		phase = { kind: 'idle' };
	}

	const selectClass =
		'block w-full rounded-md bg-gray-800 px-3 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-500';
</script>

<svelte:head><title>Report a bug · Agentic Org</title></svelte:head>

<div class="max-w-2xl space-y-6 px-8 py-6">
	<p class="flex items-center gap-x-2 text-xs/5 text-gray-400" data-testid="sample-data-note">
		<Badge variant="secondary">Sample data</Badge> Nothing is sent: the copilot and the bug list are a mock.
	</p>

	{#if sent}
		<div class="space-y-4" role="status" data-testid="bug-sent">
			<span class="flex size-10 items-center justify-center rounded-full bg-green-500/10 text-green-400"><Check class="size-5" aria-hidden="true" /></span>
			<div>
				<h2 class="text-base/7 font-semibold text-white">Bug reported</h2>
				<p class="mt-1 text-sm/6 text-gray-400"><span class="font-semibold text-white" data-testid="bug-id">{sent.id}</span> · {sent.title}</p>
			</div>
			<Button type="button" variant="outline" onclick={another}>Report another bug</Button>
		</div>
	{:else}
		{#if canDraft}
			<section class="space-y-3 rounded-lg bg-white/5 p-4 outline-1 -outline-offset-1 outline-white/10" aria-labelledby="sentence-heading">
				<h2 id="sentence-heading" class="text-sm/6 font-semibold text-white">Start from a sentence</h2>
				<p class="text-sm/6 text-gray-400">Say what went wrong in your own words. The copilot drafts the report below. You check every field, then report it.</p>
				<div class="space-y-2">
					<Label for="sentence">What went wrong?</Label>
					<textarea
						id="sentence"
						rows="2"
						maxlength={COPILOT_LIMITS.sentenceMaxChars}
						placeholder="The save button does nothing after I rename a file"
						disabled={drafting}
						bind:value={sentence}
						class="block w-full rounded-md bg-white/5 px-3 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 placeholder:text-gray-500 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-500 disabled:opacity-50"
					></textarea>
				</div>
				<div aria-live="polite">
					{#if drafting}
						<p class="flex items-center gap-x-2 text-sm/6 text-gray-300"><LoaderCircle class="size-4 animate-spin text-indigo-400" aria-hidden="true" /> Drafting the report…</p>
					{/if}
				</div>
				{#if failure}
					<Alert variant="destructive">
						<CircleAlert aria-hidden="true" />
						<AlertTitle>{failure.title}</AlertTitle>
						<AlertDescription>{failure.hint}</AlertDescription>
					</Alert>
				{/if}
				<div class="flex items-center gap-x-3">
					<Button type="button" size="sm" disabled={drafting || !sentence.trim()} onclick={draft}>
						<Sparkles aria-hidden="true" /> Draft the report
					</Button>
					{#if before}
						<Button type="button" size="sm" variant="ghost" onclick={undoDraft}><Undo aria-hidden="true" /> Undo draft</Button>
					{/if}
				</div>
			</section>
		{/if}

		{#if drafted}
			<div class="rounded-lg bg-indigo-500/10 px-4 py-3 text-sm/6 text-indigo-100 outline-1 -outline-offset-1 outline-indigo-400/30" data-testid="drafted-banner">
				<p class="font-semibold">Drafted by the copilot. Nothing is reported yet.</p>
				<p>Check every field and change what is wrong, then press Report bug.</p>
				{#if drafted.notes.length > 0}
					<ul class="mt-1 list-disc pl-5 text-xs/5 text-indigo-200">
						{#each drafted.notes as note (note)}<li>{note}</li>{/each}
					</ul>
				{/if}
			</div>
		{/if}

		<form class="space-y-5" onsubmit={report} novalidate aria-label="Bug report">
			<DraftableText id="bug-title" label="Title" multiline={false} maxlength={BUG_LIMITS.title * 2} bind:value={form.title} error={errors.title} disabled={sending} />
			<DraftableText id="bug-actual" label="What happens" rows={3} bind:value={form.actual} error={errors.actual} disabled={sending} />
			<DraftableText id="bug-expected" label="What you expected" rows={2} bind:value={form.expected} error={errors.expected} disabled={sending} />
			<DraftableText id="bug-steps" label="Steps to reproduce (one per line)" rows={4} bind:value={form.steps} error={errors.steps} disabled={sending} />
			<div class="space-y-2">
				<Label for="bug-severity">Severity</Label>
				<select id="bug-severity" bind:value={form.severity} disabled={sending} class={selectClass}>
					{#each SEVERITIES as s (s)}<option value={s}>{s[0].toUpperCase() + s.slice(1)}</option>{/each}
				</select>
			</div>

			{#if sendError}
				<Alert variant="destructive">
					<CircleAlert aria-hidden="true" />
					<AlertTitle>Couldn't report the bug</AlertTitle>
					<AlertDescription>{sendError}</AlertDescription>
				</Alert>
			{/if}

			<div class="flex items-center gap-x-3">
				<Button type="submit" disabled={sending || isBlank(form)}>
					{#if sending}<LoaderCircle class="animate-spin" aria-hidden="true" />{/if}
					Report bug
				</Button>
			</div>
		</form>
	{/if}
</div>
