<!--
  @component
  Organization settings › Agent setup (our-org-in-the-app §5, AC-IA07-1): every current agent with its configured
  runtime, model and model profile, its office and one Health word, A–Z with the built-in HR lead first. From 640 px
  the facts sit in aligned columns; below, each agent is a divided row. A name opens that agent's setup. Hire an
  agent (the one primary action) opens the existing hire flow at `hireHref`, which the route supplies. States: loading, unavailable, error with Retry, no
  agents.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Loaded from '$lib/components/Loaded.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { agentSetupApi } from '../../api';
	import HealthBadge from '../../components/HealthBadge.svelte';
	import { health, healthText, needsSetupCount } from '../../health';
	import {
		agentsText,
		isStale,
		listDescription,
		officeText,
		reportedText,
		runtimeText,
		sortAgents
	} from '../../present';
	import { agentSetupHref } from '../../urls';

	let {
		organizationId,
		hireHref
	}: {
		organizationId: string;
		/**
		 * Where Hire an agent goes: the existing reviewed hire flow, which agent-setup doesn't own. The route supplies it,
		 * so this module depends on no module that depends on members (no cycle: members links here).
		 */
		hireHref: string;
	} = $props();

	const resource = new Resource(() => agentSetupApi.list(organizationId));
	const agents = $derived(resource.data ? sortAgents(resource.data.agents) : []);
	const healthSummary = $derived.by(() => {
		let ready = 0;
		let setup = 0;
		let paused = 0;
		let unknown = 0;
		for (const agent of agents) {
			const kind = health(agent).kind;
			if (kind === 'ready') ready++;
			else if (kind === 'needs_setup') setup++;
			else if (kind === 'paused') paused++;
			else unknown++;
		}
		return { ready, setup, paused, unknown };
	});
	const description = $derived(
		listDescription(agents.length, resource.data ? needsSetupCount(agents) : 0)
	);
	/** When the shown answer arrived: ages and the stale flag follow each reload. */
	const now = $derived(resource.data ? Date.now() : 0);
	const cell = 'min-w-0 text-sm/6';
	/** After the handle: Built-in and the role, each after a separator, as one string so the spaces stay. */
	const metaText = (a: (typeof agents)[number]) =>
		[a.builtIn ? 'Built-in' : '', a.role ?? a.roleText ?? '']
			.filter(Boolean)
			.map((fact) => ` · ${fact}`)
			.join('');
</script>

