<!--
  @component
  Lifecycle modal dialogs for /work/[ref]:
  Close as… (Done, Cancelled, Duplicate with self-link check, Won't fix, Can't reproduce),
  Reopen (requires note), and Block (requires reason).
-->
<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { toAppError } from '$lib/proto';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		errorClass,
		inputClass,
		mobileButton,
		selectClass,
		textareaClass
	} from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import { normalizeRef } from '../../present';
	import type { CloseAs, TaskDetail } from '../../types';

	let {
		organizationId,
		task,
		closeOpen = $bindable(false),
		initialCloseKind = 'done',
		reopenOpen = $bindable(false),
		blockOpen = $bindable(false),
		onchanged
	}: {
		organizationId: string;
		task: TaskDetail;
		closeOpen: boolean;
		initialCloseKind?: CloseAs['as'];
		reopenOpen: boolean;
		blockOpen: boolean;
		onchanged: () => void;
	} = $props();

	const uid = $props.id();

	// Close as… state (FIX-7, FIX-8)
	let closeKind = $state<CloseAs['as']>('done');
	let duplicateRef = $state('');
	let closeReason = $state('');
	let closeBusy = $state(false);
	let closeError = $state<string | undefined>();

	// Reopen state (FIX-9)
	let reopenNote = $state('');
	let reopenBusy = $state(false);
	let reopenError = $state<string | undefined>();

	// Block state (TSK-3)
	let blockReason = $state('');
	let blockBusy = $state(false);
	let blockError = $state<string | undefined>();

	$effect(() => {
		if (closeOpen) {
			closeKind = initialCloseKind;
			duplicateRef = '';
			closeReason = '';
			closeError = undefined;
		}
	});

	$effect(() => {
		if (reopenOpen) {
			reopenNote = '';
			reopenError = undefined;
		}
	});

	$effect(() => {
		if (blockOpen) {
			blockReason = task.blockedReason ?? '';
			blockError = undefined;
		}
	});

	async function submitClose(event: SubmitEvent) {
		event.preventDefault();
		if (closeBusy) return;
		closeError = undefined;

		let payload: CloseAs;
		if (closeKind === 'duplicate') {
			const target = normalizeRef(duplicateRef);
			if (!target) {
				closeError = 'Enter the original work item reference.';
				return;
			}
			if (target === normalizeRef(task.ref)) {
				closeError = "A work item can't be marked as a duplicate of itself.";
				return;
			}
			payload = { as: 'duplicate', originalRef: target };
		} else if (closeKind === 'wont_fix' || closeKind === 'cant_reproduce') {
			const reason = closeReason.trim();
			if (!reason) {
				closeError = 'Give a reason for closing this bug.';
				return;
			}
			payload = { as: closeKind, reason };
		} else if (closeKind === 'cancelled') {
			payload = { as: 'cancelled', reason: closeReason.trim() || null };
		} else {
			payload = { as: 'done' };
		}

		closeBusy = true;
		try {
			await workItemsApi.close(organizationId, task.id, {
				...payload,
				expectedRevision: task.revision
			});
			closeOpen = false;
			toast.success(`Closed ${task.ref}.`);
			onchanged();
		} catch (e) {
			closeError = toAppError(e).message;
		} finally {
			closeBusy = false;
		}
	}

	async function submitReopen(event: SubmitEvent) {
		event.preventDefault();
		if (reopenBusy) return;
		const note = reopenNote.trim();
		if (!note) {
			reopenError = 'Add a note explaining why this item is being reopened.';
			return;
		}
		reopenBusy = true;
		reopenError = undefined;
		try {
			await workItemsApi.reopen(organizationId, task.id, {
				note,
				expectedRevision: task.revision
			});
			reopenOpen = false;
			toast.success(`Reopened ${task.ref}.`);
			onchanged();
		} catch (e) {
			reopenError = toAppError(e).message;
		} finally {
			reopenBusy = false;
		}
	}

	async function submitBlock(event: SubmitEvent) {
		event.preventDefault();
		if (blockBusy) return;
		const reason = blockReason.trim();
		if (!reason) {
			blockError = 'Explain what is blocking this work.';
			return;
		}
		blockBusy = true;
		blockError = undefined;
		try {
			await workItemsApi.setStatus(organizationId, task.id, {
				status: 'blocked',
				blockedReason: reason,
				expectedRevision: task.revision
			});
			blockOpen = false;
			toast.success(`Marked ${task.ref} as Blocked.`);
			onchanged();
		} catch (e) {
			blockError = toAppError(e).message;
		} finally {
			blockBusy = false;
		}
	}
</script>

