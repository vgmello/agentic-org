<script lang="ts">
	import { untrack } from 'svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '$lib/components/ui/sheet';
	import { toAppError } from '$lib/office/api';
	import type { OrgSnapshot, Priority, WorkItem, WorkItemSummary } from '$lib/proto';
	import { workApi, type WorkDraft } from '$lib/work';
	import { BUG_STATUS_LABEL, bugRef, closedWord, SEVERITIES, severityWord } from '$lib/work/bugs';
	import { assigneeOptions, nameOf } from '$lib/work/people';
	import { timeBlock } from '$lib/work/time';
	import { newId } from '$lib/org/commands';
	import { PRIORITIES, STATUS_LABEL, taskRef } from '$lib/work/tasks';

	/** The bug page (BUG-*, TRI-*, FIX-*): its report, triage while New, then fix, verify and close or reopen. */
	let {
		open = $bindable(false),
		organizationId,
		bugId,
		snapshot,
		areas,
		bugs,
		items,
		send,
		onchanged
	}: {
		open: boolean;
		organizationId: string;
		bugId: string | undefined;
		snapshot: OrgSnapshot;
		areas: { id: string; name: string; general?: boolean }[];
		bugs: WorkItemSummary[];
		/** Every item of the project (the fix tasks are looked up here). */
		items: WorkItemSummary[];
		send: (draft: WorkDraft) => Promise<unknown>;
		onchanged: (message?: string) => void;
	} = $props();

	let item = $state<WorkItem | undefined>();
	let error = $state<string | undefined>();
	let busy = $state(false);
	// The step the owner is in the middle of (needs words before it is sent).
	let step = $state<'fix' | 'still' | 'close' | 'reopen' | undefined>();
	let note = $state('');
	let anyway = $state('');
	let askAnyway = $state(false);
	let closeAs = $state<'duplicate' | 'wont_fix' | 'cant_reproduce'>('wont_fix');
	let original = $state('');
	// Triage choices (while New).
	let tArea = $state('');
	let tSeverity = $state('major');
	let tPriority = $state<Priority>('normal');
	let tAssignee = $state('');

	async function load() {
		if (!bugId) return;
		try {
			item = await workApi.get(organizationId, bugId);
		} catch (e) {
			error = toAppError(e).message;
		}
	}

	$effect(() => {
		if (open && bugId) {
			untrack(() => {
				item = undefined;
				error = undefined;
				step = undefined;
				note = anyway = original = '';
				askAnyway = false;
				void load().then(() => {
					const s = item?.summary;
					if (!s) return;
					tArea = s.area_id ?? areas.find((a) => a.general)?.id ?? '';
					tSeverity = s.severity ?? 'major';
					tPriority = s.priority ?? 'normal';
					tAssignee = '';
				});
			});
		}
	});

	async function act(draft: WorkDraft, message?: string): Promise<boolean> {
		if (busy) return false;
		busy = true;
		error = undefined;
		try {
			await send(draft);
			await load();
			onchanged(message);
			return true;
		} catch (e) {
			const err = toAppError(e);
			error = err.message;
			if (err.source === 'api' && err.code === 'checklist_incomplete') askAnyway = true;
			if (err.source === 'api' && err.code === 'conflict') await load();
			return false;
		} finally {
			busy = false;
		}
	}

	const s = $derived(item?.summary);
	const v = $derived(s?.version ?? 0);
	const people = $derived(assigneeOptions(snapshot));
	const others = $derived(bugs.filter((b) => b.kind === 'bug' && b.id !== s?.id && b.status !== 'closed'));
	const originalBug = $derived(s?.duplicate_of ? bugs.find((b) => b.id === s.duplicate_of) : undefined);
	// Its fix tasks: the tasks with a fixes link to this bug (design §1.7).
	const fixTasks = $derived((item?.backlinks ?? []).filter((l) => l.kind === 'fixes').map((l) => items.find((i) => i.id === l.from_item_id)).filter((t): t is WorkItemSummary => !!t && !t.deleted));
	async function createFixTask() {
		if (!s) return;
		await act({ method: 'task.create', item_id: newId(), project_id: s.project_id!, title: [...`Fix ${bugRef(s)}: ${s.title}`].slice(0, 120).join(''), fixes: [{ link_id: newId(), bug_id: s.id }] }, `Created a fix task for ${bugRef(s)}.`);
	}
	const areaName = (id: string | undefined) => areas.find((a) => a.id === id)?.name ?? 'General';
	// TRI-1: who looks at it first (no area leads yet: the project's owner of record, the owner).
	const triager = $derived(nameOf(snapshot, snapshot.members.find((m) => m.owner)?.id));
	const field = 'w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400';
	const close = () => ((step = undefined), (note = ''), (anyway = ''), (askAnyway = false));

	async function submitStep() {
		if (!s) return;
		let ok = false;
		if (step === 'fix') ok = await act({ method: 'bug.mark_fixed', item_id: s.id, expected_version: v, note: note.trim(), ...(askAnyway ? { anyway_reason: anyway.trim() } : {}) }, `${bugRef(s)} marked fixed.`);
		else if (step === 'still') ok = await act({ method: 'bug.verify', item_id: s.id, expected_version: v, outcome: 'still_happens', note: note.trim() }, `${bugRef(s)} sent back to Triaged.`);
		else if (step === 'reopen') ok = await act({ method: 'bug.reopen', item_id: s.id, expected_version: v, note: note.trim() }, `${bugRef(s)} reopened.`);
		else if (step === 'close')
			ok = await act(
				closeAs === 'duplicate'
					? { method: 'bug.close', item_id: s.id, expected_version: v, duplicate_of: original }
					: { method: 'bug.close', item_id: s.id, expected_version: v, resolution: closeAs, reason: note.trim() },
				`${bugRef(s)} closed.`
			);
		if (ok) close();
	}
