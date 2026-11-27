<!--
  @component
  Mark as fixed & Verify fix dialogs for BugSection (FIX-3..5).
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
	import type { FixHow, TaskDetail } from '../../types';

	let {
		organizationId,
		task,
		stale = false,
		markFixedOpen = $bindable(false),
		verifyOpen = $bindable(false),
		onchanged
	}: {
		organizationId: string;
		task: TaskDetail;
		stale?: boolean;
		markFixedOpen: boolean;
		verifyOpen: boolean;
		onchanged: () => void;
	} = $props();

	const uid = $props.id();

	let fixKind = $state<'task' | 'commit' | 'note'>('note');
	let fixTaskRef = $state('');
	let fixCommit = $state('');
	let fixNote = $state('');
	let fixBusy = $state(false);
	let fixError = $state<string | undefined>();

	let verifyResult = $state<'fixed' | 'still_happens'>('fixed');
	let verifyNote = $state('');
	let verifyBusy = $state(false);
	let verifyError = $state<string | undefined>();

	export function resetMarkFixedError() {
		fixError = undefined;
	}

	export function resetVerifyError() {
		verifyError = undefined;
	}

	function resolveFixHow(): FixHow | string {
		if (fixKind === 'task') {
			return fixTaskRef.trim()
				? { kind: 'task', taskRef: fixTaskRef.trim().toUpperCase() }
				: 'Enter the fix task reference (for example T-A40).';
		}
		if (fixKind === 'commit') {
			return fixCommit.trim()
				? { kind: 'commit', commit: fixCommit.trim() }
				: 'Enter the commit SHA.';
		}
		return fixNote.trim() ? { kind: 'note' } : 'Describe what fixed the bug.';
	}

	async function submitMarkFixed(event: SubmitEvent) {
		event.preventDefault();
		if (fixBusy || stale) return;
		fixError = undefined;
		const how = resolveFixHow();
		if (typeof how === 'string') {
			fixError = how;
			return;
		}
		fixBusy = true;
		try {
			await workItemsApi.markFixed(organizationId, task.id, {
				how,
				note: fixNote.trim(),
				expectedRevision: task.revision
			});
			markFixedOpen = false;
			fixNote = '';
			fixTaskRef = '';
			fixCommit = '';
			toast.success(`Recorded fix evidence on ${task.ref}.`);
			onchanged();
		} catch (e) {
			fixError = toAppError(e).message;
		} finally {
			fixBusy = false;
		}
	}

	async function submitVerify(event: SubmitEvent) {
		event.preventDefault();
		if (verifyBusy || stale) return;
		if (verifyResult === 'still_happens' && !verifyNote.trim()) {
			verifyError = 'Explain what still happens so the team can follow up.';
			return;
		}
		verifyBusy = true;
		verifyError = undefined;
		try {
			await workItemsApi.verify(organizationId, task.id, {
				result: verifyResult,
				note: verifyNote.trim(),
				expectedRevision: task.revision
			});
			verifyOpen = false;
			verifyNote = '';
			toast.success(
				verifyResult === 'fixed'
					? `Verified ${task.ref} as fixed.`
					: `Reopened ${task.ref}: still happens.`
			);
			onchanged();
		} catch (e) {
			verifyError = toAppError(e).message;
		} finally {
			verifyBusy = false;
		}
	}
</script>

