<!--
  @component
  Organization settings › Costs › Agent budgets (costs-and-insights-ux §1 Organization › Cost, §4; owner only): every
  agent's daily and monthly budget bars in the budget zone, which is the owner's time zone (HQ's before that is
  known), labelled with it. Budgets are set on the agent's profile, under Cost; the agent's name goes there. An unknown
  budget zone is said plainly: budget periods can't start without it. A viewer who may not see amounts gets nothing
  here; the page says why once.
-->
<script lang="ts">
	import CostsError from '../states/CostsError.svelte';
	import { resolve } from '$app/paths';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import type { Resource } from '$lib/components/resource.svelte';
	import { zoneSourceText } from '../../present';
	import type { Scoped } from '../../scoped';
	import type { AgentBudgets } from '../../types';
	import BudgetBar from './BudgetBar.svelte';

	let {
		resource,
		memberHref
	}: {
		resource: Resource<Scoped<AgentBudgets>>;
		/** Where an agent's budgets are set: its profile's Cost tab. */
		memberHref: (memberId: string) => string;
	} = $props();

	const headingId = $props.id();
	const data = $derived(resource.data?.allowed ? resource.data.value : undefined);
	const withBudget = $derived(data?.agents.filter((a) => a.daily || a.monthly) ?? []);
	const withoutBudget = $derived(data?.agents.filter((a) => !a.daily && !a.monthly) ?? []);
</script>

<section
	aria-labelledby={headingId}
	class="border-t border-line-subtle py-6"
	data-testid="agent-budgets"
>
	<div class="px-4 min-[640px]:px-6 min-[1024px]:px-8">
		<h3 id={headingId} class="text-base/7 font-semibold text-fg">Agent budgets</h3>
		<p class="mt-1 max-w-3xl text-sm/6 text-fg-muted">
			{#if data?.zone.name}
				Each agent's day and month run in {data.zone.name}, {zoneSourceText(data.zone.source)}, so
				"today" here can differ from your own. Budgets are set on each agent's profile, under Cost.
			{:else}
				Daily and monthly limits for each agent. Budgets are set on each agent's profile, under
				Cost.
			{/if}
		</p>
	</div>

	<div class="mt-4">
		{#if resource.state === 'loading'}
			<SkeletonRows label="agent budgets" rows={3} avatar />
		{:else if resource.state === 'unavailable'}
			<p class="px-4 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
				This server doesn't serve agent budgets yet. Nothing is shown rather than a guess.
			</p>
		{:else if resource.state === 'error'}
			<CostsError
				title="Couldn't load the agent budgets"
				message={resource.message}
				onretry={() => resource.reload()}
				retrying={resource.reloading}
			/>
		{:else if data}
			{#if !data.zone.name}
				<p
					class="px-4 text-sm/6 text-fg-secondary min-[640px]:px-6 min-[1024px]:px-8"
					data-testid="budget-zone-unknown"
				>
					Agent budgets can't run yet: {zoneSourceText(data.zone.source)} isn't known, and budget days
					and months are never counted in UTC by default.
					{#if data.zone.source === 'owner_account'}<a
							href={resolve('/preferences')}
							class="font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
							>Set your time zone</a
						>{/if}
				</p>
			{:else if data.agents.length === 0}
				<p class="px-4 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
					No agents yet. Agents hired through HR appear here with their budgets.
				</p>
			{:else}
				{#if withBudget.length > 0}
					<ul role="list" class="divide-y divide-line-subtle border-y border-line-subtle">
						{#each withBudget as agent (agent.memberId)}
							<li
								class="grid gap-x-8 gap-y-4 px-4 py-4 min-[640px]:px-6 min-[1024px]:grid-cols-[minmax(10rem,14rem)_1fr_1fr] min-[1024px]:px-8"
								data-agent={agent.memberId}
							>
								<p class="min-w-0 text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]">
									<a
										href={resolve(memberHref(agent.memberId) as '/')}
										class="inline-flex min-h-6 items-center rounded-sm hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
										>{agent.name}</a
									>{#if agent.former}<span class="ml-2 text-xs/5 font-normal text-fg-muted"
											>former</span
										>{/if}
								</p>
								{#if agent.daily}
									<BudgetBar budget={agent.daily} kind="daily" zone={data.zone.name} />
								{:else}
									<p class="text-xs/5 text-fg-muted min-[1024px]:pt-0.5">No daily budget</p>
								{/if}
								{#if agent.monthly}
									<BudgetBar budget={agent.monthly} kind="monthly" zone={data.zone.name} />
								{:else}
									<p class="text-xs/5 text-fg-muted min-[1024px]:pt-0.5">No monthly budget</p>
								{/if}
							</li>
						{/each}
					</ul>
				{/if}
				{#if withoutBudget.length > 0}
					<div class="px-4 pt-4 min-[640px]:px-6 min-[1024px]:px-8" data-testid="no-budget">
						<p class="text-sm/6 text-fg-secondary">
							No budget set: {#each withoutBudget as agent, i (agent.memberId)}<a
									href={resolve(memberHref(agent.memberId) as '/')}
									class="inline-flex min-h-6 items-center rounded-sm font-semibold text-fg hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
									>{agent.name}</a
								>{i < withoutBudget.length - 1 ? ', ' : ''}{/each}
						</p>
					</div>
				{/if}
			{/if}
		{/if}
	</div>
</section>
