<script lang="ts">
	import { onMount } from 'svelte';
	import Plus from '@lucide/svelte/icons/plus';
	import { toast } from 'svelte-sonner';
	import NewTaskDialog from '$lib/components/NewTaskDialog.svelte';
	import TaskSheet from '$lib/components/TaskSheet.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/office/api';
	import type { OrgSnapshot, TaskStatus, WorkItemSummary } from '$lib/proto';
	import { WORK_LIMITS } from '$lib/proto';
	import { sender, workApi } from '$lib/work';
	import { nameOf } from '$lib/work/people';
	import { checklistWords, columns, lastWeek, movesFrom, priorityWord, STATUS_LABEL, taskRef } from '$lib/work/tasks';

	/** A project's tasks as a board (work-items-ux §9; TSK-1..TSK-5, TSK-10). */
	let {
		organizationId,
		projectId,
		projectName,
		snapshot
	}: { organizationId: string; projectId: string; projectName: string; snapshot: OrgSnapshot } = $props();

	let tasks = $state<WorkItemSummary[] | undefined>();
	let loadError = $state<string | undefined>();
	let showAllDone = $state(false);
	let adding = $state(false);
	let openId = $state<string | undefined>();
	let sheetOpen = $state(false);
	let blocking = $state<WorkItemSummary | undefined>();
	let blockReason = $state('');
	let blockError = $state<string | undefined>();
	let announcement = $state('');
	let announced = 0;
	// The same words twice must be read twice: a zero-width mark makes the text differ.
	const announce = (text: string) => (announcement = text + (announced++ % 2 ? '\u200b' : ''));
	let startSheet = $state<{ pending: 'done_anyway'; message: string } | undefined>();

	const send = $derived(sender(organizationId));
	const cols = $derived(columns(tasks ?? []));

	async function load() {
		try {
			tasks = (await workApi.snapshot(organizationId, projectId, showAllDone ? undefined : lastWeek())).items;
			loadError = undefined;
		} catch (e) {
			// A board that was showing stays; only a first load that failed has nothing to show.
			if (tasks === undefined) loadError = toAppError(e).message;
			else toast.error(toAppError(e).message);
		}
	}
	onMount(load);

	async function move(task: WorkItemSummary, to: TaskStatus) {
		if (to === 'blocked') {
			blocking = task;
			blockReason = '';
			blockError = undefined;
			return;
		}
		try {
			await send({ method: 'task.set_status', item_id: task.id, expected_version: task.version, status: to });
			announce(`${taskRef(task)} moved to ${STATUS_LABEL[to]}.`);
		} catch (e) {
			const err = toAppError(e);
			if (err.source === 'api' && err.code === 'checklist_incomplete') {
				// Needs a reason: the task page asks for it, with the message.
				startSheet = { pending: 'done_anyway', message: err.message };
				openId = task.id;
				sheetOpen = true;
			} else toast.error(err.message);
		}
		await load();
	}

	async function confirmBlock(e: SubmitEvent) {
		e.preventDefault();
		// The latest of the task: a retry after a stale answer must send the version it now has.
		const task = tasks?.find((t) => t.id === blocking?.id) ?? blocking;
		if (!task || blockReason.trim() === '') return;
		try {
			await send({ method: 'task.set_status', item_id: task.id, expected_version: task.version, status: 'blocked', blocked_reason: blockReason.trim() });
			announce(`${taskRef(task)} moved to Blocked.`);
			blocking = undefined;
		} catch (error) {
			blockError = toAppError(error).message;
		}
		await load();
	}

	const select = 'mt-2 w-full rounded-md bg-gray-800 px-1.5 py-1 text-xs/5 text-gray-200 outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400';
</script>

