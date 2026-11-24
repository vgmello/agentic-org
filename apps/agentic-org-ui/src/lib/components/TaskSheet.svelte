<script lang="ts">
	import DraftableText from '$lib/modules/copilot/features/draft-text/DraftableText.svelte';
	import { onDestroy, untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { resolve } from '$app/paths';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { toAppError } from '$lib/office/api';
	import { newId } from '$lib/org/commands';
	import type { OrgSnapshot, Priority, TaskStatus, WorkItem } from '$lib/proto';
	import { WORK_LIMITS } from '$lib/proto';
	import { workApi, type WorkDraft } from '$lib/work';
	import { assigneeOptions, assignWarning, nameOf } from '$lib/work/people';
	import { timeBlock } from '$lib/work/time';
	import { requestRef } from '$lib/work/requests-view';
	import { bugRef } from '$lib/work/bugs';
	import { shortDate } from '$lib/work/format';
	import { movesFrom, PRIORITIES, priorityWord, STATUS_LABEL, taskRef } from '$lib/work/tasks';
	import { priorityTone, statusTone, type WorkTone } from '$lib/work/tones';

	/** The task page (work-items-ux §9): its fields, status, checklist and time. Each change is one call; a stale one reloads. */
	let {
		open = $bindable(false),
		organizationId,
		taskId,
		snapshot,
		send,
		onchanged,
		start,
		inline = false
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
		/** Render inline inside Work's split reading panel instead of a floating Sheet overlay. */
		inline?: boolean;
	} = $props();

	let item = $state<WorkItem | undefined>();
	// The bugs this task fixes, with their numbers and titles.
	let fixed = $state<{ id: string; ref: string; title: string }[]>([]);
	// The request this task was made from, when an approval made it ("from REQ-12").
	let fromRequest = $state<{ id: string; ref: string; title: string } | undefined>();
	let error = $state<string | undefined>();
	let busy = $state(false);
	let title = $state('');
	let description = $state('');
	let definitionOfDone = $state('');
	let newStep = $state('');
	let newRequired = $state(false);
	let blockedReason = $state('');
	let anywayReason = $state('');
	// The status the owner picked that needs more words before it is sent.
	let pending = $state<'blocked' | 'done_anyway' | undefined>();
	let confirmDelete = $state(false);
	let saved = $state('');
	let assignPending = $state<string | undefined>();
	let statusSelect = $state<HTMLSelectElement | undefined>();

	let loadSeq = 0;
	let saveSeq = 0;
	let pendingSave: Promise<boolean> | undefined;
	let destroyed = false;

	/** Wait for any in-flight save and confirm no inline error or unsaved draft blocks closing. */
	export async function waitForPendingSave(): Promise<boolean> {
		if (pendingSave) {
			const ok = await pendingSave;
			if (!ok) return false;
		}
		if (error) return false;
		if (
			item &&
			(title.trim() !== item.summary.title ||
				(description.trim() === '' ? '' : description) !== (item.description ?? '') ||
				(definitionOfDone.trim() === '' ? '' : definitionOfDone) !==
					(item.definition_of_done ?? ''))
		) {
			return false;
		}
		return true;
	}

	onDestroy(() => {
		destroyed = true;
	});

	async function load(reset: boolean) {
		if (!taskId) return;
		const forOrg = organizationId;
		const forTask = taskId;
		const seq = ++loadSeq;
		try {
			const got = await workApi.get(forOrg, forTask);
			if (destroyed || forOrg !== organizationId || forTask !== taskId || seq !== loadSeq) return;
			const prev = item;
			item = got;
			if (reset) {
				title = got.summary.title;
				description = got.description ?? '';
				definitionOfDone = got.definition_of_done ?? '';
			} else if (prev) {
				// Refresh untouched fields from the newer server copy while preserving unsaved edits.
				if (title === prev.summary.title) title = got.summary.title;
				if (description === (prev.description ?? '')) description = got.description ?? '';
				if (definitionOfDone === (prev.definition_of_done ?? ''))
					definitionOfDone = got.definition_of_done ?? '';
			}
			const fixedNow = (
				await Promise.all(
					got.links
						.filter((l) => l.kind === 'fixes' && l.to.type === 'item')
						.map((l) =>
							workApi.get(forOrg, (l.to as { item_id: string }).item_id).catch(() => undefined)
						)
				)
			).flatMap((b) =>
				b ? [{ id: b.summary.id, ref: bugRef(b.summary), title: b.summary.title }] : []
			);
			const origin = got.links.find((l) => l.kind === 'from_request' && l.to.type === 'item');
			const originNow = origin
				? await workApi
						.get(forOrg, (origin.to as { item_id: string }).item_id)
						.then((r) => ({ id: r.summary.id, ref: requestRef(r.summary), title: r.summary.title }))
						.catch(() => undefined)
				: undefined;
			// Another task may have been opened while these were read: they are not its.
			if (destroyed || forOrg !== organizationId || forTask !== taskId || seq !== loadSeq) return;
			fixed = fixedNow;
			fromRequest = originNow;
		} catch (e) {
			// An answer for a task that is no longer the one open says nothing about this one.
			if (!destroyed && forOrg === organizationId && forTask === taskId && seq === loadSeq)
				error = toAppError(e).message;
		}
	}

	$effect(() => {
		if (open && taskId) {
			untrack(() => {
				item = undefined;
				fixed = [];
				fromRequest = undefined;
				error = undefined;
				busy = false;
				saveSeq += 1;
				newStep = blockedReason = anywayReason = '';
				newRequired = false;
				pending = start?.pending;
				if (start) error = start.message;
				confirmDelete = false;
				saved = '';
				assignPending = undefined;
				void load(true);
			});
		} else {
			untrack(() => {
				busy = false;
				saveSeq += 1;
			});
		}
	});

	/** Send one change; on success reload the task and the board, on a stale one say so and reload. */
	async function change(draft: WorkDraft, announce?: string): Promise<boolean> {
		if (busy || !taskId) return false;
		const forOrg = organizationId;
		const forTask = taskId;
		const seq = ++saveSeq;
		busy = true;
		error = undefined;
		const run = (async () => {
			try {
				await send(draft);
				if (destroyed || seq !== saveSeq || forOrg !== organizationId || forTask !== taskId) {
					if (forOrg === organizationId) onchanged(announce);
					return false;
				}
				// What was typed in the other fields stays: only a stale change (below) takes the latest.
				await load(false);
				if (destroyed || seq !== saveSeq || forOrg !== organizationId || forTask !== taskId) {
					if (forOrg === organizationId) onchanged(announce);
					return false;
				}
				onchanged(announce);
				return true;
			} catch (e) {
				const err = toAppError(e);
				if (destroyed || seq !== saveSeq || forOrg !== organizationId || forTask !== taskId) {
					if (forOrg === organizationId) toast.error(err.message);
					return false;
				}
				error = err.message;
				if (err.source === 'api' && err.code === 'conflict') await load(false);
				if (err.source === 'api' && err.code === 'checklist_incomplete') pending = 'done_anyway';
				return false;
			} finally {
				if (!destroyed && seq === saveSeq && forOrg === organizationId && forTask === taskId)
					busy = false;
			}
		})();
		pendingSave = run;
		try {
			return await run;
		} finally {
			if (pendingSave === run) pendingSave = undefined;
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
		if (
			await change({
				method: 'task.set_status',
				item_id: s.id,
				expected_version: version,
				status: 'blocked',
				blocked_reason: blockedReason.trim()
			})
		) {
			pending = undefined;
			blockedReason = '';
		}
	}
	async function confirmAnyway() {
		if (!s || anywayReason.trim() === '') return;
		if (
			await change({
				method: 'task.set_status',
				item_id: s.id,
				expected_version: version,
				status: 'done',
				anyway_reason: anywayReason.trim()
			})
		) {
			pending = undefined;
			anywayReason = '';
		}
	}
	async function addStep(e: SubmitEvent) {
		e.preventDefault();
		if (!s || newStep.trim() === '') return;
		if (
			await change({
				method: 'checklist.add',
				item_id: s.id,
				steps: [{ step_id: newId(), text: newStep.trim(), required: newRequired }]
			})
		)
			((newStep = ''), (newRequired = false));
	}
	async function remove() {
		if (!s) return;
		if (await change({ method: 'work.delete', item_id: s.id, expected_version: version }))
			open = false;
	}
	/** Save a text field when it is left, if it changed, and say so (one save model: no Save buttons). */
	async function saveField(what: 'Title' | 'Description' | 'Definition of done') {
		if (!s || !item || busy || !taskId) return;
		const forOrg = organizationId;
		const forTask = taskId;
		const base = { item_id: s.id, expected_version: version } as const;
		let draft: WorkDraft | undefined;
		if (what === 'Title' && title.trim() !== s.title)
			draft =
				title.trim() === '' ? undefined : { method: 'task.update', ...base, title: title.trim() };
		if (what === 'Title' && title.trim() === '') title = s.title;
		if (what === 'Description' && description !== (item.description ?? ''))
			draft = {
				method: 'task.update',
				...base,
				...(description.trim() === '' ? { clear_description: true } : { description })
			};
		if (what === 'Definition of done' && definitionOfDone !== (item.definition_of_done ?? ''))
			draft = {
				method: 'task.update',
				...base,
				...(definitionOfDone.trim() === ''
					? { clear_definition_of_done: true }
					: { definition_of_done: definitionOfDone })
			};
		if (!draft) return;
		saved = '';
		if (await change(draft)) {
			if (!destroyed && forOrg === organizationId && forTask === taskId) saved = `${what} saved.`;
		}
	}
	function requestAssign(id: string) {
		if (!s) return;
		if (assignWarning(snapshot, id || undefined)) assignPending = id;
		else
			void change({
				method: 'task.assign',
				item_id: s.id,
				expected_version: version,
				assignee_id: id || undefined
			});
	}
	async function confirmAssign() {
		if (!s || !assignPending) return;
		if (
			await change({
				method: 'task.assign',
				item_id: s.id,
				expected_version: version,
				assignee_id: assignPending
			})
		)
			assignPending = undefined;
	}
	function cancelAssign() {
		assignPending = undefined;
		const select = document.getElementById('task-assignee') as HTMLSelectElement | null;
		if (select && s) select.value = s.assignee_id ?? '';
	}
	const field =
		'w-full rounded-md bg-surface-raised px-2.5 py-2 text-sm/6 text-fg outline-1 -outline-offset-1 outline-field-border focus-visible:outline-2 focus-visible:outline-focus';
	const actionBtn = 'h-auto min-h-9 max-[639px]:min-h-11 [@media(pointer:coarse)]:min-h-11';

	function pillToneClass(tone: WorkTone): string {
		if (tone === 'ok') return 't-green';
		if (tone === 'warn') return 't-amber';
		if (tone === 'bad') return 't-red';
		if (tone === 'accent') return '';
		return 'bg-surface-raised text-fg-secondary ring-1 ring-line ring-inset';
	}
</script>

{#snippet badges()}
	{#if s}
		{@const prio = priorityWord(s.priority)}
		{@const sTone = statusTone('task', s.status)}
		{@const pTone = priorityTone(s.priority)}
		<div class="mt-2 flex flex-wrap items-center gap-1.5">
			<span
				class={[
					'inline-flex items-center rounded-full px-2 py-0.5 text-xs/4 font-medium',
					sTone === 'accent' ? 'task-sheet-accent' : pillToneClass(sTone)
				]}>{STATUS_LABEL[s.status as TaskStatus] ?? s.status}</span
			>
			{#if prio}
				<span
					class={[
						'inline-flex items-center rounded-full px-2 py-0.5 text-xs/4 font-medium',
						pTone === 'accent' ? 'task-sheet-accent' : pillToneClass(pTone)
					]}>{prio}</span
				>
			{/if}
		</div>
	{/if}
{/snippet}

{#snippet body()}
	{#if s && item}
		<div
			class={['flex flex-col gap-5', inline ? 'px-0 pt-4 pb-2' : 'px-4 pt-4 pb-6']}
			data-testid="task-page"
		>
			<div class="flex flex-col gap-2">
				<Label for="task-title">Title</Label>
				<Input
					id="task-title"
					bind:value={title}
					maxlength={WORK_LIMITS.title * 2}
					disabled={busy}
					autocomplete="off"
					onblur={() => saveField('Title')}
				/>
			</div>

			{#if fromRequest}<p
					class="text-sm/6 text-fg-secondary [overflow-wrap:anywhere]"
					data-testid="task-from-request"
				>
					From <a
						class="font-medium text-link underline hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
						href={resolve(`/work?request=${fromRequest.id}` as '/work')}
						onclick={() => (open = false)}>{fromRequest.ref} · {fromRequest.title}</a
					>
				</p>{/if}
			{#if fixed.length > 0}<p class="text-sm/6 text-fg-secondary" data-testid="task-fixes">
					Fixes {fixed.map((b) => `${b.ref} · ${b.title}`).join(', ')}
				</p>{/if}

			<div class="grid gap-4 sm:grid-cols-2">
				<div class="flex flex-col gap-2">
					<Label for="task-status">Status</Label>
					<select
						id="task-status"
						bind:this={statusSelect}
						disabled={busy}
						value={s.status}
						onchange={(e) => setStatus(e.currentTarget.value as TaskStatus)}
						class={field}
					>
						<option value={s.status}
							>{STATUS_LABEL[s.status as TaskStatus]}{s.status === 'done'
								? s.resolution === 'cancelled'
									? ' · Cancelled'
									: ' · Completed'
								: ''}</option
						>
						{#each movesFrom(s.status) as m (m)}<option value={m}>{STATUS_LABEL[m]}</option>{/each}
					</select>
				</div>
				<div class="flex flex-col gap-2">
					<Label for="task-priority">Priority</Label>
					<select
						id="task-priority"
						disabled={busy}
						value={s.priority ?? 'normal'}
						onchange={(e) =>
							change(
								{
									method: 'task.update',
									item_id: s.id,
									expected_version: version,
									priority: e.currentTarget.value as Priority
								},
								`${taskRef(s)} priority is now ${PRIORITIES.find((x) => x.id === e.currentTarget.value)?.label}.`
							)}
						class={field}
					>
						{#each PRIORITIES as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
					</select>
				</div>
			</div>

			{#if s.status === 'blocked' && s.blocked_reason}
				<p class="t-amber rounded-md px-3 py-2 text-sm/6 font-medium" data-testid="blocked-reason">
					Blocked: {s.blocked_reason}
				</p>
			{/if}
			{#if pending === 'blocked'}
				<div
					class="flex flex-col gap-2 rounded-md bg-surface-raised p-3 outline-1 -outline-offset-1 outline-line"
				>
					<Label for="task-blocked-reason">What is it blocked on?</Label>
					<Input
						id="task-blocked-reason"
						bind:value={blockedReason}
						maxlength={WORK_LIMITS.blockedReason * 2}
						autocomplete="off"
					/>
					<div class="flex gap-x-2">
						<Button
							size="sm"
							class={actionBtn}
							disabled={busy || blockedReason.trim() === ''}
							onclick={confirmBlocked}>Block task</Button
						>
						<Button size="sm" variant="outline" class={actionBtn} onclick={cancelPending}
							>Cancel</Button
						>
					</div>
				</div>
			{/if}
			{#if pending === 'done_anyway'}
				<div
					class="flex flex-col gap-2 rounded-md bg-surface-raised p-3 outline-1 -outline-offset-1 outline-line"
				>
					<Label for="task-anyway">Why is it done with required steps left?</Label>
					<Input id="task-anyway" bind:value={anywayReason} autocomplete="off" />
					<div class="flex gap-x-2">
						<Button
							size="sm"
							class={actionBtn}
							disabled={busy || anywayReason.trim() === ''}
							onclick={confirmAnyway}>Mark done anyway</Button
						>
						<Button size="sm" variant="outline" class={actionBtn} onclick={cancelPending}
							>Cancel</Button
						>
					</div>
				</div>
			{/if}
			{#if s.done_anyway && item.done_anyway_detail}
				<p class="text-sm/6 text-fg-secondary" data-testid="done-anyway">
					Done anyway: {item.done_anyway_detail.reason}
				</p>
			{/if}

			<div class="flex flex-col gap-2">
				<Label for="task-assignee">Assignee</Label>
				<select
					id="task-assignee"
					disabled={busy}
					value={s.assignee_id ?? ''}
					onchange={(e) => requestAssign(e.currentTarget.value)}
					class={field}
				>
					<option value="">Unassigned</option>
					{#each people as p (p.member.id)}<option value={p.member.id}>{p.label}</option>{/each}
				</select>
				{#if assignPending && assignWarning(snapshot, assignPending)}
					<div
						class="t-amber flex flex-col gap-2 rounded-md p-3 text-sm/6"
						role="alert"
						data-testid="assign-warning"
					>
						<p>{assignWarning(snapshot, assignPending)}</p>
						<div class="flex gap-x-2">
							<Button size="sm" class={actionBtn} disabled={busy} onclick={confirmAssign}
								>Assign anyway</Button
							><Button size="sm" variant="outline" class={actionBtn} onclick={cancelAssign}
								>Cancel</Button
							>
						</div>
					</div>
				{/if}
			</div>

			<DraftableText
				id="task-description"
				label="Description"
				rows={4}
				bind:value={description}
				limit={WORK_LIMITS.description}
				disabled={busy}
				onblur={() => saveField('Description')}
				onchange={() => saveField('Description')}
			/>
			<div class="flex flex-col gap-2">
				<Label for="task-dod">Definition of done</Label>
				<textarea
					id="task-dod"
					bind:value={definitionOfDone}
					rows="2"
					maxlength={WORK_LIMITS.definitionOfDone * 2}
					disabled={busy}
					placeholder="The goal the agent works to"
					class={field}
					onblur={() => saveField('Definition of done')}></textarea>
				<p class="h-5 text-xs/5 text-fg-muted" role="status" data-testid="saved">{saved}</p>
			</div>

			<section class="flex flex-col gap-2" aria-labelledby="task-checklist">
				<h3
					id="task-checklist"
					class="text-xs/5 font-semibold tracking-wider text-fg-muted uppercase"
				>
					Checklist {#if item.checklist.length > 0}<span
							class="font-normal text-fg-muted tabular-nums"
							>{s.checklist.done} of {s.checklist.total}</span
						>{/if}
				</h3>
				<ul
					class="divide-y divide-line-subtle rounded-lg bg-surface-raised outline-1 -outline-offset-1 outline-line"
					data-testid="checklist"
				>
					{#each item.checklist as step (step.id)}
						<li class="flex items-center gap-x-3 px-3 py-2 text-sm/6">
							<Checkbox
								checked={step.done}
								disabled={busy || !!step.task_id}
								aria-label={step.text}
								onCheckedChange={(on) =>
									change({ method: 'checklist.check', step_id: step.id, done: on === true })}
							/>
							<span
								class="min-w-0 flex-1 [overflow-wrap:anywhere] {step.done
									? 'text-fg-muted line-through'
									: 'text-fg'}">{step.text}</span
							>
							{#if step.required}<Badge variant="secondary">Required</Badge>{/if}
							<button
								type="button"
								class="rounded p-1 text-fg-muted hover:text-fg focus-visible:outline-2 focus-visible:outline-focus"
								aria-label={`Remove step ${step.text}`}
								disabled={busy}
								onclick={() =>
									change({
										method: 'checklist.remove',
										step_id: step.id,
										expected_version: step.version
									})}><Trash2 class="size-4" aria-hidden="true" /></button
							>
						</li>
					{:else}
						<li class="px-3 py-2 text-sm/6 text-fg-muted">No steps yet.</li>
					{/each}
				</ul>
				<form class="flex gap-x-2" onsubmit={addStep}>
					<Input
						aria-label="New step"
						placeholder="Add a step"
						bind:value={newStep}
						maxlength={WORK_LIMITS.stepText * 2}
						disabled={busy}
						autocomplete="off"
					/>
					<label class="flex shrink-0 items-center gap-x-1.5 text-sm/6 text-fg-secondary"
						><Checkbox bind:checked={newRequired} aria-label="Required" /> Required</label
					>
					<Button
						type="submit"
						variant="outline"
						class={actionBtn}
						disabled={busy || newStep.trim() === ''}>Add step</Button
					>
				</form>
			</section>

			<p class="text-xs/5 text-fg-muted tabular-nums" data-testid="task-time">
				{timeBlock(item.time)}
			</p>

			{#if error}<p class="text-sm/6 text-danger-text [overflow-wrap:anywhere]" role="alert">
					{error}
				</p>{/if}

			<div class="border-t border-line pt-4">
				{#if confirmDelete}
					<p class="text-sm/6 text-fg-secondary">
						Delete {taskRef(s)}? Its history stays and its number is never reused.
					</p>
					<div class="mt-2 flex gap-x-2">
						<Button
							variant="destructive"
							size="sm"
							class={actionBtn}
							disabled={busy}
							onclick={remove}>Delete task</Button
						>
						<Button
							variant="outline"
							size="sm"
							class={actionBtn}
							onclick={() => (confirmDelete = false)}>Keep</Button
						>
					</div>
				{:else}
					<Button
						variant="outline"
						size="sm"
						class={actionBtn}
						onclick={() => (confirmDelete = true)}>Delete…</Button
					>
				{/if}
			</div>
		</div>
	{:else if error}
		<p class={['py-4 text-sm/6 text-danger-text', inline ? 'px-0' : 'px-4']} role="alert">
			{error}
		</p>
	{/if}
{/snippet}

{#if inline}
	{#if open}
		<header class="border-b border-line pb-4">
			<h2
				id="work-detail-title"
				tabindex="-1"
				class="text-lg/7 font-semibold text-fg [overflow-wrap:anywhere] focus:outline-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
			>
				{s ? s.title : 'Task'}
			</h2>
			<p class="mt-1 text-sm/6 text-fg-muted">
				{s ? `Created ${shortDate(s.created_at)} by ${nameOf(snapshot, s.created_by)}` : 'Loading…'}
			</p>
			{@render badges()}
		</header>
		{@render body()}
	{/if}
{:else}
	<Sheet bind:open>
		<SheetContent side="right" class="w-full max-w-lg overflow-y-auto">
			<SheetHeader class="border-b border-line pb-4">
				<SheetTitle class="[overflow-wrap:anywhere]"
					>{s ? `${taskRef(s)} · ${s.title}` : 'Task'}</SheetTitle
				>
				<SheetDescription
					>{s
						? `Created ${shortDate(s.created_at)} by ${nameOf(snapshot, s.created_by)}`
						: 'Loading…'}</SheetDescription
				>
				{@render badges()}
			</SheetHeader>
			{@render body()}
		</SheetContent>
	</Sheet>
{/if}

<style>
	.task-sheet-accent {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
</style>
