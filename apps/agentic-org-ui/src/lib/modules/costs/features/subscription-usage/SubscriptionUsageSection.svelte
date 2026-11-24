<!--
  @component
  Organization settings › Costs › Subscription model usage (TASK-79; owner only): provider-reported total, daily,
  weekly, and optional short-term rolling usage per model across enrolled subscription connections, with manual
  collection refresh and private credential management. Explicitly marked as provider-reported usage, never
  API-equivalent USD cost estimates. Every null/missing counter renders as "—", never fabricated as zero.
-->
<script lang="ts">
	import KeyRound from '@lucide/svelte/icons/key-round';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import { Button } from '$lib/components/ui/button';
	import { mobileButton } from '$lib/components/classes';
	import type { Resource } from '$lib/components/resource.svelte';
	import type { Scoped } from '../../scoped';
	import type {
		SubscriptionModelUsage,
		SubscriptionUsageOverview,
		SubscriptionUsageSnapshot,
		SubscriptionUsageWindow
	} from '../../types';
	import CostsError from '../states/CostsError.svelte';
	import {
		authSourceLabel,
		formatCollectedAt,
		hasAnyShortTerm,
		providerLabel,
		SUBSCRIPTION_USAGE_NOTICE,
		subscriptionStatusDisplay,
		summarizeWindow
	} from './present';

	let {
		resource,
		zone,
		collectingConnectionId,
		oncollect,
		onmanagecredential
	}: {
		resource: Resource<Scoped<SubscriptionUsageOverview>>;
		zone: string;
		/** `'*'` while refreshing all connections, a specific `connectionId`, or `undefined`. */
		collectingConnectionId?: string;
		oncollect: (connectionId?: string) => void;
		onmanagecredential: (snapshot: SubscriptionUsageSnapshot, from: HTMLElement) => void;
	} = $props();

	const headingId = $props.id();
	const data = $derived(resource.data?.allowed ? resource.data.value : undefined);
	const totalsHaveShortTerm = $derived(data ? hasAnyShortTerm(data.totalsByModel) : false);
	const collectedAsOfText = $derived(
		data?.collectedAsOf ? formatCollectedAt(data.collectedAsOf, zone) : undefined
	);
	const isCollectingAll = $derived(collectingConnectionId === '*');
	const anyCollecting = $derived(Boolean(collectingConnectionId));
</script>