<div class="space-y-4">
	<div class="flex items-center justify-between gap-3">
		<p class="text-sm/6 text-gray-400">Tasks in {projectName}. <span class="rounded bg-white/10 px-1.5 text-xs/5 text-gray-200">Sample data</span></p>
		<Button onclick={() => (adding = true)}><Plus aria-hidden="true" /> New task</Button>
	</div>

	{#if loadError}
		<p class="rounded-xl bg-white/5 px-6 py-8 text-center text-sm/6 text-red-300" role="alert">{loadError} <button class="underline" onclick={load}>Try again</button></p>
	{:else if tasks === undefined}
		<p class="text-sm/6 text-gray-400">Loading tasks…</p>
	{:else}
		<div class="grid gap-3 md:grid-cols-2 xl:grid-cols-4" data-testid="task-board">
			{#each cols as col (col.status)}
				<section class="rounded-xl bg-white/5 p-3 outline-1 -outline-offset-1 outline-white/10" aria-labelledby={`col-${col.status}`} data-testid="task-column" data-status={col.status}>
					<h3 id={`col-${col.status}`} class="flex items-center justify-between text-sm/6 font-semibold text-white">{col.label} <span class="font-normal text-gray-400">{col.tasks.length}</span></h3>
					<ul class="mt-2 space-y-2">
						{#each col.tasks as task (task.id)}
							{@const word = priorityWord(task.priority)}
							<li class="rounded-lg bg-gray-800 p-2.5 text-sm/6 outline-1 -outline-offset-1 outline-white/10" data-testid="task-card" data-task={task.id}>
								<button type="button" class="block w-full text-left focus-visible:outline-2 focus-visible:outline-indigo-400" onclick={() => ((startSheet = undefined), (openId = task.id), (sheetOpen = true))}>
									<span class="text-xs/5 text-gray-400">{taskRef(task)}</span>
									<span class="block font-semibold text-white [overflow-wrap:anywhere]">{task.title}</span>
								</button>
								<p class="mt-1 flex flex-wrap items-center gap-x-2 text-xs/5 text-gray-400">
									{#if word}<span class={task.priority === 'urgent' ? 'font-semibold text-amber-300' : 'text-gray-200'} data-testid="priority">{word}</span>{/if}
									<span>{nameOf(snapshot, task.assignee_id)}</span>
									{#if checklistWords(task)}<span>{checklistWords(task)}</span>{/if}
								</p>
								{#if task.status === 'blocked' && task.blocked_reason}<p class="mt-1 text-xs/5 text-amber-200" data-testid="card-blocked">Blocked: {task.blocked_reason}</p>{/if}
								<label class="sr-only" for={`move-${task.id}`}>Move {taskRef(task)} to</label>
								<select id={`move-${task.id}`} class={select} value="" onchange={(e) => { const to = e.currentTarget.value as TaskStatus; e.currentTarget.value = ''; if (to) void move(task, to); }}>
									<option value="">Move to…</option>
									{#each movesFrom(task.status) as m (m)}<option value={m}>{STATUS_LABEL[m]}</option>{/each}
								</select>
							</li>
						{:else}
							<li class="px-1 py-2 text-xs/5 text-gray-500">No tasks.</li>
						{/each}
					</ul>
					{#if col.status === 'done'}
						<Button variant="ghost" size="sm" class="mt-2" onclick={() => { showAllDone = !showAllDone; void load(); }}>{showAllDone ? 'Show last 7 days' : 'Show all done'}</Button>
					{/if}
				</section>
			{/each}
		</div>
		{#if tasks.length === 0}
			<p class="text-sm/6 text-gray-400" data-testid="no-tasks">No tasks yet. Create the first one.</p>
		{/if}
	{/if}
	<p class="sr-only" role="status" aria-live="polite">{announcement}</p>
</div>

<NewTaskDialog bind:open={adding} {projectId} {projectName} {snapshot} {send} ondone={(title) => { toast.success(`Created “${title}”.`); void load(); }} />
<TaskSheet bind:open={sheetOpen} {organizationId} taskId={openId} {snapshot} {send} start={startSheet} onchanged={(message) => { if (message) announce(message); void load(); }} />

<Dialog open={!!blocking} onOpenChange={(next) => !next && (blocking = undefined)}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Block {blocking ? taskRef(blocking) : ''}</DialogTitle>
			<DialogDescription>Say in one line what it is blocked on. The card shows it.</DialogDescription>
		</DialogHeader>
		<form class="space-y-3" onsubmit={confirmBlock} novalidate>
			<Label for="block-reason">Blocked on</Label>
			<Input id="block-reason" bind:value={blockReason} maxlength={WORK_LIMITS.blockedReason * 2} autocomplete="off" />
			{#if blockError}<p class="text-sm/6 text-red-400" role="alert">{blockError}</p>{/if}
			<DialogFooter>
				<Button type="button" variant="outline" onclick={() => (blocking = undefined)}>Cancel</Button>
				<Button type="submit" disabled={blockReason.trim() === ''}>Block task</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
