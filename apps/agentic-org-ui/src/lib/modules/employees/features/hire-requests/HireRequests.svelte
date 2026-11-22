<!--
  @component
  Hire requests `/teams/hires` (our-org-in-the-app §7.1, APPR-6): each hire with who hired, the home team, its state
  (Drafting · Hiring… · Not finished: <step that failed> · Hired · Withdrawn) and age. The owner sees every hire,
  anyone else its own. A drafting request opens in Hire or is withdrawn; one that stopped partway is tried again
  without a new confirmation. Rows still hiring are re-read every two seconds until they settle.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { gutters, mobileButton } from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { toAppError } from '$lib/office/api';
	import { employeesApi } from '../../api';
	import { pollWhile } from '../../poll.svelte';
	import {
		ageWords,
		backoff,
		doneText,
		hireUrl,
		requestName,
		requestStatus,
		type RequestStatus
	} from '../../hire';
	import type { HireRequest } from '../../types';

	let { organizationId }: { organizationId: string } = $props();

	const requests = new Resource(() => employeesApi.hireRequests(organizationId));
	/** Rows read since the first load. A failed re-read keeps them, says so, and tries again (app-shell-ux §6a). */
	let refreshed = $state.raw<HireRequest[] | undefined>();
	let refreshError = $state<string | undefined>();
	let failures = $state(0);

	let withdrawing = $state<HireRequest | undefined>();
	/** The row just withdrawn: its Withdraw button is gone, so focus goes to its name. */
	let withdrawnId: string | undefined;
	let retrying = $state<string | undefined>();
	let rowError = $state<{ id: string; message: string } | undefined>();

	const rowsOf = (first: HireRequest[]) => refreshed ?? first;
	const settling = $derived(
		(refreshed ?? requests.data ?? []).some((r) => r.state === 'hiring' && !r.failedStep)
	);

	async function refresh() {
		try {
			refreshed = await employeesApi.hireRequests(organizationId);
			refreshError = undefined;
			failures = 0;
		} catch (e) {
			refreshError = toAppError(e).message;
			failures++;
		}
	}

	// While a hire is provisioning, or a re-read failed, the list follows the office until every hire has settled.
	pollWhile(
		() => requests.state === 'ready' && (settling || !!refreshError),
		refresh,
		() => backoff(2000, failures)
	);

	const pill = (s: RequestStatus) =>
		s.tone === 'working'
			? 'bg-brand/10 text-brand-text ring-brand/30'
			: s.tone === 'ok'
				? 't-green'
				: s.tone === 'bad'
					? 't-red'
					: s.tone === 'owner'
						? 't-amber'
						: tone[s.tone];

	/** Where a row's name leads: a hire still in the flow opens there, a hired agent opens its profile. */
	function linkOf(r: HireRequest): string | undefined {
		if (r.state === 'hired' && r.employeeId) return `/members/${encodeURIComponent(r.employeeId)}`;
		// A draft opens for its hirer only; others see it in the list (§7.1).
		if ((r.state === 'drafting' && r.byViewer) || r.state === 'hiring')
			return hireUrl({ requestId: r.id });
		return undefined;
	}

	const focusName = (id: string) =>
		setTimeout(() => document.getElementById(`hire-${id}`)?.focus(), 0);

	async function tryAgain(r: HireRequest) {
		if (retrying) return;
		retrying = r.id;
		rowError = undefined;
		try {
			await employeesApi.retry(organizationId, r.id);
			toast.success(`Finishing ${requestName(r)}'s hire.`);
			await refresh();
			// Try again goes with the failure: focus the row's name, which stays.
			focusName(r.id);
		} catch (e) {
			rowError = { id: r.id, message: toAppError(e).message };
		} finally {
			retrying = undefined;
		}
	}

	async function withdraw() {
		const r = withdrawing!;
		await employeesApi.withdraw(organizationId, r.id);
		withdrawnId = r.id;
		toast.success(`Withdrew the hire of ${requestName(r)}.`);
		await refresh();
	}

	/** The Withdraw button goes with the state; focus lands on the row's name, which stays. */
	function focusRow(event: Event) {
		const id = withdrawnId;
		withdrawnId = undefined;
		if (!id) return;
		event.preventDefault();
		focusName(id);
	}
</script>

<PageHeader
	title="Hire requests"
	count={(refreshed ?? requests.data)?.length || undefined}
	countLabel={(refreshed ?? requests.data)?.length === 1 ? 'hire request' : 'hire requests'}
	description="Each hire with who started it, its home team and its state, newest first. Nothing is created until its hirer confirms it."
	back={{ href: '/teams', label: 'Teams' }}
