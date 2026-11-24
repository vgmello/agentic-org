<!--
  @component
  Organization settings › Costs (app-shell-ux §0.1; costs-and-insights-ux §1 Organization › Cost, §4, §5): the
  organization's cost for a period with its breakdowns, each agent's budget bars and the prices the estimates use. The
  owner sees everything and adds rates; anyone else reads the prices and is told, once, that amounts are the owner's.
  Each part loads and fails on its own. Against a server that serves none of it, the page is the honest unavailable
  page instead of three empty parts.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import NotBuilt from '$lib/components/NotBuilt.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { costsApi } from '../../api';
	import { formatDay } from '../../dates';
	import { scoped } from '../../scoped';
	import type {
		PeriodRequest,
		PriceVersion,
		RecordsTarget,
		SubscriptionUsageSnapshot
	} from '../../types';
	import CostsError from '../states/CostsError.svelte';
	import AgentBudgets from '../agent-budgets/AgentBudgets.svelte';
	import OrganizationCost from '../organization-cost/OrganizationCost.svelte';
	import AddRateDialog from '../prices/AddRateDialog.svelte';
	import PricesSection from '../prices/PricesSection.svelte';
	import SubscriptionCredentialDialog from '../subscription-usage/SubscriptionCredentialDialog.svelte';
	import SubscriptionUsageSection from '../subscription-usage/SubscriptionUsageSection.svelte';
	import UsageRecordsSheet from '../usage-records/UsageRecordsSheet.svelte';
	import { toAppError } from '$lib/proto';

	let {
		organizationId,
		zone,
		memberHref
	}: {
		organizationId: string;
		/** The viewing account's IANA zone: periods and times are read in it. */
		zone: string;
		/** Where an agent's budgets are set (its profile's Cost tab). */
		memberHref: (memberId: string) => string;
	} = $props();

	const HEADING = 'costs-heading';
	// The UI's initial cost period is This month (§3 "Cost reads and periods").
	let chosen = $state<Omit<PeriodRequest, 'timeZone'>>({ kind: 'this_month' });
	const period = $derived<PeriodRequest>({ ...chosen, timeZone: zone });
	const cost = new Resource(() => scoped(() => costsApi.organizationCost(organizationId, period)));
	const budgets = new Resource(() => scoped(() => costsApi.agentBudgets(organizationId)));
	const subscriptionUsage = new Resource(() =>
		scoped(() => costsApi.subscriptionUsage(organizationId))
	);
	const prices = new Resource(() => costsApi.prices(organizationId));

	const unavailable = $derived(
		cost.state === 'unavailable' &&
			budgets.state === 'unavailable' &&
			subscriptionUsage.state === 'unavailable' &&
			prices.state === 'unavailable'
	);
	/** HQ couldn't be reached for any part: one failure with one Try again, not four. */
	const allFailed = $derived(
		cost.state === 'error' &&
			budgets.state === 'error' &&
			subscriptionUsage.state === 'error' &&
			prices.state === 'error'
	);
	const retryingAll = $derived(
		cost.reloading || budgets.reloading || subscriptionUsage.reloading || prices.reloading
	);
	function retryAll() {
		void cost.reload();
		void budgets.reload();
		void subscriptionUsage.reload();
		void prices.reload();
	}
	/** A rate's save was not confirmed: the prices part says so until Check status settles it (§5). */
	let unconfirmed = $state(false);
	function unconfirmedChanged(state: { unconfirmed: boolean; closed?: boolean }) {
		unconfirmed = state.unconfirmed;
		// The rate may have been recorded after all: read the list again rather than show a stale one.
		if (state.closed || !state.unconfirmed) void prices.reload();
	}
	/** Budgets are the owner's: a viewer who may not see amounts sees no budget part at all. */
	// Shown once a read says amounts are the viewer's (or a failure needs saying), so a non-owner never sees the budget
	// skeleton appear and vanish.
	const budgetsAllowed = $derived(
		budgets.state === 'loading'
			? !!(cost.data && cost.data.allowed)
			: !(budgets.data && !budgets.data.allowed)
	);
	/** Subscription model usage is owner-scoped like budgets: never shown to non-owner viewers. */
	const subscriptionAllowed = $derived(
		subscriptionUsage.state === 'loading'
			? !!(cost.data && cost.data.allowed)
			: !(subscriptionUsage.data && !subscriptionUsage.data.allowed)
	);

	let sheet = $state<ReturnType<typeof UsageRecordsSheet>>();
	let dialog = $state<ReturnType<typeof AddRateDialog>>();
	let credentialDialog = $state<ReturnType<typeof SubscriptionCredentialDialog>>();
	let collectingConnectionId = $state<string | undefined>(undefined);
	let opener: HTMLElement | null = null;

	// After a client navigation the shell focuses the area's h1 on the next tick; this page's heading takes focus after
	// that, so the person lands on what the page is for (app-shell-ux §6a).
	onMount(() => {
		const timer = setTimeout(() => {
			const active = document.activeElement;
			if (active && active !== document.body && !active.matches('main h1')) return;
			focusHeading();
		}, 0);
		return () => clearTimeout(timer);
	});

	function focusHeading() {
		const heading = document.getElementById(HEADING);
		if (!heading) return;
		heading.tabIndex = -1;
		heading.focus({ preventScroll: true });
	}

	function changePeriod(next: PeriodRequest) {
		chosen = { kind: next.kind, from: next.from, to: next.to };
		void cost.reload();
	}

	function openRecords(target: RecordsTarget, from: HTMLElement) {
		opener = from;
		sheet?.show(target, period);
	}

	function addRate(from: HTMLElement, prefill?: PriceVersion) {
		opener = from;
		dialog?.start(prefill);
	}

	async function collectSubscriptionUsage(connectionId?: string) {
		if (collectingConnectionId) return;
		collectingConnectionId = connectionId ?? '*';
		try {
			const res = await costsApi.collectSubscriptionUsage(organizationId, {
				...(connectionId ? { connectionId } : {}),
				timeZone: zone
			});
			await subscriptionUsage.reload();
			const authFailed = res.snapshots.find((s) => s.status === 'auth_required');
			const unavailableSnap = res.snapshots.find((s) => s.status === 'unavailable');
			if (authFailed) {
				toast.error(
					authFailed.message ||
						`Usage credential was rejected for ${authFailed.connectionName}. Update it and try again.`
				);
			} else if (unavailableSnap) {
				toast.error(
					unavailableSnap.message ||
						`Provider usage endpoint was unavailable for ${unavailableSnap.connectionName}.`
				);
			} else if (res.deferredConnectionIds && res.deferredConnectionIds.length > 0) {
				toast.info(
					`Updated subscription usage (${res.deferredConnectionIds.length} deferred; refresh again for remaining connections).`
				);
			} else {
				toast.success('Subscription model usage updated.');
			}
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			collectingConnectionId = undefined;
		}
	}

	function manageSubscriptionCredential(snapshot: SubscriptionUsageSnapshot, from: HTMLElement) {
		opener = from;
		credentialDialog?.start(snapshot);
	}

	async function subscriptionCredentialUpdated(message: string) {
		toast.success(message);
		await subscriptionUsage.reload();
	}

	async function added(price: PriceVersion) {
		toast.success(
			`Rate added: ${price.model} from ${formatDay(new Date(price.effectiveFrom), zone)}.`
		);
		await prices.reload();
	}

	function restoreFocus(event: Event) {
		event.preventDefault();
		if (opener?.isConnected) opener.focus();
		else focusHeading();
	}

	const names = $derived.by(() => {
		const list = prices.data?.prices ?? [];
		return {
			providers: [...new Set(list.map((p) => p.provider))].toSorted(),
			models: [...new Set(list.map((p) => p.model))].toSorted()
		};
	});
