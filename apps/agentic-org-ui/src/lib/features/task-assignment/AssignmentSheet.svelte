<script lang="ts">
	import { onMount } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Sheet from '$lib/components/ui/sheet';
	import * as Alert from '$lib/components/ui/alert';
	import { Label } from '$lib/components/ui/label';
	import type { Operation, OperationState } from '$lib/platform/http/client';
	import type { TaskFeature } from './feature';
	import { current, type Card, type Completion, type ContentRef, type Target } from './wire';
	import { eventSummary } from './history';
	let {
		feature,
		taskId,
		closed,
		changed
	}: { feature: TaskFeature; taskId: string; closed: () => void; changed: () => void } = $props();
	let open = $state(true),
		loading = $state(true),
		pending = $state(false),
		error = $state('');
	let task = $state<Card>(),
		team = $state(''),
		employee = $state('');
	let teams = $state<Target[]>([]),
		employees = $state<Target[]>([]),
		teamNext = $state<string | null>(null),
		employeeNext = $state<string | null>(null);
	let history = $state<ContentRef[]>([]),
		historyNext = $state<string | null>(null),
		coverage = $state('unknown'),
		source = $state(''),
		sourceSummary = $state('');
	let operation = $state.raw<Operation<Completion>>(),
		outcome = $state<OperationState<Completion>>();
	const blocked = $derived(loading || pending || !!operation);
	const observed = $derived.by(() => {
		if (outcome?.status !== 'rejected' || outcome.error.current === undefined) return undefined;
		try {
			return current(outcome.error.current);
		} catch {
			return undefined;
		}
	});
	const status = $derived.by(() => {
		if (pending && operation)
			return 'Waiting for the office. Keep this operation ID until its outcome is known.';
		switch (outcome?.status) {
			case 'completed':
				return `Assignment saved at revision ${outcome.result.revision}.`;
			case 'rejected':
				return outcome.error.code === 'conflict'
					? 'This task changed before your assignment was accepted. Your edit was not applied.'
					: `Assignment was refused (${outcome.error.code}).`;
			case 'unresolved':
			case 'accepted':
				return 'The outcome is not yet known. Check the original operation; do not submit it again.';
			case 'expired':
				return 'The receipt has expired. This does not show whether the original assignment committed.';
			case 'unsupported':
				return 'This response needs a newer client. Keep the original operation ID.';
			case 'not_submitted':
				return 'The request was not submitted. Resolve the local error before trying again.';
			default:
				return '';
		}
	});
	let live = true;
	async function load(preserveDraft = false) {
		loading = true;
		error = '';
		try {
			const [detail, t, e, h] = await Promise.all([
				feature.reads.detail(taskId),
				feature.reads.targets(taskId, 'team'),
				feature.reads.targets(taskId, 'employee'),
				feature.reads.history(taskId)
			]);
			if (!live) return;
			task = detail.task;
			if (!preserveDraft) {
				team = task.assigned_team_id ?? '';
				employee = task.assignee_id ?? '';
			}
			teams = t.items;
			employees = e.items;
			teamNext = t.next;
			employeeNext = e.next;
			history = h.events;
			historyNext = h.next;
			coverage = h.coverage;
			source = '';
			sourceSummary = '';
		} catch {
			if (live)
				error = 'Task details could not be loaded. Refresh after checking your session and office.';
		} finally {
			if (live) loading = false;
		}
	}
	async function reconcile() {
		if (!operation || pending) return;
		pending = true;
		try {
			const next = await operation.lookup();
			if (live) {
				outcome = next;
				if (next.status === 'completed') {
					await load();
					changed();
				}
			}
		} catch {
			if (live) error = 'Sign in as the original account to check this operation.';
		} finally {
			if (live) pending = false;
		}
	}
	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!task || blocked) return;
		error = '';
		pending = true;
		try {
			operation = feature
				.forTask(taskId)
				.prepare(task.revision, { assigned_team_id: team || null, assignee_id: employee || null });
			const next = await operation.submit();
			if (!live) return;
			outcome = next;
			if (next.status === 'completed') {
				await load();
				changed();
			}
		} catch {
			if (live)
				error =
					'The operation could not start or finish. Keep any displayed operation ID and check its outcome.';
		} finally {
			if (live) pending = false;
		}
	}
	async function newEdit() {
		if (pending || !outcome || !['completed', 'rejected', 'not_submitted'].includes(outcome.status))
			return;
		// A rejected receipt acknowledges the old intent only. Refresh the
		// revision/targets while keeping the person's complete proposed pair;
		// submitting that pair again requires an explicit new Save action.
		await load(outcome.status !== 'completed');
		if (error || !live) return;
		try {
			if (!operation) return;
			feature.forTask(taskId).acknowledge(operation);
			operation = undefined;
			outcome = undefined;
		} catch {
			error = 'The stored operation reference could not be updated.';
		}
	}
	async function more(kind: 'team' | 'employee' | 'history') {
		if (pending) return;
		pending = true;
		try {
			if (kind === 'history') {
				const h = await feature.reads.history(taskId, historyNext ?? undefined);
				if (live) {
					history = [...history, ...h.events];
					historyNext = h.next;
					coverage = h.coverage;
				}
			} else {
				const t = await feature.reads.targets(
					taskId,
					kind,
					(kind === 'team' ? teamNext : employeeNext) ?? undefined
				);
				if (!live) return;
				if (kind === 'team') {
					teams = [...teams, ...t.items];
					teamNext = t.next;
				} else {
					employees = [...employees, ...t.items];
					employeeNext = t.next;
				}
			}
		} catch {
			if (live) error = 'This page could not be loaded.';
		} finally {
			if (live) pending = false;
		}
	}
	async function showSource(ref: ContentRef) {
		pending = true;
		error = '';
		try {
			const text = await feature.reads.content(ref);
			if (live) {
				source = text;
				sourceSummary = eventSummary(text, ref);
			}
		} catch {
			if (live) error = 'The source changed or its content could not be verified. Reload the task.';
		} finally {
			if (live) pending = false;
		}
	}
	onMount(() => {
		void (async () => {
			await load();
			if (!live) return;
			try {
				operation = feature.forTask(taskId).resume();
				if (operation) {
					outcome = operation.state;
					await reconcile();
				}
			} catch {
				error = 'The saved operation reference cannot be reconciled. Keep it for recovery.';
			}
		})();
		return () => {
			live = false;
		};
	});
