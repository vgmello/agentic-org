<!--
  @component
  A project's Overview (work-items-ux §3, PRJ-2): its figures (open and blocked tasks, done of total, open bugs by
  severity with Not set, features by status, and its investment to the viewers allowed amounts), the development
  process it follows, the living Brief, and the five latest activity entries with All activity. Nothing is inferred:
  no lead, health or due date is shown without data behind it. Each part says honestly when it is empty.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Loaded from '$lib/components/Loaded.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { ActivityRows } from '$lib/modules/members';
	import { devProcessHref } from '$lib/modules/processes';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { formatDate } from '$lib/modules/preferences/time';
	import { projectsApi } from '../../api';
	import { investmentResource, investmentWords } from '../../investment';
	import {
		bugParts,
		countedTasks,
		featuresText,
		keepWords,
		openBugs,
		openTasks,
		progressPercent,
		projectBarTone,
		projectHref,
		sortActivityNewestFirst,
		type ProjectBarTone
	} from '../../present';
	import { inlineLink } from '../../components/classes';
	import { projectFrame } from '../frame/context';
	import SeverityMark from '../../components/SeverityMark.svelte';
	import type { BugCounts } from '../../types';
	import BriefText from './BriefText.svelte';

	const frame = projectFrame();
	const p = $derived(frame.project);

	const activity = new Resource(() => projectsApi.activity(frame.organizationId, frame.projectId));
	const investment = investmentResource(() => frame.organizationId);
	const amounts = $derived(investment.data?.allowed ? investment.data.value : undefined);
	// A project manager sees the amounts of the projects they lead and of no other (costs §1 "Who sees amounts").
	const money = $derived(
		amounts && (amounts.scope === 'organization' || amounts.byProject.has(p.id))
			? investmentWords(amounts.byProject.get(p.id), amounts.coverage)
			: undefined
	);
	const bugs = $derived(bugParts(p.bugs));
	const totalTasks = $derived(countedTasks(p.tasks));
	const pct = $derived(progressPercent(p.tasks));
	const barTone = $derived(projectBarTone(p.tasks, p.bugs, p.archived));
	const barFill: Record<ProjectBarTone, string> = {
		ok: 'var(--tone-ok-bar)',
		warn: 'var(--tone-warn-bar)',
		bad: 'var(--tone-bad-bar)',
		accent: 'var(--tone-accent-bar)',
		archived: 'var(--quiet)'
	};

	const h4 = 'text-[11px]/5 font-semibold tracking-wider uppercase text-fg-muted';
	/** Read between a figure's qualifiers, which are shown apart by space alone. */
	const separator = ', ';
</script>

