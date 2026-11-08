<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/office/api';
	import { newId } from '$lib/org/commands';
	import type { Member, OrgSnapshot, Priority } from '$lib/proto';
	import { WORK_LIMITS } from '$lib/proto';
	import type { WorkDraft } from '$lib/work';
	import { assigneeOptions } from '$lib/work/people';
	import { PRIORITIES } from '$lib/work/tasks';

	/** New task (TSK-5): a title is enough; the priority is Normal unless changed. */
	let {
		open = $bindable(false),
		projectId,
		projectName,
		snapshot,
		send,
		ondone
	}: {
		open: boolean;
		projectId: string;
		projectName: string;
		snapshot: OrgSnapshot;
		send: (draft: WorkDraft) => Promise<unknown>;
		ondone: (title: string) => void;
	} = $props();

	let title = $state('');
	let description = $state('');
	let priority = $state<Priority>('normal');
	let assignee = $state('');
	let error = $state<string | undefined>();
	let busy = $state(false);
	let touched = $state(false);

	$effect(() => {
		if (open) {
			untrack(() => {
				title = '';
				description = '';
				priority = 'normal';
				assignee = '';
				error = undefined;
				busy = false;
				touched = false;
			});
		}
	});

	const people: { member: Member; label: string }[] = $derived(assigneeOptions(snapshot));
	const titleError = $derived(touched && title.trim() === '' ? 'Give the task a title.' : undefined);

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		touched = true;
		if (busy || title.trim() === '') return;
		busy = true;
		error = undefined;
		try {
			await send({
				method: 'task.create',
				item_id: newId(),
				project_id: projectId,
				title: title.trim(),
				description: description.trim() || undefined,
				priority,
				assignee_id: assignee || undefined
			});
			open = false;
			ondone(title.trim());
		} catch (e) {
			error = toAppError(e).message;
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent class="sm:max-w-lg">
		<DialogHeader>
			<DialogTitle>New task</DialogTitle>
			<DialogDescription>In {projectName}. It starts in To do.</DialogDescription>
		</DialogHeader>
		<form class="space-y-4" onsubmit={submit} novalidate>
			<div class="space-y-2">
				<Label for="new-task-title">Title</Label>
				<Input id="new-task-title" bind:value={title} maxlength={WORK_LIMITS.title * 2} disabled={busy} autocomplete="off" aria-invalid={titleError ? true : undefined} />
				{#if titleError}<p class="text-sm/6 text-red-400" role="alert">{titleError}</p>{/if}
			</div>
			<div class="space-y-2">
				<Label for="new-task-description">Description <span class="font-normal text-gray-400">(optional)</span></Label>
				<textarea id="new-task-description" bind:value={description} rows="3" disabled={busy} class="w-full rounded-md bg-gray-800 px-3 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400"></textarea>
			</div>
			<div class="grid gap-4 sm:grid-cols-2">
				<div class="space-y-2">
					<Label for="new-task-priority">Priority</Label>
					<select id="new-task-priority" bind:value={priority} disabled={busy} class="w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
						{#each PRIORITIES as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
					</select>
				</div>
				<div class="space-y-2">
					<Label for="new-task-assignee">Assignee <span class="font-normal text-gray-400">(optional)</span></Label>
					<select id="new-task-assignee" bind:value={assignee} disabled={busy} class="w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
						<option value="">Unassigned</option>
						{#each people as p (p.member.id)}<option value={p.member.id}>{p.label}</option>{/each}
					</select>
				</div>
			</div>
			{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
			<DialogFooter>
				<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={busy}>Create task</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
