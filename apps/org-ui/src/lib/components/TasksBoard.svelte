<script lang="ts">
	import { onMount } from 'svelte';
	import Ellipsis from '@lucide/svelte/icons/ellipsis';
	import Plus from '@lucide/svelte/icons/plus';
	import { toast } from 'svelte-sonner';
	import NewTaskDialog from '$lib/components/NewTaskDialog.svelte';
	import TaskSheet from '$lib/components/TaskSheet.svelte';
	import { Button } from '$lib/components/ui/button';
	import { DropdownMenu, DropdownMenuContent, DropdownMenuGroup, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from '$lib/components/ui/dropdown-menu';
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
	// Keep the reference while the Block dialog finishes closing.
	let blockTitle = $state('');
	let blockReason = $state('');
	let blockError = $state<string | undefined>();
	let announcement = $state('');
	let announced = 0;
	// The same words twice must be read twice: a zero-width mark makes the text differ.
	const announce = (text: string) => (announcement = text + (announced++ % 2 ? '\u200b' : ''));
	// Board or List is remembered per project (TSK-9); storage can be unavailable, then it is just this visit's.
	const viewKey = $derived(`agentic-org.tasks.view.${projectId}`);
	let view = $state<'board' | 'list'>('board');
	function setView(next: 'board' | 'list') {
		view = next;
		try {
			localStorage.setItem(viewKey, next);
		} catch {
			// not remembered
		}
	}
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
	onMount(() => {
		try {
			if (localStorage.getItem(viewKey) === 'list') view = 'list';
		} catch {
			// the default
		}
		void load();
	});

	async function move(task: WorkItemSummary, to: TaskStatus) {
		if (to === 'blocked') {
			blocking = task;
			blockTitle = taskRef(task);
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

</script>

<div class="space-y-4">
	<div class="flex items-center justify-between gap-3">
		<p class="text-sm/6 text-gray-400">Tasks in {projectName}. <span class="rounded bg-white/10 px-1.5 text-xs/5 text-gray-200">Sample data</span></p>
		<div class="flex items-center gap-x-2">
			<div role="group" aria-label="View" class="inline-flex rounded-lg bg-white/5 p-1 outline-1 -outline-offset-1 outline-white/10">
				{#each [['board', 'Board'], ['list', 'List']] as const as [id, name] (id)}
					<button type="button" aria-pressed={view === id} onclick={() => setView(id)} class="rounded-md px-3 py-1 text-sm/6 font-semibold focus-visible:outline-2 focus-visible:outline-indigo-400 {view === id ? 'bg-white/10 text-white' : 'text-gray-400 hover:text-white'}">{name}</button>
				{/each}
			</div>
			<Button onclick={() => (adding = true)}><Plus aria-hidden="true" /> New task</Button>
		</div>
	</div>

	{#if loadError}
		<p class="rounded-xl bg-white/5 px-6 py-8 text-center text-sm/6 text-red-300" role="alert">{loadError} <button class="underline" onclick={load}>Try again</button></p>
	{:else if tasks === undefined}
		<p class="text-sm/6 text-gray-400">Loading tasks…</p>
	{:else}
		{#if view === 'list'}
			<div class="overflow-x-auto rounded-xl bg-white/5 outline-1 -outline-offset-1 outline-white/10">
				<table class="w-full text-left text-sm/6" data-testid="task-list">
					<caption class="sr-only">Tasks</caption>
					<thead class="text-xs/5 text-gray-400">
						<tr>
							<th scope="col" class="px-4 py-2 font-medium">Task</th>
							<th scope="col" class="px-4 py-2 font-medium">Title</th>
							<th scope="col" class="px-4 py-2 font-medium">Status</th>
							<th scope="col" class="px-4 py-2 font-medium">Priority</th>
							<th scope="col" class="px-4 py-2 font-medium">Assignee</th>
							<th scope="col" class="px-4 py-2 font-medium">Steps</th>
						</tr>
					</thead>
					<tbody class="divide-y divide-white/5 text-gray-200">
						{#each cols.flatMap((c) => c.tasks) as task (task.id)}
							<tr data-testid="task-row" data-task={task.id}>
								<td class="px-4 py-2 text-gray-400">{taskRef(task)}</td>
								<th scope="row" class="px-4 py-2 font-normal"><button type="button" class="text-left font-semibold text-white hover:underline focus-visible:outline-2 focus-visible:outline-indigo-400 [overflow-wrap:anywhere]" onclick={() => ((startSheet = undefined), (openId = task.id), (sheetOpen = true))}>{task.title}</button></th>
								<td class="px-4 py-2">{STATUS_LABEL[task.status as TaskStatus]}{task.blocked_reason ? `: ${task.blocked_reason}` : ''}</td>
								<td class="px-4 py-2 {task.priority === 'urgent' ? 'font-semibold text-amber-300' : ''}">{priorityWord(task.priority) ?? ''}</td>
								<td class="px-4 py-2">{nameOf(snapshot, task.assignee_id)}</td>
								<td class="px-4 py-2 text-gray-400">{checklistWords(task)}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
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
								<div class="mt-1 flex justify-end">
									<DropdownMenu>
										<DropdownMenuTrigger class="rounded p-1 text-gray-400 hover:text-white focus-visible:outline-2 focus-visible:outline-indigo-400" aria-label={`Actions for ${taskRef(task)}`}><Ellipsis class="size-4" aria-hidden="true" /></DropdownMenuTrigger>
										<DropdownMenuContent align="end">
											<DropdownMenuGroup>
												<DropdownMenuLabel>Move {taskRef(task)} to</DropdownMenuLabel>
												{#each movesFrom(task.status) as m (m)}
													<DropdownMenuItem onSelect={() => void move(task, m)}>{STATUS_LABEL[m]}</DropdownMenuItem>
												{/each}
											</DropdownMenuGroup>
										</DropdownMenuContent>
									</DropdownMenu>
								</div>
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
		{/if}
		{#if tasks.length === 0}
			<p class="text-sm/6 text-gray-400" data-testid="no-tasks">No tasks yet. Create the first one.</p>
		{/if}
	{/if}
	<p class="sr-only" role="status" aria-live="polite">{announcement}</p>
</div>

<NewTaskDialog bind:open={adding} {projectId} {projectName} {snapshot} {send} ondone={(created) => { toast.success(`Task ${created.ref} created${created.assigneeId ? ` (and given to ${nameOf(snapshot, created.assigneeId)})` : ''}.`); void load(); }} />
<TaskSheet bind:open={sheetOpen} {organizationId} taskId={openId} {snapshot} {send} start={startSheet} onchanged={(message) => { if (message) announce(message); void load(); }} />

<Dialog open={!!blocking} onOpenChange={(next) => !next && (blocking = undefined)}>
	<DialogContent class="sm:max-w-md">
		<DialogHeader>
			<DialogTitle>Block {blockTitle}</DialogTitle>
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
