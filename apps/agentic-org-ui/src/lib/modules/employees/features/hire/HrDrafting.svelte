<!--
  @component
  Hire, Suggestion while the HR lead drafts (our-org-in-the-app §7): "HR lead is reading…"; after 60 s "HR lead is
  still starting…" with Details and Fill in the details yourself; if its run stops, "The HR lead stopped: <reason>."
  with Fill in the details yourself and Try again. What was typed in Describe stays above as a note. The request is
  re-read every second until the HR lead answers or stops.
-->
<script lang="ts">
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { toAppError } from '$lib/modules/offices/office-api';
	import { employeesApi } from '../../api';
	import { ageWords, backoff, clockWords, draftingPhase } from '../../hire';
	import { pollWhile } from '../../poll.svelte';
	import type { HireRequest } from '../../types';

	let {
		organizationId,
		request,
		retrying = false,
		onready,
		onupdate,
		onmanual,
		ontryagain,
		onback
	}: {
		organizationId: string;
		request: HireRequest;
		retrying?: boolean;
		/** The HR lead's proposal is ready. */
		onready: (r: HireRequest) => void;
		/** A newer reading while it drafts (still reading, or stopped). */
		onupdate: (r: HireRequest) => void;
		onmanual: () => void;
		ontryagain: () => void;
		/** Back to Describe; the request stays open. */
		onback: () => void;
	} = $props();

	let latest = $state.raw<HireRequest | undefined>();
	let checkedAt = $state(Date.now());
	let pollError = $state<string | undefined>();
	let showDetails = $state(false);
	let failures = $state(0);
	// An answer that arrives after this view is gone is not ours to apply (the flow guards too).
	let gone = false;
	$effect(() => () => (gone = true));

	const current = $derived(
		latest?.id === request.id && latest.updatedAt >= request.updatedAt ? latest : request
	);
	const phase = $derived(current.drafting ? draftingPhase(current.drafting, checkedAt) : 'reading');

	// The HR lead's turn runs at the office: follow it until it answers or stops.
	pollWhile(
		() => phase !== 'stopped' && phase !== 'ready',
		async () => {
			try {
				const r = await employeesApi.hireRequest(organizationId, current.id);
				if (gone) return;
				pollError = undefined;
				failures = 0;
				if (r.drafting?.state === 'ready') {
					// Stop following either way: the flow decides whether the answer still applies.
					latest = r;
					onready(r);
				} else {
					latest = r;
					onupdate(r);
				}
			} catch (e) {
				if (gone) return;
				pollError = toAppError(e).message;
				failures++;
			}
			checkedAt = Date.now();
		},
		() => backoff(1000, failures)
	);
</script>

<div class="max-w-2xl space-y-6">
	{#if current.reason}
		<figure class="rounded-md border border-line px-4 py-3">
			<figcaption class="text-xs/5 text-fg-muted">
				You asked for, into {current.homeTeam.name}
			</figcaption>
			<blockquote class="mt-1 text-sm/6 text-fg-secondary [overflow-wrap:anywhere]">
				{current.reason}
			</blockquote>
		</figure>
	{/if}

	{#if phase === 'stopped'}
		<div role="alert" class="space-y-1">
			<p class="text-sm/6 font-semibold text-danger-text [overflow-wrap:anywhere]">
				The HR lead stopped: {current.drafting?.stoppedReason ?? 'no reason was given'}.
			</p>
			<p class="text-sm/6 text-fg-secondary">
				Nothing has been created. Fill in the details yourself, or ask the HR lead again.
			</p>
		</div>
		<div class="flex flex-wrap gap-3">
			<Button class={mobileButton} onclick={onmanual}>Fill in the details yourself</Button>
			<Button
				variant="outline"
				class="min-w-24 {mobileButton}"
				aria-busy={retrying}
				onclick={() => !retrying && ontryagain()}>{retrying ? 'Trying again…' : 'Try again'}</Button
			>
		</div>
	{:else}
		<div role="status" class="flex items-start gap-x-3">
			<span
				aria-hidden="true"
				class="mt-1 size-4 shrink-0 rounded-full border-2 border-info-text/30 border-t-info-text motion-safe:animate-spin"
			></span>
			<div>
				<p class="text-sm/6 font-semibold text-fg">
					{phase === 'still_starting' ? 'HR lead is still starting…' : 'HR lead is reading…'}
				</p>
				<p class="text-sm/6 text-fg-secondary">
					It drafts a proposal for you to check. Nothing is created until you confirm it.
				</p>
			</div>
		</div>
		<div class="flex flex-wrap gap-3">
			{#if phase === 'still_starting'}
				<Button class={mobileButton} onclick={onmanual}>Fill in the details yourself</Button>
				<Button
					variant="outline"
					class={mobileButton}
					aria-expanded={showDetails}
					aria-controls="hire-hr-details"
					onclick={() => (showDetails = !showDetails)}>Details</Button
				>
			{/if}
			<Button variant="outline" class={mobileButton} onclick={onback}>Back</Button>
		</div>
		{#if phase === 'still_starting'}
			<div id="hire-hr-details" hidden={!showDetails} class="text-sm/6 text-fg-secondary">
				{#if current.drafting}
					<p>
						Asked at <time datetime={current.drafting.startedAt}
							>{clockWords(current.drafting.startedAt)}</time
						>, {ageWords(current.drafting.startedAt, checkedAt)}. The HR lead's run hasn't answered
						yet.
					</p>
				{/if}
				<p class="mt-1">
					You can wait here, or fill in the details yourself. Until then the request stays in Hire
					requests as Drafting.
				</p>
			</div>
		{/if}
	{/if}
	{#if pollError}
		<p class="text-xs/5 text-danger-text [overflow-wrap:anywhere]">
			Couldn't check on the HR lead: {pollError} Checking again.
		</p>
	{/if}
</div>