<!-- Each figure: what it counts, the number, then what qualifies it, words kept whole so no separator strands. -->
{#snippet figure(
	label: string,
	value: string,
	parts: { text: string; severity?: keyof BugCounts; warn?: boolean }[],
	bar?: { pct: number; fill: string }
)}
	<div class="ov-figure flex flex-col gap-1">
		<dt class="text-[11px]/5 font-semibold tracking-wider text-fg-muted uppercase">{label}</dt>
		<dd class="text-2xl/8 font-bold tracking-tight text-fg tabular-nums">{value}</dd>
		{#if bar}
			<dd class="mt-1 h-1.5 w-full overflow-hidden rounded-full bg-bar-track" aria-hidden="true">
				<span
					class="block h-full rounded-full transition-[width] duration-300 motion-reduce:transition-none"
					style:width="{bar.pct}%"
					style:background={bar.fill}
				></span>
			</dd>
		{/if}
		<dd class="mt-1 flex flex-wrap gap-1.5 text-xs/5 text-fg-muted">
			{#each parts as part, i (part.text)}<span
					class={[
						'whitespace-nowrap',
						part.warn &&
							't-amber inline-flex items-center rounded-full px-1.5 py-0.5 text-xs/4 font-medium'
					]}
					>{#if part.severity}<SeverityMark
							severity={part.severity}
							text={part.text}
						/>{:else}{part.text}{/if}{#if i < parts.length - 1}<span class="sr-only"
							>{separator}</span
						>{/if}</span
				>{/each}
		</dd>
	</div>
{/snippet}

<div class="flex flex-col gap-5 px-4 py-5 min-[640px]:px-6 min-[1024px]:px-8">
	<dl
		class={[
			'ov-figures grid grid-cols-1 gap-3 min-[640px]:grid-cols-2',
			money ? 'min-[1280px]:grid-cols-5' : 'min-[1280px]:grid-cols-4'
		]}
		data-testid="project-figures"
	>
		{@render figure('Open tasks', String(openTasks(p.tasks)), [
			{
				text: p.tasks.blocked > 0 ? `${p.tasks.blocked} blocked` : 'None blocked',
				warn: p.tasks.blocked > 0
			}
		])}
		{@render figure(
			'Done',
			String(p.tasks.done),
			[
				{
					text:
						totalTasks === 0
							? 'No tasks yet'
							: `of ${totalTasks} ${totalTasks === 1 ? 'task' : 'tasks'}`
				}
			],
			totalTasks > 0 ? { pct, fill: barFill[barTone] } : undefined
		)}
		{@render figure(
			'Open bugs',
			String(openBugs(p.bugs)),
			bugs.length ? bugs.map((b) => ({ text: b.text, severity: b.key })) : [{ text: 'None open' }]
		)}
		<!-- The features line, word for word (features-ux "Overview tile"). -->
		<div class="ov-figure flex flex-col gap-1" data-testid="features-line">
			<dt class="text-[11px]/5 font-semibold tracking-wider text-fg-muted uppercase">
				Features<span class="sr-only">:</span>
			</dt>
			<dd class="mt-0.5 text-base/7 font-semibold text-fg [overflow-wrap:anywhere]">
				{keepWords(featuresText(p.features))}
			</dd>
		</div>
		{#if money}
			{@render figure(
				'Investment',
				money.toDate,
				money.thisMonth ? [{ text: money.thisMonth }] : []
			)}
		{/if}
	</dl>

	{#if money?.note}
		<p class="text-xs/5 text-fg-muted">
			Investment: {money.detail ?? money.note}
		</p>
	{:else if investment.state === 'error'}
		<p class="text-xs/5 text-fg-muted" role="status">
			Couldn't load the investment: {investment.message}
			<button
				type="button"
				class="{inlineLink.xs} font-semibold"
				disabled={investment.reloading}
				onclick={() => investment.reload()}>{investment.reloading ? 'Retrying…' : 'Retry'}</button
			>
		</p>
	{/if}

	<div class="grid grid-cols-1 gap-5 min-[1280px]:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
		<section aria-labelledby="brief-heading" class="exec-card p-5">
			<h4 id="brief-heading" class={h4}>Brief</h4>
			{#if p.brief && p.brief.markdown.trim()}
				<div class="mt-3"><BriefText markdown={p.brief.markdown} /></div>
				{#if p.brief.updatedAt}
					<p class="mt-4 text-xs/5 text-fg-muted">
						Updated <time datetime={p.brief.updatedAt}
							>{formatDate(new Date(p.brief.updatedAt), accountZone.zone, new Date())}</time
						>{p.brief.updatedBy ? ` by ${p.brief.updatedBy}` : ''}
					</p>
				{/if}
			{:else}
				<p class="mt-2 max-w-prose text-sm/6 text-fg-muted" data-testid="no-brief">
					No brief yet. The brief says what {p.name} is for and what it leaves out; write it in
					<a href={resolve(projectHref(p.id, 'docs') as '/')} class={inlineLink.sm}>Docs</a>.
				</p>
			{/if}
		</section>

		<section aria-labelledby="process-heading" class="exec-card p-5">
			<h4 id="process-heading" class={h4}>Development process</h4>
			<p class="mt-2 text-sm/6 text-fg-secondary" data-testid="process-line">
				{#if p.developmentProcess}
					Follows <a
						href={resolve(devProcessHref(p.developmentProcess.id) as '/')}
						class={inlineLink.sm}>{p.developmentProcess.name}</a
					>.
				{:else}
					None. Its tabs are the fixed ones.
				{/if}
				{#if p.viewer.canEdit}
					<a href={resolve(projectHref(p.id, 'settings') as '/')} class={inlineLink.sm}
						>{p.developmentProcess ? 'Change' : 'Choose one'} in Settings</a
					>
				{/if}
			</p>
		</section>
	</div>

	<section aria-labelledby="latest-heading" class="exec-card overflow-hidden">
		<div
			class="flex flex-wrap items-baseline justify-between gap-x-4 border-b border-line px-5 py-3.5"
		>
			<h4 id="latest-heading" class={h4}>Latest activity</h4>
			<a href={resolve(projectHref(p.id, 'activity') as '/')} class="{inlineLink.sm} text-sm/6"
				>All activity</a
			>
		</div>
		<Loaded
			resource={activity}
			label="the latest activity"
			error="Couldn't load the latest activity"
			errorLevel={4}
			rows={3}
		>
			{#snippet children(rows)}
				{#if rows.length === 0}
					<p class="px-5 py-6 text-sm/6 text-fg-muted">No activity yet.</p>
				{:else}
					<ActivityRows rows={sortActivityNewestFirst(rows).slice(0, 5)} />
				{/if}
			{/snippet}
		</Loaded>
	</section>
</div>

<style>
	.ov-figure {
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: 1rem 1.125rem;
		min-width: 0;
	}
	:global(html[data-ui-theme^='report']) .ov-figures {
		gap: 0;
		border-bottom: 1px solid var(--report-rule);
		padding-bottom: 1rem;
	}
	:global(html[data-ui-theme^='report']) .ov-figure {
		padding: 0.625rem 0;
	}
	:global(html[data-ui-theme^='report']) .ov-figure + .ov-figure {
		border-top: 1px solid var(--report-rule-subtle);
	}
	@media (min-width: 1280px) {
		:global(html[data-ui-theme^='report']) .ov-figure {
			padding: 0.5rem 1.25rem;
			border-top: 0;
			border-left: 1px solid var(--report-rule-subtle);
		}
		:global(html[data-ui-theme^='report']) .ov-figure:first-child {
			border-left: 0;
			padding-left: 0;
		}
		:global(html[data-ui-theme^='report']) .ov-figure + .ov-figure {
			border-top: 0;
		}
	}
</style>
