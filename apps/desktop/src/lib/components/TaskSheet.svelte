<script lang="ts">
	import { untrack } from 'svelte';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '$lib/components/ui/sheet';
	import { toAppError } from '$lib/office/api';
	import { newId } from '$lib/org/commands';
	import type { OrgSnapshot, Priority, TaskStatus, WorkItem } from '$lib/proto';
	import { WORK_LIMITS } from '$lib/proto';
	import { workApi, type WorkDraft } from '$lib/work';
	import { assigneeOptions, nameOf } from '$lib/work/people';
	import { elapsed } from '$lib/work/time';
	import { movesFrom, PRIORITIES, STATUS_LABEL, taskRef } from '$lib/work/tasks';

	/** The task page (work-items-ux §9): its fields, status, checklist and time. Each change is one call; a stale one reloads. */
	let {
		open = $bindable(false),
		organizationId,
		taskId,
		snapshot,
		send,
		onchanged,
		start
	}: {
		open: boolean;
		organizationId: string;
		taskId: string | undefined;
		snapshot: OrgSnapshot;
		send: (draft: WorkDraft) => Promise<unknown>;
		/** Something changed: the board reloads (and may announce `message`). */
		onchanged: (message?: string) => void;
		/** Opened because a move was refused: show what to do about it at once. */
		start?: { pending: 'done_anyway'; message: string };
	} = $props();

	let item = $state<WorkItem | undefined>();
	let error = $state<string | undefined>();
	let busy = $state(false);
	let title = $state('');
	let description = $state('');
	let newStep = $state('');
	let newRequired = $state(false);
	let blockedReason = $state('');
	let anywayReason = $state('');
	// The status the owner picked that needs more words before it is sent.
	let pending = $state<'blocked' | 'done_anyway' | undefined>();
	let confirmDelete = $state(false);
	let statusSelect = $state<HTMLSelectElement | undefined>();

	async function load(reset: boolean) {
		if (!taskId) return;
		try {
			const got = await workApi.get(organizationId, taskId);
			item = got;
			if (reset) {
				title = got.summary.title;
				description = got.description ?? '';
			}
		} catch (e) {
			error = toAppError(e).message;
		}
	}

	$effect(() => {
		if (open && taskId) {
			untrack(() => {
				item = undefined;
				error = undefined;
				newStep = blockedReason = anywayReason = '';
				newRequired = false;
				pending = start?.pending;
				if (start) error = start.message;
				confirmDelete = false;
				void load(true);
			});
		}
	});

	/** Send one change; on success reload the task and the board, on a stale one say so and reload. */
	async function change(draft: WorkDraft, announce?: string) {
		if (busy) return false;
		busy = true;
		error = undefined;
		try {
			await send(draft);
			// What was typed in the other fields stays: only a stale change (below) takes the latest.
			await load(false);
			onchanged(announce);
			return true;
		} catch (e) {
			const err = toAppError(e);
			error = err.message;
			if (err.source === 'api' && err.code === 'conflict') await load(true);
			if (err.source === 'api' && err.code === 'checklist_incomplete') pending = 'done_anyway';
			return false;
		} finally {
			busy = false;
		}
	}

	const s = $derived(item?.summary);
	const version = $derived(s?.version ?? 0);
	const people = $derived(assigneeOptions(snapshot));

	async function setStatus(status: TaskStatus) {
		if (!s) return;
		if (status === 'blocked') {
			pending = 'blocked';
			return;
		}
		pending = undefined;
		await change({ method: 'task.set_status', item_id: s.id, expected_version: version, status });
		resyncStatus();
	}
	/** The picker shows where the task is: after a refused or cancelled move it says so again. */
	function resyncStatus() {
		if (statusSelect && item) statusSelect.value = item.summary.status;
	}
	function cancelPending() {
		pending = undefined;
		resyncStatus();
	}
	async function confirmBlocked() {
		if (!s || blockedReason.trim() === '') return;
		if (await change({ method: 'task.set_status', item_id: s.id, expected_version: version, status: 'blocked', blocked_reason: blockedReason.trim() })) {
			pending = undefined;
			blockedReason = '';
		}
	}
	async function confirmAnyway() {
		if (!s || anywayReason.trim() === '') return;
		if (await change({ method: 'task.set_status', item_id: s.id, expected_version: version, status: 'done', anyway_reason: anywayReason.trim() })) {
			pending = undefined;
			anywayReason = '';
		}
	}
	async function addStep(e: SubmitEvent) {
		e.preventDefault();
		if (!s || newStep.trim() === '') return;
		if (await change({ method: 'checklist.add', item_id: s.id, steps: [{ step_id: newId(), text: newStep.trim(), required: newRequired }] }  )) ((newStep = ''), (newRequired = false));
	}
	async function remove() {
		if (!s) return;
		if (await change({ method: 'work.delete', item_id: s.id, expected_version: version })) open = false;
	}
	const field = 'w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400';
