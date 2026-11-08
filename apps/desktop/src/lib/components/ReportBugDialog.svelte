<script lang="ts">
	import { onDestroy, onMount, untrack } from 'svelte';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Undo from '@lucide/svelte/icons/undo';
	import DraftableText from '$lib/components/DraftableText.svelte';
	import { Alert, AlertDescription, AlertTitle } from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { copilotApi, copilotFeatures, describeCopilotFailure, offers, type CopilotApi } from '$lib/copilot/api';
	import { applyDraft, undoDraft, type BugFields } from '$lib/copilot/bug-prefill';
	import { createDraftFlow, isDrafting, type DraftPhase } from '$lib/copilot/draft-flow';
	import { COPILOT_LIMITS, type BugDraft, type CopilotFeature } from '$lib/copilot/types';
	import { toAppError } from '$lib/office/api';
	import { newId } from '$lib/org/commands';
	import type { WorkAccepted, WorkItemSummary } from '$lib/proto';
	import { WORK_LIMITS } from '$lib/proto';
	import type { WorkDraft } from '$lib/work';
	import { bugRef, SEVERITIES, similarBugs } from '$lib/work/bugs';
	import { environmentLine } from '$lib/work/format';

	/**
	 * Report a bug (BUG-1..BUG-4): a title, what happened and a severity (no default), with similar open bugs suggested.
	 * When the copilot is available, one sentence can draft the fields (the `bug.from_sentence` feature): the draft only
	 * fills this form, and the person edits it and presses Report bug. Nothing is reported by the copilot.
	 */
	let {
		open = $bindable(false),
		projectId,
		projectName,
		areas,
		bugs,
		triager,
		send,
		ondone,
		onopen,
		copilot = copilotApi
	}: {
		open: boolean;
		projectId: string;
		projectName: string;
		areas: { id: string; name: string; general?: boolean }[];
		bugs: WorkItemSummary[];
		/** Who looks at a new bug first (TRI-1). */
		triager: string;
		send: (draft: WorkDraft) => Promise<WorkAccepted>;
		ondone: (created: { ref: string }) => void;
		/** Open an existing bug (a suggestion was the one meant). */
		onopen: (id: string) => void;
		copilot?: CopilotApi;
	} = $props();

	let title = $state('');
	let happened = $state('');
	let severity = $state('');
	let area = $state('');
	let expected = $state('');
	let steps = $state('');
	let environment = $state('');
	let error = $state<string | undefined>();
	let busy = $state(false);
	let touched = $state(false);

	// The copilot's draft from one sentence.
	let sentence = $state('');
	let features = $state<CopilotFeature[] | undefined>();
	let phase = $state<DraftPhase<BugDraft>>({ kind: 'idle' });
	// What the person had before the first draft: Undo brings this back, never an earlier draft.
	let before = $state<BugFields | undefined>();
	let drafted = $state<{ notes: string[] } | undefined>();
	const canSentence = $derived(offers(features, 'bug.from_sentence'));
	const drafting = $derived(isDrafting(phase));
	const failure = $derived(phase.kind === 'failed' ? describeCopilotFailure(phase.error) : undefined);
	const limits = { title: WORK_LIMITS.title, text: WORK_LIMITS.text, environment: 2000 };
	const fields = (): BugFields => ({ title, happened, severity, area, expected, steps, environment });
	// svelte-ignore state_referenced_locally
	const flow = createDraftFlow(copilot, 'bug.from_sentence', (next) => {
		phase = next;
		if (next.kind === 'ready') {
			const filled = applyDraft(before, fields(), next.response.draft, areas, limits);
			({ title, happened, severity, area, expected, steps, environment } = filled.fields);
			before = filled.before;
			drafted = { notes: next.response.notes };
		}
	});
	onMount(() => {
		copilotFeatures(copilot).then((list) => (features = list));
	});
	onDestroy(() => flow.cancel());

	function draftFromSentence() {
		flow.run(
			{ sentence },
			areas.map((a) => ({ kind: 'area' as const, label: a.name, text: a.id }))
		);
	}

	/** Undo: stop a draft that is still on its way, and put the person's own text back. */
	function undoDraftFields() {
		flow.cancel();
		({ title, happened, severity, area, expected, steps, environment } = undoDraft(before, fields()));
		before = undefined;
		drafted = undefined;
	}

	$effect(() => {
		if (!open) flow.cancel();
	});

	$effect(() => {
		if (open) {
			untrack(() => {
				title = happened = severity = expected = steps = '';
				sentence = '';
				before = drafted = undefined;
				flow.cancel();
				area = areas.find((a) => a.general)?.id ?? areas[0]?.id ?? '';
				// Prefilled from where the app runs; the reporter can change it.
				environment = environmentLine();
				error = undefined;
				busy = false;
				touched = false;
			});
		}
	});

	const similar = $derived(similarBugs(bugs, title));
	const missing = $derived(title.trim() === '' || happened.trim() === '' || severity === '');

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		touched = true;
		if (busy || missing) return;
		// A draft still on its way must not land on the bug being reported.
		flow.cancel();
		busy = true;
		error = undefined;
		try {
			const accepted = await send({
				method: 'bug.report',
				item_id: newId(),
				project_id: projectId,
				title: title.trim(),
				what_happened: happened.trim(),
				severity: severity as 'critical' | 'major' | 'minor' | 'trivial',
				area_id: area || undefined,
				expected: expected.trim() || undefined,
				steps: steps.trim() || undefined,
				environment: environment.trim() || undefined
			});
			open = false;
			ondone({ ref: bugRef(accepted.items[0]) });
		} catch (e) {
			error = toAppError(e).message;
		} finally {
			busy = false;
		}
	}
	const field = 'w-full rounded-md bg-gray-800 px-3 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400';
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent class="max-h-[90vh] overflow-y-auto sm:max-w-xl">
		<DialogHeader>
			<DialogTitle>Report a bug</DialogTitle>
			<DialogDescription>In {projectName}. It starts in New, and a triager looks at it.</DialogDescription>
		</DialogHeader>
		<form class="space-y-4" onsubmit={submit} novalidate>
			{#if canSentence}
				<section class="space-y-2 rounded-lg bg-white/5 p-3 outline-1 -outline-offset-1 outline-white/10" aria-labelledby="bug-sentence-heading" data-testid="bug-from-sentence">
					<div class="flex items-center justify-between gap-x-3">
						<h3 id="bug-sentence-heading" class="text-sm/6 font-semibold text-white">Start from a sentence</h3>
						{#if copilot.sample}<Badge variant="secondary">Sample data</Badge>{/if}
					</div>
					<p class="text-xs/5 text-gray-400">Say it in one sentence. The copilot drafts the fields below, and you check them before reporting.</p>
					<Label for="bug-sentence">What went wrong?</Label>
					<textarea id="bug-sentence" bind:value={sentence} rows="2" maxlength={COPILOT_LIMITS.sentenceMaxChars} disabled={busy || drafting} class={field} placeholder="The save button does nothing after I rename a file"></textarea>
					<div aria-live="polite">
						{#if drafting}<p class="flex items-center gap-x-2 text-sm/6 text-gray-300"><LoaderCircle class="size-4 animate-spin text-indigo-400" aria-hidden="true" /> Drafting the report…</p>{/if}
					</div>
					{#if failure}
						<Alert variant="destructive">
							<CircleAlert aria-hidden="true" />
							<AlertTitle>{failure.title}</AlertTitle>
							<AlertDescription>{failure.hint}</AlertDescription>
						</Alert>
					{/if}
					<div class="flex items-center gap-x-2">
						<Button type="button" size="sm" disabled={busy || drafting || !sentence.trim()} onclick={draftFromSentence}><Sparkles aria-hidden="true" /> Draft the report</Button>
						{#if drafting || before}
							<Button type="button" size="sm" variant="ghost" disabled={busy} onclick={undoDraftFields}><Undo aria-hidden="true" /> {drafting && !before ? 'Cancel draft' : 'Undo draft'}</Button>
						{/if}
					</div>
				</section>
				{#if drafted}
					<div class="rounded-md bg-indigo-500/10 px-3 py-2 text-sm/6 text-indigo-100 outline-1 -outline-offset-1 outline-indigo-400/30" data-testid="drafted-banner">
						<p class="font-semibold">Drafted by the copilot. Nothing is reported yet.</p>
						<p>Check every field and change what is wrong, then press Report bug.</p>
						{#if drafted.notes.length > 0}
							<ul class="mt-1 list-disc pl-5 text-xs/5 text-indigo-200">{#each drafted.notes as note, i (i)}<li>{note}</li>{/each}</ul>
						{/if}
					</div>
				{/if}
			{/if}
			<div class="space-y-2">
				<DraftableText id="bug-title" label="Title" multiline={false} bind:value={title} maxlength={WORK_LIMITS.title * 2} limit={WORK_LIMITS.title} disabled={busy} error={touched && title.trim() === '' ? 'Give the bug a title.' : undefined} api={copilot} />
				{#if similar.length > 0}
					<div class="rounded-md bg-white/5 p-2 text-sm/6 outline-1 -outline-offset-1 outline-white/10" data-testid="similar-bugs">
						<p class="text-xs/5 text-gray-400">Similar open bugs. Is it one of these?</p>
						<ul>
							{#each similar as b (b.id)}
								<li><button type="button" class="text-left text-indigo-300 underline underline-offset-2" onclick={() => { open = false; onopen(b.id); }}>{bugRef(b)} · {b.title}</button></li>
							{/each}
						</ul>
					</div>
				{/if}
			</div>
			<DraftableText id="bug-happened" label="What happened" rows={3} bind:value={happened} limit={WORK_LIMITS.text} disabled={busy} error={touched && happened.trim() === '' ? 'Say what happened.' : undefined} api={copilot} />
			<fieldset class="space-y-2" disabled={busy}>
				<legend class="text-sm/6 font-medium text-white">Severity</legend>
				<div class="grid gap-2 sm:grid-cols-2">
					{#each SEVERITIES as s (s.id)}
						<label class="flex cursor-pointer items-start gap-x-2 rounded-md bg-white/5 p-2 text-sm/6 outline-1 -outline-offset-1 outline-white/10 {severity === s.id ? 'bg-indigo-500/20' : ''}">
							<input type="radio" name="severity" value={s.id} bind:group={severity} class="mt-1" />
							<span><span class="font-semibold text-white">{s.label}</span><span class="block text-xs/5 text-gray-400">{s.help}</span></span>
						</label>
					{/each}
				</div>
				{#if touched && severity === ''}<p class="text-sm/6 text-red-400" role="alert">Choose a severity.</p>{/if}
			</fieldset>
			<div class="space-y-2">
				<Label for="bug-area">Area</Label>
				<select id="bug-area" bind:value={area} disabled={busy} class={field}>{#each areas as a (a.id)}<option value={a.id}>{a.name}</option>{/each}</select>
				<p class="text-xs/5 text-gray-400" data-testid="goes-to">Goes to: {triager}, who triages new bugs.</p>
			</div>
			<DraftableText id="bug-expected" label="Expected" hint="(optional)" rows={2} bind:value={expected} limit={WORK_LIMITS.text} disabled={busy} api={copilot} />
			<DraftableText id="bug-steps" label="Steps to reproduce" hint="(optional)" rows={2} bind:value={steps} limit={WORK_LIMITS.text} disabled={busy} api={copilot} />
			<div class="space-y-2">
				<Label for="bug-environment">Environment <span class="font-normal text-gray-400">(optional)</span></Label>
				<Input id="bug-environment" bind:value={environment} disabled={busy} autocomplete="off" />
			</div>
			{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
			<DialogFooter>
				<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={busy}>Report bug</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