</script>

{#if unavailable}
	<NotBuilt title="Costs" place="Costs" area="Organization settings" />
{:else}
	<PageHeader
		title="Costs"
		id={HEADING}
		class="[&_h2:focus]:outline-none"
		description="What the organization's agent work costs, valued at API list prices in US dollars, and the prices those estimates use. Figures are estimates, not invoices."
	/>

	{#if allFailed}
		<div class="py-6">
			<CostsError
				level={3}
				title="Couldn't load costs"
				message={cost.message}
				onretry={retryAll}
				retrying={retryingAll}
			/>
		</div>
	{:else}
		<OrganizationCost
			resource={cost}
			{period}
			onperiodchange={changePeriod}
			onopenrecords={openRecords}
		/>

		{#if budgetsAllowed}
			<AgentBudgets resource={budgets} {memberHref} />
		{/if}

		{#if subscriptionAllowed}
			<SubscriptionUsageSection
				resource={subscriptionUsage}
				{zone}
				{collectingConnectionId}
				oncollect={(connId) => void collectSubscriptionUsage(connId)}
				onmanagecredential={manageSubscriptionCredential}
			/>
		{/if}

		<PricesSection resource={prices} {zone} {unconfirmed} onadd={addRate} />
	{/if}

	<UsageRecordsSheet bind:this={sheet} {organizationId} {zone} onCloseAutoFocus={restoreFocus} />
	<SubscriptionCredentialDialog
		bind:this={credentialDialog}
		{organizationId}
		onupdated={(msg) => void subscriptionCredentialUpdated(msg)}
		onCloseAutoFocus={restoreFocus}
	/>
	<AddRateDialog
		bind:this={dialog}
		{organizationId}
		{zone}
		providers={names.providers}
		models={names.models}
		onadded={added}
		onunconfirmed={unconfirmedChanged}
		onCloseAutoFocus={restoreFocus}
	/>
{/if}