<!-- Close as… modal -->
<Dialog bind:open={closeOpen} onOpenChange={(n) => !n && closeBusy && (closeOpen = true)}>
	<DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md">
		<DialogHeader>
			<DialogTitle text="Close {task.ref}" />
			<DialogDescription>
				Choose how {task.ref} is closed. Done and Cancelled are distinct and keep full history.
			</DialogDescription>
		</DialogHeader>
		<form class="flex flex-col gap-4" onsubmit={submitClose} novalidate>
			<div>
				<Label for="{uid}-close-as" class="block text-sm/6 font-medium text-fg">Close as</Label>
				<select
					id="{uid}-close-as"
					bind:value={closeKind}
					disabled={closeBusy}
					class="mt-1.5 {selectClass}"
				>
					<option value="done">Done</option>
					<option value="cancelled">Cancelled</option>
					{#if task.type === 'bug'}
						<option value="duplicate">Duplicate</option>
						<option value="wont_fix">Won't fix</option>
						<option value="cant_reproduce">Can't reproduce</option>
					{/if}
				</select>
			</div>

			{#if closeKind === 'duplicate'}
				<div>
					<Label for="{uid}-dup-ref" class="block text-sm/6 font-medium text-fg">
						Original bug reference
					</Label>
					<Input
						id="{uid}-dup-ref"
						bind:value={duplicateRef}
						placeholder="T-A42"
						disabled={closeBusy}
						class="mt-1.5 {inputClass}"
					/>
				</div>
			{:else if closeKind === 'wont_fix' || closeKind === 'cant_reproduce' || closeKind === 'cancelled'}
				<div>
					<Label for="{uid}-close-reason" class="block text-sm/6 font-medium text-fg">
						{closeKind === 'cancelled' ? 'Reason (optional)' : 'Reason (required)'}
					</Label>
					<textarea
						id="{uid}-close-reason"
						rows={3}
						bind:value={closeReason}
						disabled={closeBusy}
						class="mt-1.5 {textareaClass}"></textarea>
				</div>
			{/if}

			{#if closeError}
				<p class={errorClass} role="alert">{closeError}</p>
			{/if}

			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={closeBusy}
					onclick={() => (closeOpen = false)}>Cancel</Button
				>
				<Button type="submit" class={mobileButton} disabled={closeBusy}>
					{closeBusy ? 'Closing…' : 'Close item'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>

<!-- Reopen modal -->
<Dialog bind:open={reopenOpen} onOpenChange={(n) => !n && reopenBusy && (reopenOpen = true)}>
	<DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md">
		<DialogHeader>
			<DialogTitle text="Reopen {task.ref}" />
			<DialogDescription>
				Reopening requires a note and preserves all original history.
			</DialogDescription>
		</DialogHeader>
		<form class="flex flex-col gap-4" onsubmit={submitReopen} novalidate>
			<div>
				<Label for="{uid}-reopen-note" class="block text-sm/6 font-medium text-fg">Note</Label>
				<textarea
					id="{uid}-reopen-note"
					rows={3}
					bind:value={reopenNote}
					disabled={reopenBusy}
					class="mt-1.5 {textareaClass}"></textarea>
				{#if reopenError}
					<p class={errorClass} role="alert">{reopenError}</p>
				{/if}
			</div>
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={reopenBusy}
					onclick={() => (reopenOpen = false)}>Cancel</Button
				>
				<Button type="submit" class={mobileButton} disabled={reopenBusy}>
					{reopenBusy ? 'Reopening…' : 'Reopen'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>

<!-- Block modal -->
<Dialog bind:open={blockOpen} onOpenChange={(n) => !n && blockBusy && (blockOpen = true)}>
	<DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md">
		<DialogHeader>
			<DialogTitle text="Block {task.ref}" />
			<DialogDescription>Explain what is blocking this work.</DialogDescription>
		</DialogHeader>
		<form class="flex flex-col gap-4" onsubmit={submitBlock} novalidate>
			<div>
				<Label for="{uid}-block-note" class="block text-sm/6 font-medium text-fg">Reason</Label>
				<textarea
					id="{uid}-block-note"
					rows={3}
					bind:value={blockReason}
					disabled={blockBusy}
					class="mt-1.5 {textareaClass}"></textarea>
				{#if blockError}
					<p class={errorClass} role="alert">{blockError}</p>
				{/if}
			</div>
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={blockBusy}
					onclick={() => (blockOpen = false)}>Cancel</Button
				>
				<Button type="submit" class={mobileButton} disabled={blockBusy}>
					{blockBusy ? 'Saving…' : 'Mark as blocked'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