</script>

<Sheet.Root
	bind:open
	onOpenChange={(value) => {
		if (!value) closed();
	}}
>
	<Sheet.Content class="overflow-y-auto sm:max-w-xl">
		<Sheet.Header>
			<Sheet.Title>{task?.title_preview ?? 'Task assignment'}</Sheet.Title>
			<Sheet.Description
				>{taskId} · {task ? `Revision ${task.revision}` : 'Loading task'}</Sheet.Description
			>
		</Sheet.Header>
		<div class="flex flex-col gap-6 px-4 pb-6">
			{#if loading}<p role="status">Loading task, targets and history…</p>{/if}
			{#if error}<Alert.Root variant="destructive"
					><Alert.Title>Could not continue</Alert.Title><Alert.Description
						>{error}</Alert.Description
					></Alert.Root
				>{/if}
			{#if task}
				<dl class="grid grid-cols-2 gap-2">
					<dt>Accountable team</dt>
					<dd class="break-words">{task.accountable_team_id}</dd>
					<dt>Status</dt>
					<dd>{task.status}</dd>
					<dt>Assigned team</dt>
					<dd class="break-words">{task.assigned_team_id ?? 'None'}</dd>
					<dt>Assignee</dt>
					<dd class="break-words">{task.assignee_id ?? 'None'}</dd>
				</dl>
				<form onsubmit={submit} class="flex min-w-0 flex-col gap-4">
					<fieldset disabled={blocked} class="flex min-w-0 flex-col gap-4">
						<legend class="mb-2 font-medium">Assignment</legend>
						<Label for="selected-team">Assigned team</Label>
						<select
							id="selected-team"
							bind:value={team}
							class="border-input bg-background w-full min-w-0 rounded-md border p-2"
							><option value="">No assigned team</option
							>{#if team && !teams.some((t) => t.id === team)}<option value={team}
									>{team} (current; unavailable)</option
								>{/if}{#each teams as t (t.id)}<option value={t.id} disabled={!t.active}
									>{t.name_preview} · {t.id}</option
								>{/each}</select
						>
						<Button type="button" variant="outline" onclick={() => (team = '')}
							>Clear assigned team</Button
						>
						<Label for="selected-employee">Assignee</Label>
						<select
							id="selected-employee"
							bind:value={employee}
							class="border-input bg-background w-full min-w-0 rounded-md border p-2"
							><option value="">No assignee</option
							>{#if employee && !employees.some((e) => e.id === employee)}<option value={employee}
									>{employee} (current; unavailable)</option
								>{/if}{#each employees as e (e.id)}<option value={e.id} disabled={!e.active}
									>{e.name_preview} · {e.id}</option
								>{/each}</select
						>
						<Button type="button" variant="outline" onclick={() => (employee = '')}
							>Clear assignee</Button
						>
						<p class="text-muted-foreground text-sm">
							The team and employee are independent. Saving replaces both fields together.
						</p>
					</fieldset>
					<div class="flex flex-wrap gap-2">
						{#if teamNext}<Button
								type="button"
								variant="outline"
								disabled={pending}
								onclick={() => more('team')}>More teams</Button
							>{/if}{#if employeeNext}<Button
								type="button"
								variant="outline"
								disabled={pending}
								onclick={() => more('employee')}>More employees</Button
							>{/if}
					</div>
					<Button type="submit" disabled={blocked}>Save assignment</Button>
				</form>
			{/if}
			{#if status}<div role="status" aria-live="polite" class="flex flex-col gap-2">
					<p>{status}</p>
					{#if operation}<p class="break-all font-mono text-xs">Operation {operation.id}</p>{/if}
				</div>{/if}
			{#if observed}<Alert.Root
					><Alert.Title>State observed at conflict</Alert.Title><Alert.Description
						>Revision {observed.revision}; team {observed.assigned_team_id ?? 'none'}; assignee {observed.assignee_id ??
							'none'}. This is the original receipt’s state.</Alert.Description
					></Alert.Root
				>{/if}
			{#if operation}<Button variant="outline" disabled={pending} onclick={reconcile}
					>Check original operation</Button
				>{/if}
			{#if outcome && ['completed', 'rejected', 'not_submitted'].includes(outcome.status)}<Button
					variant="outline"
					disabled={pending}
					onclick={newEdit}>Acknowledge result and reload for a new edit</Button
				>{/if}
			{#if task}<Button
					variant="outline"
					disabled={pending}
					onclick={() => task && showSource(task.snapshot)}>View verified public Markdown</Button
				>{/if}
			<section aria-label="Task history" class="flex flex-col gap-2">
				<h2 class="font-medium">History</h2>
				<p class="text-muted-foreground text-sm">
					Coverage: {coverage}. {coverage !== 'complete'
						? 'Earlier history may be unavailable or unverified.'
						: ''}
				</p>
				<ol class="flex flex-col gap-2">
					{#each history as entry (entry.revision)}<li>
							<Button variant="outline" disabled={pending} onclick={() => showSource(entry)}
								>Read event at revision {entry.revision}</Button
							>
						</li>{/each}
				</ol>
				{#if historyNext}<Button
						variant="outline"
						disabled={pending}
						onclick={() => more('history')}>More history</Button
					>{/if}
			</section>
			{#if source}<section aria-label="Verified public source">
					<h2 class="font-medium">Public source</h2>
					<p class="mt-2 text-sm" role="status">{sourceSummary}</p>
					<pre
						class="bg-muted mt-2 overflow-x-auto rounded-md p-3 text-xs whitespace-pre-wrap break-words">{source}</pre>
				</section>{/if}
		</div>
	</Sheet.Content>
</Sheet.Root>