</script>

<Sheet bind:open>
	<SheetContent side="right" class="w-full max-w-lg overflow-y-auto">
		<SheetHeader>
			<SheetTitle class="[overflow-wrap:anywhere]">{s ? `${bugRef(s)} · ${s.title}` : 'Bug'}</SheetTitle>
			<SheetDescription>{s ? `Reported ${new Date(s.created_at).toLocaleDateString()} by ${nameOf(snapshot, s.created_by)}` : 'Loading…'}</SheetDescription>
		</SheetHeader>

		{#if s && item}
			<div class="flex flex-col gap-y-4 px-4 pb-6" data-testid="bug-page">
				<div class="flex flex-wrap items-center gap-2">
					<Badge variant="secondary" data-testid="bug-status">{BUG_STATUS_LABEL[s.status] ?? s.status}{s.status === 'closed' ? ` · ${closedWord(s, originalBug)}` : ''}</Badge>
					<Badge variant="secondary" data-testid="bug-severity">{severityWord(s.severity)}{s.severity_confirmed ? '' : ' (not confirmed)'}</Badge>
					<span class="text-sm/6 text-gray-400">{areaName(s.area_id)} · {nameOf(snapshot, s.assignee_id)}</span>
					{#if s.reopen_count > 0}<span class="text-xs/5 text-gray-400">Reopened {s.reopen_count} {s.reopen_count === 1 ? 'time' : 'times'}</span>{/if}
				</div>

				<section class="space-y-2 text-sm/6">
					<h3 class="font-semibold text-white">What happened</h3>
					<p class="whitespace-pre-wrap text-gray-200 [overflow-wrap:anywhere]">{item.what_happened}</p>
					{#if item.expected}<h3 class="font-semibold text-white">Expected</h3><p class="whitespace-pre-wrap text-gray-200 [overflow-wrap:anywhere]">{item.expected}</p>{/if}
					{#if item.steps}<h3 class="font-semibold text-white">Steps to reproduce</h3><p class="whitespace-pre-wrap text-gray-200 [overflow-wrap:anywhere]">{item.steps}</p>{/if}
					{#if item.environment}<p class="text-xs/5 text-gray-400 [overflow-wrap:anywhere]">Environment: {item.environment}</p>{/if}
					{#if item.closed_reason && (s.status === 'fixed' || s.status === 'closed')}<p class="rounded-md bg-white/5 px-3 py-2 text-gray-200" data-testid="bug-reason">{s.status === 'fixed' ? 'Fix' : 'Reason'}: {item.closed_reason}</p>{/if}
				</section>

				{#if s.status === 'new'}
					<section class="space-y-3 rounded-lg bg-white/5 p-3 outline-1 -outline-offset-1 outline-white/10" aria-labelledby="triage" data-testid="triage-panel">
						<h3 id="triage" class="text-sm/6 font-semibold text-white">Triage</h3>
						<p class="text-xs/5 text-gray-400">Suggested triager: {triager}. Confirm the area and severity, set the priority, and give it to someone.</p>
						<div class="grid gap-3 sm:grid-cols-2">
							<div class="space-y-1"><Label for="t-area">Area</Label><select id="t-area" bind:value={tArea} class={field}>{#each areas as a (a.id)}<option value={a.id}>{a.name}</option>{/each}</select></div>
							<div class="space-y-1"><Label for="t-severity">Severity</Label><select id="t-severity" bind:value={tSeverity} class={field}>{#each SEVERITIES as x (x.id)}<option value={x.id}>{x.label}</option>{/each}</select></div>
							<div class="space-y-1"><Label for="t-priority">Priority</Label><select id="t-priority" bind:value={tPriority} class={field}>{#each PRIORITIES as p (p.id)}<option value={p.id}>{p.label}</option>{/each}</select></div>
							<div class="space-y-1"><Label for="t-assignee">Assignee</Label><select id="t-assignee" bind:value={tAssignee} class={field}><option value="">Unassigned</option>{#each people as p (p.member.id)}<option value={p.member.id}>{p.label}</option>{/each}</select></div>
						</div>
						<Button disabled={busy} onclick={() => act({ method: 'bug.triage', item_id: s.id, expected_version: v, area_id: tArea, severity: tSeverity as 'major', priority: tPriority, assignee_id: tAssignee || undefined }, `${bugRef(s)} triaged.`)}>Triage</Button>
					</section>
				{/if}

				<section class="space-y-2" aria-labelledby="fix-tasks" data-testid="fix-tasks">
					<h3 id="fix-tasks" class="text-sm/6 font-semibold text-white">Fix tasks</h3>
					<ul class="divide-y divide-white/5 rounded-lg bg-white/5 outline-1 -outline-offset-1 outline-white/10">
						{#each fixTasks as t (t.id)}
							<li class="flex items-center gap-x-3 px-3 py-2 text-sm/6" data-testid="fix-task"><span class="text-gray-400">{taskRef(t)}</span><span class="min-w-0 flex-1 [overflow-wrap:anywhere]">{t.title}</span><span class="text-gray-300">{STATUS_LABEL[t.status as 'todo']}{t.status === 'done' && t.resolution === 'cancelled' ? ' · Cancelled' : ''}</span></li>
						{:else}
							<li class="px-3 py-2 text-sm/6 text-gray-400">No fix task yet.</li>
						{/each}
					</ul>
					{#if s.status !== 'closed'}<Button size="sm" variant="outline" disabled={busy} onclick={createFixTask}>Create a fix task</Button>{/if}
					{#if fixTasks.length > 0 && fixTasks.every((t) => t.status === 'done' && t.resolution === 'cancelled') && s.status !== 'closed'}<p class="text-xs/5 text-amber-200">All its fix tasks were cancelled. Create a fix task, or close the bug.</p>{/if}
				</section>

				{#if s.status !== 'closed' && s.status !== 'new'}
					<div class="space-y-1">
						<Label for="bug-assignee">Assignee</Label>
						<select id="bug-assignee" disabled={busy} value={s.assignee_id ?? ''} onchange={(e) => act({ method: 'bug.assign', item_id: s.id, expected_version: v, assignee_id: e.currentTarget.value || undefined })} class={field}>
							<option value="">Unassigned</option>{#each people as p (p.member.id)}<option value={p.member.id}>{p.label}</option>{/each}
						</select>
					</div>
				{/if}

				{#if !step}
					<div class="flex flex-wrap gap-2" data-testid="bug-actions">
						{#if s.status === 'triaged' || s.status === 'in_progress'}<Button size="sm" onclick={() => (step = 'fix')}>Mark fixed…</Button>{/if}
						{#if s.status === 'fixed'}
							<Button size="sm" disabled={busy} onclick={() => act({ method: 'bug.verify', item_id: s.id, expected_version: v, outcome: 'fixed' }, `${bugRef(s)} verified and closed.`)}>It's fixed: close</Button>
							<Button size="sm" variant="outline" onclick={() => (step = 'still')}>Still happens…</Button>
						{/if}
						{#if s.status !== 'closed'}<Button size="sm" variant="outline" onclick={() => (step = 'close')}>Close…</Button>{/if}
						{#if s.status === 'closed'}<Button size="sm" variant="outline" onclick={() => (step = 'reopen')}>Reopen…</Button>{/if}
					</div>
				{:else}
					<section class="space-y-2 rounded-lg bg-white/5 p-3 outline-1 -outline-offset-1 outline-white/10" data-testid="bug-step">
						{#if step === 'close'}
							<Label for="close-as">Close it as</Label>
							<select id="close-as" bind:value={closeAs} class={field}>
								<option value="wont_fix">Won't fix</option>
								<option value="cant_reproduce">Can't reproduce</option>
								<option value="duplicate">A duplicate of another bug</option>
							</select>
							{#if closeAs === 'duplicate'}
								<Label for="close-original">The original</Label>
								<select id="close-original" bind:value={original} class={field}><option value="">Choose a bug…</option>{#each others as b (b.id)}<option value={b.id}>{bugRef(b)} · {b.title}</option>{/each}</select>
							{:else}
								<Label for="bug-note">Why</Label>
								<Input id="bug-note" bind:value={note} autocomplete="off" />
							{/if}
						{:else}
							<Label for="bug-note">{step === 'fix' ? 'What fixed it' : step === 'still' ? 'What do you still see' : 'Why is it reopened'}</Label>
							<Input id="bug-note" bind:value={note} autocomplete="off" />
						{/if}
						{#if askAnyway}
							<Label for="bug-anyway">Why is it fixed with required steps left?</Label>
							<Input id="bug-anyway" bind:value={anyway} autocomplete="off" />
						{/if}
						<div class="flex gap-x-2">
							<Button size="sm" disabled={busy || (step === 'close' && closeAs === 'duplicate' ? original === '' : note.trim() === '') || (askAnyway && anyway.trim() === '')} onclick={submitStep}>
								{step === 'fix' ? (askAnyway ? 'Mark fixed anyway' : 'Mark fixed') : step === 'still' ? 'Send back' : step === 'reopen' ? 'Reopen' : 'Close bug'}
							</Button>
							<Button size="sm" variant="outline" onclick={close}>Cancel</Button>
						</div>
					</section>
				{/if}

				<p class="text-sm/6 text-gray-400" data-testid="bug-time">{timeBlock(item.time)}</p>
				{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
			</div>
		{:else if error}
			<p class="px-4 text-sm/6 text-red-400" role="alert">{error}</p>
		{/if}
	</SheetContent>
</Sheet>