{#snippet hire()}
	<Button
		href={resolve(hireHref as '/')}
		class="min-h-9 {mobileButton} [@media(pointer:coarse)]:min-h-11">Hire an agent</Button
	>
{/snippet}

<div class="agents-masthead">
	<PageHeader
		id="agent-setup-heading"
		title="Agent setup"
		count={resource.data ? agents.length : undefined}
		countLabel={agentsText(agents.length)}
		{description}
		actions={hire}
	/>
</div>

<Loaded
	{resource}
	label="agent setup"
	error="Couldn't load agent setup"
	place="Agent setup"
	area="Organization settings"
	isEmpty={(data) => data.agents.length === 0}
	emptyTitle="No agents yet"
	emptyDescription="Agents come only from a hire. Once one is hired, how it runs shows here."
	emptyAction={hire}
	rows={6}
>
	<div class="flex flex-col gap-5 px-4 py-5 min-[640px]:px-6 min-[1024px]:px-8">
		<section
			aria-label="Agent readiness summary"
			class="agent-kpi-strip grid grid-cols-1 gap-3 min-[640px]:grid-cols-2 min-[1024px]:grid-cols-4"
		>
			<div class="agent-kpi-tile flex flex-col gap-1">
				<span class="agent-kpi-label inline-flex items-center gap-1.5">
					<span class="agent-dot dot-ok" aria-hidden="true"></span>Ready
				</span>
				<p class="agent-kpi-num">{healthSummary.ready}</p>
				<p class="text-xs/5 text-fg-muted">Runnable on next turn</p>
			</div>
			<div class="agent-kpi-tile flex flex-col gap-1">
				<div class="flex items-center justify-between gap-2">
					<span class="agent-kpi-label inline-flex items-center gap-1.5">
						<span class="agent-dot dot-warn" aria-hidden="true"></span>Needs setup
					</span>
					{#if healthSummary.setup > 0}
						<span
							class="t-amber inline-flex items-center rounded-full px-2 py-0.5 text-[11px]/4 font-semibold"
							>Action</span
						>
					{/if}
				</div>
				<p class="agent-kpi-num">{healthSummary.setup}</p>
				<p class="text-xs/5 text-fg-muted">Missing config or office</p>
			</div>
			<div class="agent-kpi-tile flex flex-col gap-1">
				<span class="agent-kpi-label inline-flex items-center gap-1.5">
					<span class="agent-dot dot-muted" aria-hidden="true"></span>Paused
				</span>
				<p class="agent-kpi-num">{healthSummary.paused}</p>
				<p class="text-xs/5 text-fg-muted">Admissions held</p>
			</div>
			<div class="agent-kpi-tile flex flex-col gap-1">
				<span class="agent-kpi-label inline-flex items-center gap-1.5">
					<span class="agent-dot dot-accent" aria-hidden="true"></span>Unverified
				</span>
				<p class="agent-kpi-num">{healthSummary.unknown}</p>
				<p class="text-xs/5 text-fg-muted">Readiness not reported</p>
			</div>
		</section>

		<!-- The columns follow the list's own width: Organization settings' side column narrows it from 960 px. -->
		<div class="exec-card @container overflow-hidden">
			<div
				class="hidden grid-cols-[minmax(0,5fr)_minmax(0,6fr)_minmax(10rem,5fr)] gap-x-4 border-b border-line px-4 py-3 text-[11px]/5 font-semibold tracking-wider text-fg-muted uppercase min-[640px]:grid min-[640px]:px-6 @4xl:grid-cols-[minmax(0,5fr)_minmax(0,6fr)_minmax(0,4fr)_minmax(10rem,5fr)]"
				aria-hidden="true"
			>
				<span>Agent</span>
				<span>Runs on<span class="@4xl:hidden">&nbsp;and office</span></span>
				<span class="hidden @4xl:block">Office</span>
				<span>Health</span>
			</div>
			<ul role="list" class="divide-y divide-line-subtle" aria-labelledby="agent-setup-heading">
				{#each agents as agent (agent.memberId)}
					{@const h = health(agent)}
					{@const reported = reportedText(agent.placement, now)}
					<li
						class="relative grid grid-cols-1 gap-y-2 px-4 py-4 transition-colors duration-150 hover:bg-surface-raised/60 motion-reduce:transition-none min-[640px]:grid-cols-[minmax(0,5fr)_minmax(0,6fr)_minmax(10rem,5fr)] min-[640px]:gap-x-4 min-[640px]:px-6 @4xl:grid-cols-[minmax(0,5fr)_minmax(0,6fr)_minmax(0,4fr)_minmax(10rem,5fr)]"
						data-agent={agent.memberId}
					>
						<div class="{cell} min-[640px]:row-span-2 @4xl:row-span-1">
							<p class="font-semibold text-fg">
								<a
									href={resolve(agentSetupHref(agent.memberId) as '/')}
									class="rounded-sm [overflow-wrap:anywhere] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus"
									><span class="absolute inset-0" aria-hidden="true"></span>{agent.name}</a
								>
							</p>
							<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
								<span class="font-mono">@{agent.handle}</span>
								{metaText(agent)}
							</p>
						</div>
						<div class={cell}>
							<span class="sr-only">Runs on: </span>
							{#if agent.execution}
								<p class="text-fg-secondary [overflow-wrap:anywhere]">
									{runtimeText(agent.execution)} · {agent.execution.modelId}
								</p>
								<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
									{agent.execution.profile.name}
								</p>
							{:else}
								<p class="text-fg-muted">Execution not configured</p>
							{/if}
						</div>
						<div class="{cell} min-[640px]:col-start-2 @4xl:col-start-auto">
							<span class="sr-only">Office: </span>
							{#if agent.placement}
								<!-- One line below 640 px, two from 640 px where it has its own column. -->
								<p class="[overflow-wrap:anywhere]">
									<span class="text-fg-secondary">{officeText(agent.placement)}</span><span
										class="text-fg-muted min-[640px]:hidden"
										aria-hidden="true">&nbsp;·&nbsp;</span
									><span class="text-xs/5 text-fg-muted min-[640px]:block"
										>{reported}{#if isStale(agent.placement.observedAt, now)}<span class="sr-only"
												>, may be out of date</span
											>{/if}</span
									>
								</p>
							{:else}
								<!-- Health carries the warning (amber, with the reason); the office cell says it in words only. -->
								<p class="font-medium text-fg-secondary">Unplaced</p>
							{/if}
						</div>
						<div
							class="{cell} flex flex-col items-start gap-y-1 min-[640px]:col-start-3 min-[640px]:row-span-2 min-[640px]:row-start-1 @4xl:col-start-auto @4xl:row-span-1 @4xl:row-start-auto"
						>
							<span class="sr-only">Health: {healthText(h)}</span>
							<HealthBadge health={h} aria-hidden="true" />
							{#if h.kind === 'needs_setup'}
								<p class="text-xs/5 text-fg-secondary [overflow-wrap:anywhere]" aria-hidden="true">
									{h.reasons[0].text}{#if h.reasons.length > 1}<span class="text-fg-muted"
											>&nbsp;(and {h.reasons.length - 1} more)</span
										>{/if}
								</p>
							{/if}
						</div>
					</li>
				{/each}
			</ul>
		</div>
	</div>
</Loaded>

<style>
	.agents-masthead :global(h2) {
		font-size: 1.25rem;
		line-height: 1.75rem;
		letter-spacing: -0.02em;
	}
	:global(html[data-ui-theme^='report']) .agents-masthead :global(header) {
		border-bottom: 1.5px solid var(--report-masthead-rule);
	}
	.agent-kpi-tile {
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: 1rem 1.125rem;
		min-width: 0;
	}
	.agent-kpi-label {
		font-size: 0.7rem;
		font-weight: 650;
		letter-spacing: 0.09em;
		text-transform: uppercase;
		color: var(--exec-muted);
	}
	.agent-kpi-num {
		font-size: 1.5rem;
		line-height: 1.15;
		font-weight: 700;
		letter-spacing: -0.03em;
		color: var(--exec-ink);
		font-variant-numeric: tabular-nums;
	}
	.agent-dot {
		width: 0.4375rem;
		height: 0.4375rem;
		border-radius: 999px;
		flex: none;
	}
	.dot-ok {
		background: var(--tone-ok);
	}
	.dot-warn {
		background: var(--tone-warn);
	}
	.dot-accent {
		background: var(--tone-accent);
	}
	.dot-muted {
		background: var(--fg-muted);
	}
	:global(html[data-ui-theme^='report']) .agent-kpi-strip {
		gap: 0;
		border-bottom: 1px solid var(--report-rule);
		padding-bottom: 0.75rem;
	}
	:global(html[data-ui-theme^='report']) .agent-kpi-tile {
		padding: 0.625rem 0;
	}
	:global(html[data-ui-theme^='report']) .agent-kpi-tile + .agent-kpi-tile {
		border-top: 1px solid var(--report-rule-subtle);
	}
	@media (min-width: 1024px) {
		:global(html[data-ui-theme^='report']) .agent-kpi-tile {
			padding: 0.5rem 1.25rem;
			border-top: 0;
			border-left: 1px solid var(--report-rule-subtle);
		}
		:global(html[data-ui-theme^='report']) .agent-kpi-tile:first-child {
			border-left: 0;
			padding-left: 0;
		}
		:global(html[data-ui-theme^='report']) .agent-kpi-tile + .agent-kpi-tile {
			border-top: 0;
		}
	}
</style>
