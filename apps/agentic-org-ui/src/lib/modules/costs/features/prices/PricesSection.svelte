<!--
  @component
  Organization settings › Costs › Prices (costs-and-insights-ux §5; document 10 `model_prices`): the versioned rates
  the token estimates use, one entry per provider and model with the rate in effect now, any rate scheduled to replace
  it, and earlier rates one step away. Rates are dollars per million tokens; an unknown rate reads "—" (records with
  those tokens are unpriced), a zero rate is an explicit $0.00. The owner adds rates; everyone else reads them.
  Wide sections show a table; narrower, each model stacks its rates.
-->
<script lang="ts">
	import CostsError from '../states/CostsError.svelte';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import { mobileButton } from '$lib/components/classes';
	import type { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { formatRate } from '../../money';
	import { NO_PRICES, UNKNOWN_RATE, effectiveText, groupPrices } from '../../present';
	import type { PriceVersion, PricesOverview } from '../../types';

	let {
		resource,
		zone,
		unconfirmed = false,
		onadd
	}: {
		resource: Resource<PricesOverview>;
		/** The viewer's zone, for effective dates. */
		zone: string;
		/** A rate's save is not confirmed yet: Add a rate… reopens it for Check status. */
		unconfirmed?: boolean;
		/** Add a rate, prefilled from a model's current rates when given. */
		onadd: (opener: HTMLElement, prefill?: PriceVersion) => void;
	} = $props();

	const headingId = $props.id();
	const data = $derived(resource.data);
	const groups = $derived(data ? groupPrices(data.prices, new Date()) : []);
	const canManage = $derived(!!data?.viewer.canManage);

	const RATES = [
		['inputMicroUsdPerMillion', 'Input'],
		['outputMicroUsdPerMillion', 'Output'],
		['cacheReadMicroUsdPerMillion', 'Cache read'],
		['cacheWriteMicroUsdPerMillion', 'Cache write']
	] as const;
	const gutters = 'px-4 min-[640px]:px-6 min-[1024px]:px-8';
</script>

{#snippet rate(value: number | null)}
	{#if value === null}
		<span aria-hidden="true">—</span><span class="sr-only">{UNKNOWN_RATE}</span>
	{:else}{formatRate(value)}{/if}
{/snippet}

{#snippet rateList(price: PriceVersion)}
	<dl class="grid grid-cols-2 gap-x-4 gap-y-1 text-sm/6">
		{#each RATES as [key, label] (key)}
			<div class="flex flex-wrap justify-between gap-x-2">
				<dt class="text-fg-muted">{label}</dt>
				<dd class="text-fg-secondary tabular-nums">{@render rate(price[key])}</dd>
			</div>
		{/each}
	</dl>
{/snippet}

{#snippet versionLine(price: PriceVersion, word: string)}
	<p class="text-xs/5 text-fg-muted">
		<span class="font-medium text-fg-secondary">{word}</span> · {effectiveText(
			price,
			zone
		)}{price.used ? ' · in use, never changes' : ''}
	</p>
{/snippet}

<section aria-labelledby={headingId} class="border-t border-line-subtle py-6" data-testid="prices">
	<div class="flex flex-wrap items-start justify-between gap-x-6 gap-y-3 {gutters}">
		<div class="min-w-0 flex-1 basis-64">
			<h3 id={headingId} class="text-base/7 font-semibold text-fg">Prices</h3>
			<p class="mt-1 max-w-3xl text-sm/6 text-fg-muted">
				The API list prices token estimates use, in US dollars per million tokens. Each record keeps
				the exact price version it was valued with; a new rate never re-estimates past spend.
			</p>
		</div>
		{#if canManage && groups.length > 0}
			<Button
				class={mobileButton}
				onclick={(e: MouseEvent) => onadd(e.currentTarget as HTMLElement)}
				><MiniIcon name="plus" class="-ml-0.5 size-5" />Add a rate…</Button
			>
		{/if}
	</div>

	{#if unconfirmed}
		<p
			role="status"
			class="mt-3 max-w-3xl text-sm/6 text-fg-secondary {gutters}"
			data-testid="unconfirmed-save"
		>
			A rate's save isn't confirmed yet. Open Add a rate… to check its status before adding another.
		</p>
	{/if}

	<div class="mt-4">
		{#if resource.state === 'loading'}
			<SkeletonRows label="prices" rows={3} />
		{:else if resource.state === 'unavailable'}
			<p class="text-sm/6 text-fg-muted {gutters}">
				This server doesn't serve prices yet. Nothing is shown rather than a guess.
			</p>
		{:else if resource.state === 'unsupported'}
			<div class={gutters} data-testid="resource-unsupported">
				<h4 class="text-sm/6 font-semibold text-fg">Not available in this version</h4>
				<p class="mt-1 max-w-xl text-sm/6 text-fg-muted [overflow-wrap:anywhere]">
					{resource.message || "This version of the office doesn't support prices."}
				</p>
			</div>
		{:else if resource.state === 'error'}
			<CostsError
				title="Couldn't load the prices"
				message={resource.message}
				onretry={() => resource.reload()}
				retrying={resource.reloading}
			/>
		{:else if data && groups.length === 0}
			<EmptyState level={4} title="No prices set" description={NO_PRICES} class="py-12">
				{#snippet action()}
					{#if canManage}
						<Button
							class={mobileButton}
							onclick={(e: MouseEvent) => onadd(e.currentTarget as HTMLElement)}
							><MiniIcon name="plus" class="-ml-0.5 size-5" />Add a rate…</Button
						>
					{/if}
				{/snippet}
			</EmptyState>
		{:else if data}
			{#if !canManage}
				<p class="pb-4 text-sm/6 text-fg-muted {gutters}" data-testid="prices-read-only">
					Only the owner adds rates.
				</p>
			{/if}
			<ul role="list" class="divide-y divide-line-subtle border-y border-line-subtle">
				{#each groups as group (`${group.provider}/${group.model}`)}
					<li class="px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8" data-price-model={group.model}>
						<div class="flex flex-wrap items-start justify-between gap-x-4 gap-y-2">
							<div class="min-w-0 flex-1 basis-48">
								<p class="font-mono text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]">
									{group.model}
								</p>
								<p class="text-xs/5 text-fg-muted">{group.provider}</p>
							</div>
							{#if canManage}
								<Button
									variant="outline"
									class={mobileButton}
									aria-label="New rate for {group.model}"
									onclick={(e: MouseEvent) =>
										onadd(e.currentTarget as HTMLElement, group.current ?? group.scheduled[0])}
									>New rate…</Button
								>
							{/if}
						</div>

						<div class="mt-3 grid gap-4 min-[960px]:grid-cols-2">
							{#if group.current}
								<div>
									{@render versionLine(group.current, 'In effect')}
									<div class="mt-2">{@render rateList(group.current)}</div>
								</div>
							{:else}
								<p class="text-sm/6 text-fg-muted">
									No rate in effect yet: records for this model are unpriced until the scheduled
									rate starts.
								</p>
							{/if}
							{#each group.scheduled as price (price.id)}
								<div data-testid="scheduled-rate">
									{@render versionLine(price, 'Scheduled')}
									<div class="mt-2">{@render rateList(price)}</div>
								</div>
							{/each}
						</div>

						{#if group.earlier.length > 0}
							<details class="group mt-3">
								<summary
									class="inline-flex min-h-6 cursor-pointer items-center gap-x-1 rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
								>
									Earlier rates ({group.earlier.length})
								</summary>
								<ul role="list" class="mt-2 flex flex-col gap-4 border-l border-line pl-4">
									{#each group.earlier as price (price.id)}
										<li>
											{@render versionLine(price, 'Replaced')}
											<div class="mt-2 max-w-md">{@render rateList(price)}</div>
										</li>
									{/each}
								</ul>
							</details>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</section>
