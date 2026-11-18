<!--
  @component
  Assign task (our-org-in-the-app §5): gives this member one open work item that has no one assigned. The list loads
  when the dialog opens; loading, error with Retry and "nothing to give" are real states. A refusal keeps the choice.
-->
<script lang="ts">
	import Loaded from '$lib/components/Loaded.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { toAppError } from '$lib/office/api';
	import { membersApi } from '../../api';
	import type { AssignableTask, MemberProfile } from '../../types';

	let {
		open = $bindable(false),
		organizationId,
		profile,
		onassigned,
		onCloseAutoFocus
	}: {
		open: boolean;
		organizationId: string;
		profile: MemberProfile;
		onassigned: (task: AssignableTask) => void;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	// A fresh list each time it opens: what is unassigned changes.
	// While closing it keeps the last list, so the content doesn't collapse during the fade.
	let last: Resource<AssignableTask[]> | undefined;
	const tasks = $derived.by(() => {
		if (open) last = new Resource(() => membersApi.assignableTasks(organizationId));
		return last;
	});
	let picked = $state<string | undefined>();
	let busy = $state(false);
	let refusal = $state<string | undefined>();
	const ids = $props.id();

	/** Closing forgets the choice and any refusal, so the next opening starts clean. */
	function changeOpen(next: boolean) {
		if (!next && busy) {
			open = true;
			return;
		}
		if (!next) {
			picked = undefined;
			refusal = undefined;
		}
	}

	async function assign(event: SubmitEvent) {
		event.preventDefault();
		const task = tasks?.data?.find((t) => t.ref === picked);
		if (busy) return;
		if (!task) {
			refusal = 'Choose a work item to assign.';
			return;
		}
		busy = true;
		refusal = undefined;
		try {
			await membersApi.assignTask(organizationId, profile.id, task.ref);
			busy = false;
			picked = undefined;
			open = false;
			onassigned(task);
		} catch (e) {
			refusal = toAppError(e).message;
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={changeOpen}>
	<DialogContent {onCloseAutoFocus} class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
		<form onsubmit={assign} class="grid gap-4">
			<DialogHeader>
				<DialogTitle text="Assign work to {profile.name}" />
				<DialogDescription>
					Open work items without an assignee. Assigning doesn't change the item's team or project.
				</DialogDescription>
			</DialogHeader>
			{#if tasks}
				<Loaded
					resource={tasks}
					label="work items"
					error="Couldn't load the work items"
					rows={3}
					isEmpty={(list) => list.length === 0}
					emptyTitle="Nothing to assign"
					emptyDescription="Every open work item already has someone. New work items without an assignee show here."
				>
					{#snippet children(list)}
						<fieldset aria-describedby={refusal ? `${ids}-error` : undefined}>
							<legend class="sr-only">Work item</legend>
							<ul role="list" class="divide-y divide-white/5 rounded-md ring-1 ring-white/10">
								{#each list as task (task.ref)}
									<li>
										<label
											class="flex cursor-pointer items-start gap-x-3 px-3 py-3 text-sm/6 has-focus-visible:outline-2 has-focus-visible:-outline-offset-2 has-focus-visible:outline-indigo-400 max-[639px]:min-h-11"
										>
											<input
												type="radio"
												name="task"
												value={task.ref}
												bind:group={picked}
												class="mt-1 size-4 accent-indigo-500 focus-visible:outline-none"
											/>
											<span class="min-w-0">
												<span class="font-mono text-gray-400">{task.ref}</span>
												<span class="font-semibold text-white [overflow-wrap:anywhere]"
													>{task.title}</span
												>
												{#if task.project}<span class="block text-xs/5 text-gray-400"
														>{task.project}</span
													>{/if}
											</span>
										</label>
									</li>
								{/each}
							</ul>
						</fieldset>
					{/snippet}
				</Loaded>
			{/if}
			{#if refusal}
				<p id="{ids}-error" class="text-sm/6 text-red-300 [overflow-wrap:anywhere]" role="alert">
					{refusal}
				</p>
			{/if}
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={busy}
					onclick={() => {
						open = false;
						changeOpen(false);
					}}>Cancel</Button
				>
				{#if tasks?.state === 'ready' && tasks.data?.length}
					<Button type="submit" class={mobileButton} aria-busy={busy}
						>{busy ? 'Assigning…' : 'Assign'}</Button
					>
				{/if}
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