>
	{#snippet actions()}
		<Button href={resolve(hireUrl() as '/')} class={mobileButton}>
			<MiniIcon name="plus" class="-ml-0.5 size-5" /> Hire an agent
		</Button>
	{/snippet}
</PageHeader>

<Loaded
	resource={requests}
	label="hire requests"
	error="Can't load hire requests"
	place="Hire requests"
	area="Teams"
	isEmpty={(v) => v.length === 0}
	emptyTitle="No hire requests"
	emptyDescription="Hires you start show here."
	emptyLevel={3}
	errorLevel={3}
>
	{#snippet children(first)}
		{@const rows = rowsOf(first)}
		{#if refreshError}
			<p role="status" class="py-3 text-xs/5 text-danger-text {gutters} [overflow-wrap:anywhere]">
				Couldn't check on hires: {refreshError} Checking again.
			</p>
		{/if}
		<ul role="list" class="divide-y divide-line-subtle" aria-label="Hire requests">
			{#each rows as r (r.id)}
				{@const name = requestName(r)}
				{@const status = requestStatus(r, r.name)}
				{@const href = linkOf(r)}
				<li
					class="relative flex flex-wrap items-center justify-between gap-x-6 gap-y-3 py-4 transition-colors duration-150 hover:bg-surface-raised/60 motion-reduce:transition-none {gutters}"
				>
					<div class="min-w-0 flex-1 basis-64">
						<p class="flex flex-wrap items-baseline gap-x-2 text-sm/6">
							{#if href}
								<a
									id="hire-{r.id}"
									href={resolve(href as '/')}
									class="rounded-sm font-semibold text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
									><span class="absolute inset-0" aria-hidden="true"></span>{name}</a
								>
							{:else}
								<span
									id="hire-{r.id}"
									tabindex="-1"
									class="font-semibold text-fg outline-none [overflow-wrap:anywhere]">{name}</span
								>
							{/if}
							{#if r.role}<span class="text-fg-muted [overflow-wrap:anywhere]">{r.role}</span>{/if}
						</p>
						<p class="mt-1 flex flex-wrap gap-x-2 text-xs/5 text-fg-muted">
							<span>By {r.requestedBy.name}</span>
							<span aria-hidden="true">·</span>
							<span>Into {r.homeTeam.name}</span>
							<span aria-hidden="true">·</span>
							<time datetime={r.createdAt}>{ageWords(r.createdAt)}</time>
						</p>
						{#if status.detail}
							<p class="mt-1 text-xs/5 text-danger-text [overflow-wrap:anywhere]">
								{status.detail}
							</p>
						{/if}
						{#if (r.failedStep || r.state === 'failed') && r.doneSteps.length > 0}
							<p class="mt-1 text-xs/5 text-fg-muted">Already done: {doneText(r, name)}.</p>
						{/if}
						{#if r.state === 'withdrawn' && r.closedReason}
							<p class="mt-1 text-xs/5 text-fg-muted [overflow-wrap:anywhere]">{r.closedReason}</p>
						{/if}
						{#if rowError?.id === r.id}
							<p role="alert" class="mt-1 text-xs/5 text-danger-text [overflow-wrap:anywhere]">
								{rowError.message}
							</p>
						{/if}
					</div>
					<div class="relative flex max-w-full min-w-0 flex-wrap items-center gap-x-3 gap-y-2">
						<StatusPill label={status.label} class={pill(status)} />
						{#if status.action === 'open' && r.byViewer}
							<Button
								href={resolve(hireUrl({ requestId: r.id }) as '/')}
								variant="outline"
								class={mobileButton}
								aria-label="Review the proposal for {name}">Review</Button
							>
							<Button
								variant="outline"
								class={mobileButton}
								aria-label="Withdraw the hire of {name}"
								onclick={() => (withdrawing = r)}>Withdraw</Button
							>
						{:else if status.action === 'try_again'}
							<Button
								variant="outline"
								class="min-w-24 {mobileButton}"
								aria-busy={retrying === r.id}
								aria-label="Try again: finish hiring {name}"
								onclick={() => tryAgain(r)}
								>{retrying === r.id ? 'Trying again…' : 'Try again'}</Button
							>
						{/if}
					</div>
				</li>
			{/each}
		</ul>
	{/snippet}
</Loaded>

<ConfirmDialog
	bind:open={() => withdrawing !== undefined, (open) => !open && (withdrawing = undefined)}
	title="Withdraw the hire of {withdrawing ? requestName(withdrawing) : ''}?"
	description="The proposal is closed and stays listed as Withdrawn. Nothing was created, so nothing is removed."
	confirmLabel="Withdraw"
	busyLabel="Withdrawing…"
	onconfirm={withdraw}
	onCloseAutoFocus={focusRow}
/>
