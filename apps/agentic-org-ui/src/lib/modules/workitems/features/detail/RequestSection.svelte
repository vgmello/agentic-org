<!--
  @component
  Request-specific sections on /work/[ref] (feature-requests-ux §3; RQS-9, RQS-12):
  Original request text, routing metadata, recorded scoping outcome (feature, docs, draft tasks, estimate),
  editable Approve and create preview dialog, Ask for changes, Decline (with reason), and Merge into another request.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { toast } from 'svelte-sonner';
	import { newId } from '$lib/modules/organization/commands';
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
		dangerButton,
		errorClass,
		inputClass,
		mobileButton,
		textareaClass
	} from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import { PRIORITY_WORD, STATUS_WORD, linkText, taskHref, when } from '../../present';
	import type { DraftTask, RequestDetails, TaskDetail } from '../../types';

	let {
		organizationId,
		task,
		request,
		stale = false,
		onchanged
	}: {
		organizationId: string;
		task: TaskDetail;
		request: RequestDetails;
		stale?: boolean;
		onchanged: () => void;
	} = $props();

	const uid = $props.id();

	// Approve and create preview modal state (RQS-9)
	let approveOpen = $state(false);
	let editableDrafts = $state<DraftTask[]>([]);
	let approveBusy = $state(false);
	let approveError = $state<string | undefined>();
	let approveOpId = $state('');

	// Ask for changes / Decline / Merge modals (RQS-12)
	let changesOpen = $state(false);
	let changesComment = $state('');
	let declineOpen = $state(false);
	let declineReason = $state('');
	let mergeOpen = $state(false);
	let mergeRef = $state('');
	let actionBusy = $state(false);
	let actionError = $state<string | undefined>();

	function formatEstimateCents(min: number | null, max: number | null): string {
		if (min === null && max === null) return 'Cost unknown';
		const fmt = (c: number) => `$${(c / 100).toFixed(c % 100 === 0 ? 0 : 2)}`;
		if (min !== null && max !== null) return min === max ? fmt(min) : `${fmt(min)}–${fmt(max)}`;
		return min !== null ? `from ${fmt(min)}` : `up to ${fmt(max!)}`;
	}

	function openApprove() {
		editableDrafts = structuredClone(request.outcome?.draftTasks ?? []);
		approveError = undefined;
		approveOpId = newId();
		approveOpen = true;
	}

	// Executive Spotlight/Inbox "Approve request" routes to `/work/[ref]?approve=1` so the owner
	// reviews and confirms the editable draft tasks before creating work (RQS-9).
	const wantsApprovePreview = $derived(page.url.searchParams.get('approve') === '1');
	$effect(() => {
		if (!wantsApprovePreview) return;
		if (
			!stale &&
			task.status !== 'cancelled' &&
			task.can.approve &&
			request.outcome &&
			!request.approved
		) {
			openApprove();
		}
		const nextUrl = new URL(page.url);
		nextUrl.searchParams.delete('approve');
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- preserving current already-resolved URL + hash
		void goto(nextUrl, { replaceState: true, noScroll: true, keepFocus: true });
	});

	function removeDraft(draftId: string) {
		editableDrafts = editableDrafts.filter((d) => d.draftId !== draftId);
	}

	async function submitApprove(event: SubmitEvent) {
		event.preventDefault();
		if (approveBusy || stale) return;
		const cleaned = editableDrafts.map((d) => ({ ...d, title: d.title.trim() }));
		if (cleaned.some((d) => !d.title)) {
			approveError = 'Every draft task must have a title (or remove the empty draft).';
			return;
		}
		approveBusy = true;
		approveError = undefined;
		try {
			const res = await workItemsApi.approve(organizationId, task.id, {
				drafts: cleaned,
				operationId: approveOpId || newId(),
				expectedRevision: task.revision
			});
			approveOpen = false;
			toast.success(
				`Approved ${task.ref} and created ${res.created.length} ${res.created.length === 1 ? 'task' : 'tasks'}.`
			);
			onchanged();
		} catch (e) {
			approveError = toAppError(e).message;
		} finally {
			approveBusy = false;
		}
	}

	async function submitAskChanges(event: SubmitEvent) {
		event.preventDefault();
		if (actionBusy || stale) return;
		if (!changesComment.trim()) {
			actionError = 'Explain what needs to change.';
			return;
		}
		actionBusy = true;
		actionError = undefined;
		try {
			await workItemsApi.askForChanges(organizationId, task.id, {
				comment: changesComment.trim(),
				expectedRevision: task.revision
			});
			changesOpen = false;
			changesComment = '';
			toast.success(`Returned ${task.ref} to the scoper.`);
			onchanged();
		} catch (e) {
			actionError = toAppError(e).message;
		} finally {
			actionBusy = false;
		}
	}

	async function submitDecline(event: SubmitEvent) {
		event.preventDefault();
		if (actionBusy || stale) return;
		if (!declineReason.trim()) {
			actionError = 'Give a reason for declining this request.';
			return;
		}
		actionBusy = true;
		actionError = undefined;
		try {
			await workItemsApi.decline(organizationId, task.id, {
				reason: declineReason.trim(),
				expectedRevision: task.revision
			});
			declineOpen = false;
			declineReason = '';
			toast.success(`Declined ${task.ref}.`);
			onchanged();
		} catch (e) {
			actionError = toAppError(e).message;
		} finally {
			actionBusy = false;
		}
	}

	async function submitMerge(event: SubmitEvent) {
		event.preventDefault();
		if (actionBusy || stale) return;
		const target = mergeRef.trim().toUpperCase();
		if (!target) {
			actionError = 'Enter the target request reference (for example T-A43).';
			return;
		}
		actionBusy = true;
		actionError = undefined;
		try {
			await workItemsApi.merge(organizationId, task.id, {
				intoRef: target,
				expectedRevision: task.revision
			});
			mergeOpen = false;
			mergeRef = '';
			toast.success(`Merged ${task.ref} into ${target}.`);
			onchanged();
		} catch (e) {
			actionError = toAppError(e).message;
		} finally {
			actionBusy = false;
		}
	}