{#snippet windowCell(title: string, win: SubscriptionUsageWindow | undefined, testKey: string)}
	{@const summary = summarizeWindow(win)}
	<div
		class="min-w-0 rounded-md bg-surface-raised/50 px-2.5 py-2 ring-1 ring-line-subtle"
		data-window={testKey}
	>
		<p class="text-[11px]/4 font-medium text-fg-muted [overflow-wrap:anywhere]">
			{summary.durationLabel ? `${title} (${summary.durationLabel})` : title}
		</p>
		<p class="mt-0.5 text-sm/5 font-semibold text-fg tabular-nums" data-testid="window-utilization">
			{summary.utilization}
		</p>
		<p class="mt-0.5 text-xs/5 text-fg-secondary tabular-nums [overflow-wrap:anywhere]">
			<span>{summary.requests} req</span>
			<span aria-hidden="true"> · </span>
			<span>{summary.tokens} tok</span>
		</p>
	</div>
{/snippet}

{#snippet modelRows(models: readonly SubscriptionModelUsage[], showShortTerm: boolean)}
	<ul role="list" class="divide-y divide-line-subtle rounded-lg border border-line-subtle">
		{#each models as row (row.model)}
			<li class="grid gap-3 px-3 py-3 min-[640px]:px-4" data-subscription-model={row.model}>
				<p class="min-w-0 font-mono text-xs/5 font-semibold text-fg [overflow-wrap:anywhere]">
					{row.model}
				</p>
				<div
					class={[
						'grid grid-cols-1 gap-2 min-[480px]:grid-cols-2',
						showShortTerm ? 'min-[900px]:grid-cols-4' : 'min-[900px]:grid-cols-3'
					]}
				>
					{@render windowCell('Total', row.total, 'total')}
					{@render windowCell('Daily', row.daily, 'daily')}
					{@render windowCell('Weekly', row.weekly, 'weekly')}
					{#if showShortTerm}
						{@render windowCell('Short-term', row.shortTerm, 'short-term')}
					{/if}
				</div>
			</li>
		{/each}
	</ul>
{/snippet}

<section
	aria-labelledby={headingId}
	class="border-t border-line-subtle py-6"
	data-testid="subscription-usage"
>
	<div
		class="flex flex-wrap items-start justify-between gap-3 px-4 min-[640px]:px-6 min-[1024px]:px-8"
	>
		<div class="min-w-0 flex-1">
			<h3 id={headingId} class="text-base/7 font-semibold text-fg [overflow-wrap:anywhere]">
				Subscription usage
			</h3>
			<p
				class="mt-1 max-w-3xl text-sm/6 text-fg-muted [overflow-wrap:anywhere]"
				data-testid="subscription-usage-notice"
			>
				{SUBSCRIPTION_USAGE_NOTICE}
				{#if collectedAsOfText}
					<span class="tabular-nums"> Updated {collectedAsOfText}.</span>
				{/if}
			</p>
		</div>
		{#if data && data.snapshots.length > 0}
			<Button
				type="button"
				variant="outline"
				size="sm"
				disabled={anyCollecting}
				onclick={() => oncollect()}
				class="inline-flex h-auto min-h-9 max-w-full flex-wrap items-center gap-1.5 py-1.5 whitespace-normal {mobileButton}"
			>
				<RefreshCw
					class={['size-3.5 shrink-0', isCollectingAll && 'animate-spin']}
					aria-hidden="true"
				/>
				<span>{isCollectingAll ? 'Refreshing…' : 'Refresh usage'}</span>
			</Button>
		{/if}
	</div>

	<div class="mt-4">
		{#if resource.state === 'loading'}
			<SkeletonRows label="subscription usage" rows={3} />
		{:else if resource.state === 'unavailable'}
			<p class="px-4 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
				This server doesn't serve subscription usage yet. Nothing is shown rather than a guess.
			</p>
		{:else if resource.state === 'unsupported'}
			<div class="px-4 min-[640px]:px-6 min-[1024px]:px-8" data-testid="resource-unsupported">
				<h4 class="text-sm/6 font-semibold text-fg">Not available in this version</h4>
				<p class="mt-1 max-w-xl text-sm/6 text-fg-muted [overflow-wrap:anywhere]">
					{resource.message || "This version of the office doesn't support subscription usage."}
				</p>
			</div>
		{:else if resource.state === 'error'}
			<CostsError
				title="Couldn't load subscription usage"
				message={resource.message}
				onretry={() => resource.reload()}
				retrying={resource.reloading}
			/>
		{:else if data}
			{#if data.snapshots.length === 0}
				<p
					class="px-4 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8"
					data-testid="subscription-usage-empty"
				>
					No provider subscription connections yet. Connect a Claude or Codex subscription to view
					provider-reported model usage.
				</p>
			{:else}
				<div class="space-y-6 px-4 min-[640px]:px-6 min-[1024px]:px-8">
					{#if data.partialTotals}
						<p
							class="rounded-md bg-surface-raised px-3 py-2 text-xs/5 text-fg-secondary ring-1 ring-line-subtle"
							data-testid="subscription-partial-totals"
						>
							Totals are partial{#if data.excludedSnapshotCount > 0}
								({data.excludedSnapshotCount}
								{data.excludedSnapshotCount === 1 ? 'subscription' : 'subscriptions'} stale or unavailable
								excluded){/if}. Unknown provider counters stay “—” rather than zero.
						</p>
					{/if}

					{#if data.totalsByModel.length > 0}
						<div class="space-y-2" data-testid="subscription-totals">
							<h4 class="text-xs/5 font-semibold tracking-wide text-fg-muted uppercase">
								Combined across active subscriptions
							</h4>
							{@render modelRows(data.totalsByModel, totalsHaveShortTerm)}
						</div>
					{/if}

					<div class="space-y-3" data-testid="subscription-snapshots">
						<h4 class="text-xs/5 font-semibold tracking-wide text-fg-muted uppercase">
							Subscriptions ({data.snapshots.length})
						</h4>
						<ul role="list" class="space-y-3">
							{#each data.snapshots as snap (snap.connectionId)}
								{@const status = subscriptionStatusDisplay(snap.status, snap.stale)}
								{@const collectedText = formatCollectedAt(snap.collectedAt, zone)}
								{@const lastOkText = formatCollectedAt(snap.lastOkCollectedAt, zone)}
								{@const snapHasShortTerm = hasAnyShortTerm(snap.models)}
								{@const isThisRefreshing = collectingConnectionId === snap.connectionId}
								<li
									class="rounded-lg border border-line-subtle bg-surface p-4"
									data-subscription={snap.connectionId}
								>
									<div class="flex flex-wrap items-start justify-between gap-3">
										<div class="min-w-0 flex-1 space-y-1">
											<div class="flex flex-wrap items-center gap-2">
												<h5 class="text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]">
													{snap.connectionName}
												</h5>
												<span
													class="inline-flex max-w-full items-center rounded-full bg-surface-raised px-2 py-0.5 text-xs/5 font-medium text-fg-secondary ring-1 ring-line ring-inset"
												>
													{providerLabel(snap.providerType)}
												</span>
												<span
													data-testid="subscription-status"
													class={[
														'inline-flex max-w-full items-center gap-x-1.5 rounded-full px-2 py-0.5 text-xs/5 font-medium',
														status.tone === 'owner'
															? 't-amber'
															: status.tone === 'ok'
																? 't-green'
																: 'bg-surface-raised text-fg-secondary ring-1 ring-line ring-inset'
													]}
												>
													{status.label}
												</span>
											</div>
											<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
												<span>{authSourceLabel(snap.authSource)}</span>
												{#if snap.accountHint}
													<span aria-hidden="true"> · </span>
													<span>{snap.accountHint}</span>
												{/if}
												{#if collectedText}
													<span aria-hidden="true"> · </span>
													<span class="tabular-nums">Checked {collectedText}</span>
												{/if}
											</p>
											{#if (snap.stale || snap.status !== 'ok') && lastOkText}
												<p class="text-xs/5 text-fg-muted tabular-nums">
													Last successful read: {lastOkText}
												</p>
											{/if}
											{#if snap.message}
												<p class="text-xs/5 text-fg-secondary [overflow-wrap:anywhere]">
													{snap.message}
												</p>
											{/if}
										</div>

										<div class="flex min-w-0 flex-wrap items-center gap-2">
											<Button
												type="button"
												variant="outline"
												size="sm"
												disabled={anyCollecting}
												onclick={() => oncollect(snap.connectionId)}
												class="inline-flex h-auto min-h-9 max-w-full flex-wrap items-center gap-1.5 py-1.5 whitespace-normal {mobileButton}"
											>
												<RefreshCw
													class={['size-3.5 shrink-0', isThisRefreshing && 'animate-spin']}
													aria-hidden="true"
												/>
												<span>{isThisRefreshing ? 'Refreshing…' : 'Refresh'}</span>
											</Button>
											<Button
												type="button"
												variant="outline"
												size="sm"
												disabled={anyCollecting}
												onclick={(e) => onmanagecredential(snap, e.currentTarget)}
												class="inline-flex h-auto min-h-9 max-w-full flex-wrap items-center gap-1.5 py-1.5 whitespace-normal {mobileButton}"
											>
												<KeyRound class="size-3.5 shrink-0" aria-hidden="true" />
												<span>
													{snap.status === 'auth_required' ? 'Set credential' : 'Manage credential'}
												</span>
											</Button>
										</div>
									</div>

									{#if snap.models.length > 0}
										<div class="mt-3">
											{@render modelRows(snap.models, snapHasShortTerm)}
										</div>
									{:else}
										<p class="mt-3 text-xs/5 text-fg-muted">
											{#if snap.status === 'auth_required'}
												Add a private usage credential to collect per-model usage for this
												subscription.
											{:else if snap.status === 'never_collected'}
												No usage collected yet. Select Refresh to query the provider.
											{:else}
												No per-model usage counters reported for this subscription yet.
											{/if}
										</p>
									{/if}
								</li>
							{/each}
						</ul>
					</div>
				</div>
			{/if}
		{/if}
	</div>
</section>