<!-- Mark as fixed modal -->
<Dialog bind:open={markFixedOpen} onOpenChange={(n) => !n && fixBusy && (markFixedOpen = true)}>
	<DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md">
		<DialogHeader>
			<DialogTitle text="Mark {task.ref} as fixed" />
			<DialogDescription>
				Records fix evidence with your actor and time in history. Move to Done separately if ready.
			</DialogDescription>
		</DialogHeader>
		<form class="flex flex-col gap-4" onsubmit={submitMarkFixed} novalidate>
			<div>
				<Label for="{uid}-fix-how" class="block text-sm/6 font-medium text-fg">What fixed it</Label>
				<select
					id="{uid}-fix-how"
					bind:value={fixKind}
					disabled={fixBusy}
					class="mt-1.5 {selectClass}"
				>
					<option value="note">Note</option>
					<option value="task">Linked fix task</option>
					<option value="commit">Commit</option>
				</select>
			</div>

			{#if fixKind === 'task'}
				<div>
					<Label for="{uid}-fix-ref" class="block text-sm/6 font-medium text-fg">
						Fix task reference
					</Label>
					<Input
						id="{uid}-fix-ref"
						bind:value={fixTaskRef}
						placeholder="T-A40"
						disabled={fixBusy}
						class="mt-1.5 {inputClass}"
					/>
				</div>
			{:else if fixKind === 'commit'}
				<div>
					<Label for="{uid}-fix-commit" class="block text-sm/6 font-medium text-fg">
						Commit SHA
					</Label>
					<Input
						id="{uid}-fix-commit"
						bind:value={fixCommit}
						placeholder="a1b2c3d"
						disabled={fixBusy}
						class="mt-1.5 {inputClass}"
					/>
				</div>
			{/if}

			<div>
				<Label for="{uid}-fix-note" class="block text-sm/6 font-medium text-fg">
					{fixKind === 'note' ? 'Explanation' : 'Note (optional)'}
				</Label>
				<textarea
					id="{uid}-fix-note"
					rows={3}
					bind:value={fixNote}
					disabled={fixBusy}
					class="mt-1.5 {textareaClass}"></textarea>
			</div>

			{#if fixError}
				<p class={errorClass} role="alert">{fixError}</p>
			{/if}

			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={fixBusy}
					onclick={() => (markFixedOpen = false)}>Cancel</Button
				>
				<Button type="submit" class={mobileButton} disabled={fixBusy}>
					{fixBusy ? 'Saving…' : 'Mark as fixed'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>

<!-- Verify fix modal (FIX-4, FIX-5) -->
<Dialog bind:open={verifyOpen} onOpenChange={(n) => !n && verifyBusy && (verifyOpen = true)}>
	<DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md">
		<DialogHeader>
			<DialogTitle text="Verify fix for {task.ref}" />
			<DialogDescription>
				Choose whether the fix resolves the bug. "Still happens" requires a note, reopens the bug,
				and increments the reopened count once.
			</DialogDescription>
		</DialogHeader>
		<form class="flex flex-col gap-4" onsubmit={submitVerify} novalidate>
			<div class="flex gap-2" role="radiogroup" aria-label="Verification outcome">
				<label
					class="flex flex-1 cursor-pointer items-center justify-center rounded-md px-3 py-2 text-sm/6 ring-1 ring-inset max-[639px]:min-h-11 {verifyResult ===
					'fixed'
						? 'bg-success-emphasis/10 font-semibold text-success-text ring-success-emphasis/30'
						: 'bg-surface-raised text-fg-secondary ring-line'}"
				>
					<input
						type="radio"
						name="{uid}-verify"
						value="fixed"
						checked={verifyResult === 'fixed'}
						onchange={() => (verifyResult = 'fixed')}
						class="sr-only"
					/>
					Fixed
				</label>
				<label
					class="flex flex-1 cursor-pointer items-center justify-center rounded-md px-3 py-2 text-sm/6 ring-1 ring-inset max-[639px]:min-h-11 {verifyResult ===
					'still_happens'
						? 'bg-danger-emphasis/10 font-semibold text-danger-text ring-danger-emphasis/30'
						: 'bg-surface-raised text-fg-secondary ring-line'}"
				>
					<input
						type="radio"
						name="{uid}-verify"
						value="still_happens"
						checked={verifyResult === 'still_happens'}
						onchange={() => (verifyResult = 'still_happens')}
						class="sr-only"
					/>
					Still happens
				</label>
			</div>

			<div>
				<Label for="{uid}-verify-note" class="block text-sm/6 font-medium text-fg">
					{verifyResult === 'still_happens' ? 'What still happens (required)' : 'Note (optional)'}
				</Label>
				<textarea
					id="{uid}-verify-note"
					rows={3}
					bind:value={verifyNote}
					disabled={verifyBusy}
					aria-invalid={verifyError ? 'true' : undefined}
					class="mt-1.5 {textareaClass}"></textarea>
				{#if verifyError}
					<p class={errorClass} role="alert">{verifyError}</p>
				{/if}
			</div>

			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={verifyBusy}
					onclick={() => (verifyOpen = false)}>Cancel</Button
				>
				<Button type="submit" class={mobileButton} disabled={verifyBusy}>
					{verifyBusy ? 'Saving…' : verifyResult === 'fixed' ? 'Confirm fixed' : 'Reopen bug'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
