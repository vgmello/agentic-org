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
		projectHref
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

	const h4 = 'text-sm/6 font-semibold text-fg';
	/** Read between a figure's qualifiers, which are shown apart by space alone. */
	const separator = ', ';
</script>

<!-- Each figure: what it counts, the number, then what qualifies it, words kept whole so no separator strands. -->
{#snippet figure(
	label: string,
	value: string,
	parts: { text: string; severity?: keyof BugCounts }[]
)}
	<div class="bg-surface px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8">
		<dt class="text-sm/6 font-medium text-fg-muted">{label}</dt>
		<dd class="mt-1 text-2xl/8 font-semibold tracking-tight text-fg tabular-nums">{value}</dd>
		<dd class="mt-1 flex flex-wrap gap-x-2 text-xs/5 text-fg-muted">
			{#each parts as part, i (part.text)}<span class="whitespace-nowrap"
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

<dl
	class="grid grid-cols-1 gap-px border-b border-line-subtle bg-line-subtle min-[640px]:grid-cols-2 {money
		? 'min-[1280px]:grid-cols-5'
		: 'min-[1280px]:grid-cols-4'}"
	data-testid="project-figures"
>
	{@render figure('Open tasks', String(openTasks(p.tasks)), [
		{ text: p.tasks.blocked > 0 ? `${p.tasks.blocked} blocked` : 'None blocked' }
	])}
	{@render figure('Done', String(p.tasks.done), [
		{
			text:
				countedTasks(p.tasks) === 0
					? 'No tasks yet'
					: `of ${countedTasks(p.tasks)} ${countedTasks(p.tasks) === 1 ? 'task' : 'tasks'}`
		}
	])}
	{@render figure(
		'Open bugs',
		String(openBugs(p.bugs)),
		bugs.length ? bugs.map((b) => ({ text: b.text, severity: b.key })) : [{ text: 'None open' }]
	)}
	<!-- The features line, word for word (features-ux "Overview tile"). -->
	<div class="bg-surface px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8" data-testid="features-line">
		<dt class="text-sm/6 font-medium text-fg-muted">Features<span class="sr-only">:</span></dt>
		<dd class="mt-1 text-base/7 font-semibold text-fg">{keepWords(featuresText(p.features))}</dd>
	</div>
	{#if money}
		{@render figure('Investment', money.toDate, money.thisMonth ? [{ text: money.thisMonth }] : [])}
	{/if}
</dl>
{#if money?.note}
	<p class="px-4 pt-3 text-xs/5 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
		Investment: {money.detail ?? money.note}
	</p>
{:else if investment.state === 'error'}
	<p class="px-4 pt-3 text-xs/5 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8" role="status">
		Couldn't load the investment: {investment.message}
		<button
			type="button"
			class="{inlineLink.xs} font-semibold"
			disabled={investment.reloading}
			onclick={() => investment.reload()}>{investment.reloading ? 'Retrying…' : 'Retry'}</button
		>
	</p>
{/if}

<div
	class="grid grid-cols-1 gap-x-12 px-4 min-[640px]:px-6 min-[1024px]:px-8 min-[1280px]:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]"
>
	<section aria-labelledby="brief-heading" class="py-6">
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
			<p class="mt-1 max-w-prose text-sm/6 text-fg-muted" data-testid="no-brief">
				No brief yet. The brief says what {p.name} is for and what it leaves out; it is written in the
				project's Docs, which aren't built yet.
			</p>
		{/if}
	</section>

	<section aria-labelledby="process-heading" class="border-line-subtle py-6 max-[1279px]:border-t">
		<h4 id="process-heading" class={h4}>Development process</h4>
		<p class="mt-1 text-sm/6 text-fg-secondary" data-testid="process-line">
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

<section aria-labelledby="latest-heading" class="border-t border-line-subtle pt-6">
	<div
		class="flex flex-wrap items-baseline justify-between gap-x-4 px-4 min-[640px]:px-6 min-[1024px]:px-8"
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
				<p class="px-4 py-6 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
					No activity yet.
				</p>
			{:else}
				<ActivityRows rows={rows.slice(0, 5)} />
			{/if}
		{/snippet}
	</Loaded>
</section>