</script>

<!-- 1. Original request text -->
<section
	aria-labelledby="{uid}-req-heading"
	class="flex flex-col gap-3 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
	data-testid="request-original-section"
>
	<div class="flex flex-wrap items-baseline justify-between gap-2">
		<h3 id="{uid}-req-heading" class="text-sm/6 font-semibold text-fg">Original request</h3>
		{#if request.routingSource}
			<span class="text-xs/5 text-fg-muted">
				Routed by {request.routingSource}{#if request.routeReason}: {request.routeReason}{/if}
			</span>
		{/if}
	</div>

	<p class="whitespace-pre-wrap text-sm/6 text-fg-body [overflow-wrap:anywhere]">
		{request.text}
	</p>

	{#if request.declineReason}
		<p
			class="border-t border-line-subtle pt-2 text-xs/5 text-danger-text"
			data-testid="decline-reason"
		>
			Declined: {request.declineReason}
		</p>
	{/if}

	{#if request.mergedInto}
		<p
			class="border-t border-line-subtle pt-2 text-xs/5 text-fg-secondary"
			data-testid="merged-into"
		>
			Merged into
			<a
				href={resolve(taskHref(request.mergedInto.ref) as '/')}
				class="ml-1 font-medium text-link hover:underline"
			>
				{linkText(request.mergedInto)}
			</a>
		</p>
	{/if}

	{#if request.changesAsked}
		<div
			class="rounded-md bg-surface p-3 text-xs/5 text-fg-secondary outline-1 -outline-offset-1 outline-line-subtle"
			data-testid="changes-asked"
		>
			<p class="font-medium text-fg">
				Changes requested by {request.changesAsked.by.name} · {when(request.changesAsked.at)}
			</p>
			<p class="mt-1 whitespace-pre-wrap">{request.changesAsked.comment}</p>
		</div>
	{/if}
</section>

<!-- 2. Scoping outcome & controls (RQS-9, RQS-12) -->
<section
	aria-labelledby="{uid}-outcome-heading"
	class="flex flex-col gap-4 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
	data-testid="request-outcome-section"
>
	<div class="flex flex-wrap items-center justify-between gap-2">
		<h3 id="{uid}-outcome-heading" class="text-sm/6 font-semibold text-fg">Scoping outcome</h3>
		{#if !stale && task.status !== 'cancelled'}
			<div class="flex flex-wrap gap-2">
				{#if request.outcome && !request.approved}
					<Button
						type="button"
						size="sm"
						class={mobileButton}
						disabled={!task.can.approve}
						title={task.can.approveReason}
						onclick={openApprove}
					>
						Approve and create…
					</Button>
					<Button
						type="button"
						variant="outline"
						size="sm"
						class={mobileButton}
						onclick={() => ((actionError = undefined), (changesOpen = true))}
					>
						Ask for changes…
					</Button>
				{/if}
				<Button
					type="button"
					variant="outline"
					size="sm"
					class={mobileButton}
					onclick={() => ((actionError = undefined), (mergeOpen = true))}
				>
					Merge into…
				</Button>
				<Button
					type="button"
					variant="outline"
					size="sm"
					class={mobileButton}
					onclick={() => ((actionError = undefined), (declineOpen = true))}
				>
					Decline…
				</Button>
			</div>
		{/if}
	</div>

	{#if request.outcome && !request.approved && !task.can.approve && task.can.approveReason}
		<p
			class="rounded-md bg-surface px-3 py-2 text-xs/5 text-fg-secondary ring-1 ring-line-subtle"
			data-testid="approve-disabled-reason"
		>
			{task.can.approveReason}
		</p>
	{/if}

	{#if !request.outcome}
		<p class="text-sm/6 text-fg-muted">
			Not scoped yet. The assigned team or employee records the proposed feature, documents, draft
			tasks and estimate here.
		</p>
	{:else}
		{@const outcome = request.outcome}
		<p class="text-xs/5 text-fg-muted">
			Scoped by {outcome.recorded.by.name} ({outcome.recorded.by.kind}) · {when(
				outcome.recorded.at
			)}
			{#if request.approved}
				· Approved by {request.approved.by.name} · {when(request.approved.at)}
			{/if}
		</p>

		{#if outcome.feature}
			<div class="flex flex-col gap-0.5 text-sm/6">
				<span class="text-xs/5 font-medium text-fg-muted">
					{outcome.feature.type === 'new' ? 'New feature' : 'Extends feature'}
				</span>
				<p class="text-fg">
					<span class="font-mono text-xs/5 text-fg-muted">{outcome.feature.ref}</span>
					<span class="ml-1.5 font-medium">{outcome.feature.title}</span>
				</p>
			</div>
		{/if}

		{#if outcome.docs.length > 0}
			<div class="flex flex-col gap-1">
				<span class="text-xs/5 font-medium text-fg-muted">Supporting documents</span>
				<ul role="list" class="flex flex-col gap-1 text-sm/6 text-fg-secondary">
					{#each outcome.docs as doc (doc.docKey)}
						<li>
							<span class="font-mono text-xs/5 text-fg-muted">{doc.docKey}</span>
							<span class="ml-1.5">{doc.title}</span>
						</li>
					{/each}
				</ul>
			</div>
		{/if}

		{#if outcome.estimate}
			<div class="flex flex-wrap gap-x-4 gap-y-1 text-xs/5 text-fg-secondary">
				<span>Estimate: <strong class="text-fg">{outcome.estimate.effort}</strong></span>
				<span>
					{formatEstimateCents(outcome.estimate.costUsdCentsMin, outcome.estimate.costUsdCentsMax)}
				</span>
			</div>
		{/if}

		{#if request.created.length > 0}
			<div
				class="flex flex-col gap-1.5 border-t border-line-subtle pt-3"
				data-testid="created-tasks"
			>
				<span class="text-xs/5 font-medium text-fg-muted">Created tasks</span>
				<ul role="list" class="flex flex-col gap-1.5 text-sm/6">
					{#each request.created as created (created.id)}
						<li class="flex items-center justify-between gap-2">
							<a
								href={resolve(taskHref(created.ref) as '/')}
								class="font-medium text-link hover:underline"
							>
								{linkText(created)}
							</a>
							<span class="text-xs/5 text-fg-muted">{STATUS_WORD[created.status]}</span>
						</li>
					{/each}
				</ul>
			</div>
		{:else if outcome.draftTasks.length > 0}
			<div class="flex flex-col gap-1.5">
				<span class="text-xs/5 font-medium text-fg-muted">
					Draft tasks ({outcome.draftTasks.length})
				</span>
				<ul
					role="list"
					class="divide-y divide-line-subtle rounded-lg bg-surface outline-1 -outline-offset-1 outline-line-subtle"
				>
					{#each outcome.draftTasks as d (d.draftId)}
						<li class="flex flex-wrap items-baseline justify-between gap-2 px-3 py-2 text-sm/6">
							<span class="font-medium text-fg [overflow-wrap:anywhere]">{d.title}</span>
							<span class="text-xs/5 text-fg-muted">
								{d.team?.name ?? task.team.name}
								{#if d.assignee}· {d.assignee.name}{/if}
								{#if d.priority && d.priority !== 'normal'}· {PRIORITY_WORD[d.priority]}{/if}
							</span>
						</li>
					{/each}
				</ul>
			</div>
		{/if}
	{/if}
</section>

<!-- Approve and create preview modal (RQS-9) -->
<Dialog bind:open={approveOpen} onOpenChange={(n) => !n && approveBusy && (approveOpen = true)}>
	<DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-lg">
		<DialogHeader>
			<DialogTitle text="Approve and create from {task.ref}" />
			<DialogDescription>
				Review, edit or remove proposed draft tasks. Approving creates these exact records linked to
				{task.ref} in one recoverable operation.
			</DialogDescription>
		</DialogHeader>
		<form class="flex flex-col gap-4" onsubmit={submitApprove} novalidate>
			{#if request.outcome?.feature}
				<p
					class="rounded-md bg-surface-raised px-3 py-2 text-xs/5 text-fg-secondary ring-1 ring-line"
				>
					Feature: <strong class="text-fg">{request.outcome.feature.ref}</strong>
					{request.outcome.feature.title}
				</p>
			{/if}

			{#if editableDrafts.length === 0}
				<p class="text-sm/6 text-fg-muted">No draft tasks left to create.</p>
			{:else}
				<ul role="list" class="flex flex-col gap-2.5" data-testid="approve-draft-list">
					{#each editableDrafts as draft, i (draft.draftId)}
						<li class="flex items-center gap-2">
							<label for="{uid}-draft-{i}" class="sr-only">Draft task {i + 1} title</label>
							<Input
								id="{uid}-draft-{i}"
								bind:value={draft.title}
								disabled={approveBusy}
								class="flex-1 {inputClass}"
							/>
							<Button
								type="button"
								variant="outline"
								size="sm"
								class={mobileButton}
								disabled={approveBusy}
								onclick={() => removeDraft(draft.draftId)}>Remove</Button
							>
						</li>
					{/each}
				</ul>
			{/if}

			{#if approveError}
				<p class={errorClass} role="alert">{approveError}</p>
			{/if}

			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={approveBusy}
					onclick={() => (approveOpen = false)}>Cancel</Button
				>
				<Button type="submit" class={mobileButton} disabled={approveBusy}>
					{approveBusy ? 'Creating…' : `Approve and create (${editableDrafts.length})`}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>

<!-- Ask for changes modal -->
<Dialog bind:open={changesOpen} onOpenChange={(n) => !n && actionBusy && (changesOpen = true)}>
	<DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md">
		<DialogHeader>
			<DialogTitle text="Ask for changes on {task.ref}" />
			<DialogDescription
				>Return the scoping outcome to the scoper with your comment.</DialogDescription
			>
		</DialogHeader>
		<form class="flex flex-col gap-4" onsubmit={submitAskChanges} novalidate>
			<div>
				<Label for="{uid}-changes-comment" class="block text-sm/6 font-medium text-fg">
					What needs to change
				</Label>
				<textarea
					id="{uid}-changes-comment"
					rows={3}
					bind:value={changesComment}
					disabled={actionBusy}
					class="mt-1.5 {textareaClass}"></textarea>
				{#if actionError}
					<p class={errorClass} role="alert">{actionError}</p>
				{/if}
			</div>
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={actionBusy}
					onclick={() => (changesOpen = false)}>Cancel</Button
				>
				<Button type="submit" class={mobileButton} disabled={actionBusy}>
					{actionBusy ? 'Sending…' : 'Ask for changes'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>

<!-- Decline request modal -->
<Dialog bind:open={declineOpen} onOpenChange={(n) => !n && actionBusy && (declineOpen = true)}>
	<DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md">
		<DialogHeader>
			<DialogTitle text="Decline {task.ref}?" />
			<DialogDescription>
				Declining closes this request as Cancelled and records your reason in history.
			</DialogDescription>
		</DialogHeader>
		<form class="flex flex-col gap-4" onsubmit={submitDecline} novalidate>
			<div>
				<Label for="{uid}-decline-reason" class="block text-sm/6 font-medium text-fg">Reason</Label>
				<textarea
					id="{uid}-decline-reason"
					rows={3}
					bind:value={declineReason}
					disabled={actionBusy}
					class="mt-1.5 {textareaClass}"></textarea>
				{#if actionError}
					<p class={errorClass} role="alert">{actionError}</p>
				{/if}
			</div>
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={actionBusy}
					onclick={() => (declineOpen = false)}>Cancel</Button
				>
				<Button type="submit" class="{mobileButton} {dangerButton}" disabled={actionBusy}>
					{actionBusy ? 'Declining…' : 'Decline request'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>

<!-- Merge into another open request modal -->
<Dialog bind:open={mergeOpen} onOpenChange={(n) => !n && actionBusy && (mergeOpen = true)}>
	<DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md">
		<DialogHeader>
			<DialogTitle text="Merge {task.ref} into another request" />
			<DialogDescription>
				Closes {task.ref} as Cancelled and links it to the chosen open request.
			</DialogDescription>
		</DialogHeader>
		<form class="flex flex-col gap-4" onsubmit={submitMerge} novalidate>
			<div>
				<Label for="{uid}-merge-ref" class="block text-sm/6 font-medium text-fg">
					Target request reference
				</Label>
				<Input
					id="{uid}-merge-ref"
					bind:value={mergeRef}
					placeholder="T-B17"
					disabled={actionBusy}
					class="mt-1.5 {inputClass}"
				/>
				{#if actionError}
					<p class={errorClass} role="alert">{actionError}</p>
				{/if}
			</div>
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={actionBusy}
					onclick={() => (mergeOpen = false)}>Cancel</Button
				>
				<Button type="submit" class={mobileButton} disabled={actionBusy}>
					{actionBusy ? 'Merging…' : 'Merge request'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