</script>

<Sheet bind:open>
	<SheetContent side="right" class="w-full max-w-lg overflow-y-auto">
		<SheetHeader>
			<SheetTitle class="[overflow-wrap:anywhere]">{s ? `${taskRef(s)} · ${s.title}` : 'Task'}</SheetTitle>
			<SheetDescription>{s ? `Created ${new Date(s.created_at).toLocaleDateString()} by ${nameOf(snapshot, s.created_by)}` : 'Loading…'}</SheetDescription>
		</SheetHeader>

		{#if s && item}
			<div class="flex flex-col gap-y-5 px-4 pb-6" data-testid="task-page">
				<div class="space-y-2">
					<Label for="task-title">Title</Label>
					<div class="flex gap-x-2">
						<Input id="task-title" bind:value={title} maxlength={WORK_LIMITS.title * 2} disabled={busy} autocomplete="off" />
						<Button variant="outline" disabled={busy || title.trim() === s.title || title.trim() === ''} onclick={() => change({ method: 'task.update', item_id: s.id, expected_version: version, title: title.trim() })}>Save</Button>
					</div>
				</div>

				<div class="grid gap-4 sm:grid-cols-2">
					<div class="space-y-2">
						<Label for="task-status">Status</Label>
						<select id="task-status" bind:this={statusSelect} disabled={busy} value={s.status} onchange={(e) => setStatus(e.currentTarget.value as TaskStatus)} class={field}>
							<option value={s.status}>{STATUS_LABEL[s.status as TaskStatus]}{s.status === 'done' && s.resolution === 'cancelled' ? ' · Cancelled' : ''}</option>
							{#each movesFrom(s.status) as m (m)}<option value={m}>{STATUS_LABEL[m]}</option>{/each}
						</select>
					</div>
					<div class="space-y-2">
						<Label for="task-priority">Priority</Label>
						<select id="task-priority" disabled={busy} value={s.priority ?? 'normal'} onchange={(e) => change({ method: 'task.update', item_id: s.id, expected_version: version, priority: e.currentTarget.value as Priority }, `${taskRef(s)} priority is now ${PRIORITIES.find((x) => x.id === e.currentTarget.value)?.label}.`)} class={field}>
							{#each PRIORITIES as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
						</select>
					</div>
				</div>

				{#if s.status === 'blocked' && s.blocked_reason}
					<p class="rounded-md bg-amber-500/10 px-3 py-2 text-sm/6 text-amber-100" data-testid="blocked-reason">Blocked: {s.blocked_reason}</p>
				{/if}
				{#if pending === 'blocked'}
					<div class="space-y-2 rounded-md bg-white/5 p-3 outline-1 -outline-offset-1 outline-white/10">
						<Label for="task-blocked-reason">What is it blocked on?</Label>
						<Input id="task-blocked-reason" bind:value={blockedReason} maxlength={WORK_LIMITS.blockedReason * 2} autocomplete="off" />
						<div class="flex gap-x-2">
							<Button size="sm" disabled={busy || blockedReason.trim() === ''} onclick={confirmBlocked}>Block task</Button>
							<Button size="sm" variant="outline" onclick={cancelPending}>Cancel</Button>
						</div>
					</div>
				{/if}
				{#if pending === 'done_anyway'}
					<div class="space-y-2 rounded-md bg-white/5 p-3 outline-1 -outline-offset-1 outline-white/10">
						<Label for="task-anyway">Why is it done with required steps left?</Label>
						<Input id="task-anyway" bind:value={anywayReason} autocomplete="off" />
						<div class="flex gap-x-2">
							<Button size="sm" disabled={busy || anywayReason.trim() === ''} onclick={confirmAnyway}>Mark done anyway</Button>
							<Button size="sm" variant="outline" onclick={cancelPending}>Cancel</Button>
						</div>
					</div>
				{/if}
				{#if s.done_anyway && item.done_anyway_detail}
					<p class="text-sm/6 text-gray-300" data-testid="done-anyway">Done anyway: {item.done_anyway_detail.reason}</p>
				{/if}

				<div class="space-y-2">
					<Label for="task-assignee">Assignee</Label>
					<select id="task-assignee" disabled={busy} value={s.assignee_id ?? ''} onchange={(e) => change({ method: 'task.assign', item_id: s.id, expected_version: version, assignee_id: e.currentTarget.value || undefined })} class={field}>
						<option value="">Unassigned</option>
						{#each people as p (p.member.id)}<option value={p.member.id}>{p.label}</option>{/each}
					</select>
				</div>

				<div class="space-y-2">
					<Label for="task-description">Description</Label>
					<textarea id="task-description" bind:value={description} rows="4" disabled={busy} class={field}></textarea>
					<Button variant="outline" size="sm" disabled={busy || description === (item.description ?? '')} onclick={() => change({ method: 'task.update', item_id: s.id, expected_version: version, ...(description.trim() === '' ? { clear_description: true } : { description }) })}>Save description</Button>
				</div>

				<section class="space-y-2" aria-labelledby="task-checklist">
					<h3 id="task-checklist" class="text-sm/6 font-semibold text-white">Checklist {#if item.checklist.length > 0}<span class="font-normal text-gray-400">{s.checklist.done} of {s.checklist.total}</span>{/if}</h3>
					<ul class="divide-y divide-white/5 rounded-lg bg-white/5 outline-1 -outline-offset-1 outline-white/10" data-testid="checklist">
						{#each item.checklist as step (step.id)}
							<li class="flex items-center gap-x-3 px-3 py-2 text-sm/6">
								<Checkbox checked={step.done} disabled={busy || !!step.task_id} aria-label={step.text} onCheckedChange={(on) => change({ method: 'checklist.check', step_id: step.id, done: on === true }  )} />
								<span class="min-w-0 flex-1 [overflow-wrap:anywhere] {step.done ? 'text-gray-400 line-through' : 'text-white'}">{step.text}</span>
								{#if step.required}<Badge variant="secondary">Required</Badge>{/if}
								<button type="button" class="rounded p-1 text-gray-400 hover:text-white focus-visible:outline-2 focus-visible:outline-indigo-400" aria-label={`Remove step ${step.text}`} disabled={busy} onclick={() => change({ method: 'checklist.remove', step_id: step.id, expected_version: step.version }  )}><Trash2 class="size-4" aria-hidden="true" /></button>
							</li>
						{:else}
							<li class="px-3 py-2 text-sm/6 text-gray-400">No steps yet.</li>
						{/each}
					</ul>
					<form class="flex gap-x-2" onsubmit={addStep}>
						<Input aria-label="New step" placeholder="Add a step" bind:value={newStep} maxlength={WORK_LIMITS.stepText * 2} disabled={busy} autocomplete="off" />
						<label class="flex shrink-0 items-center gap-x-1.5 text-sm/6 text-gray-300"><Checkbox bind:checked={newRequired} aria-label="Required" /> Required</label>
						<Button type="submit" variant="outline" disabled={busy || newStep.trim() === ''}>Add step</Button>
					</form>
				</section>

				<p class="text-sm/6 text-gray-400" data-testid="task-time">{elapsed(item.time)}</p>

				{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}

				<div class="border-t border-white/10 pt-4">
					{#if confirmDelete}
						<p class="text-sm/6 text-gray-300">Delete {taskRef(s)}? Its history stays and its number is never reused.</p>
						<div class="mt-2 flex gap-x-2">
							<Button variant="destructive" size="sm" disabled={busy} onclick={remove}>Delete task</Button>
							<Button variant="outline" size="sm" onclick={() => (confirmDelete = false)}>Keep</Button>
						</div>
					{:else}
						<Button variant="outline" size="sm" onclick={() => (confirmDelete = true)}>Delete…</Button>
					{/if}
				</div>
			</div>
		{:else if error}
			<p class="px-4 text-sm/6 text-red-400" role="alert">{error}</p>
		{/if}
	</SheetContent>
</Sheet>
