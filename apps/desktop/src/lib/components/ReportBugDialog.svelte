<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/office/api';
	import { newId } from '$lib/org/commands';
	import type { WorkAccepted, WorkItemSummary } from '$lib/proto';
	import { WORK_LIMITS } from '$lib/proto';
	import type { WorkDraft } from '$lib/work';
	import { bugRef, SEVERITIES, similarBugs } from '$lib/work/bugs';
	import { environmentLine } from '$lib/work/format';

	/** Report a bug (BUG-1..BUG-4): a title, what happened and a severity (no default), with similar open bugs suggested. */
	let {
		open = $bindable(false),
		projectId,
		projectName,
		areas,
		bugs,
		triager,
		send,
		ondone,
		onopen
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

	$effect(() => {
		if (open) {
			untrack(() => {
				title = happened = severity = expected = steps = '';
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
			<div class="space-y-2">
				<Label for="bug-title">Title</Label>
				<Input id="bug-title" bind:value={title} maxlength={WORK_LIMITS.title * 2} disabled={busy} autocomplete="off" />
				{#if touched && title.trim() === ''}<p class="text-sm/6 text-red-400" role="alert">Give the bug a title.</p>{/if}
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
			<div class="space-y-2">
				<Label for="bug-happened">What happened</Label>
				<textarea id="bug-happened" bind:value={happened} rows="3" disabled={busy} class={field}></textarea>
				{#if touched && happened.trim() === ''}<p class="text-sm/6 text-red-400" role="alert">Say what happened.</p>{/if}
			</div>
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
			<div class="space-y-2">
				<Label for="bug-expected">Expected <span class="font-normal text-gray-400">(optional)</span></Label>
				<textarea id="bug-expected" bind:value={expected} rows="2" disabled={busy} class={field}></textarea>
			</div>
			<div class="space-y-2">
				<Label for="bug-steps">Steps to reproduce <span class="font-normal text-gray-400">(optional)</span></Label>
				<textarea id="bug-steps" bind:value={steps} rows="2" disabled={busy} class={field}></textarea>
			</div>
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
