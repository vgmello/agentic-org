<!--
  @component
  Profile › Overview (our-org-in-the-app §5, PROF-1; employee-supervision-ux §1; agent-run-modes-ux §1). For everyone:
  the current work ("Not working on anything right now." without), then the employee record. For an agent also the
  Supervision line (health in words and times, every unresolved hold, stop pending) and the execution view (requested
  pool and model, the actual connection, the turn, the office with its generation and freshness, readiness reasons,
  the managed home) with Open Agent setup. For a person, sign-in access.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import DescriptionList, { type DescriptionItem } from '$lib/components/DescriptionList.svelte';
	import Section from '$lib/components/Section.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { Button } from '$lib/components/ui/button';
	import { TURN_STATE, ageText, kindWord, whenText } from '../../present';
	import type { Supervision } from '../../types';
	import AllowHiring from '../allow-hiring/AllowHiring.svelte';
	import { profileContext } from '../profile/context.svelte';
	import SignInAccess from '../sign-in-access/SignInAccess.svelte';

	const ctx = profileContext();
	const p = $derived(ctx.profile);
	const active = $derived(p.status !== 'inactive');

	const HEALTH: Record<Supervision['tone'], { word: string; class: string }> = {
		ok: { word: 'Healthy', class: tone.ok },
		waiting: { word: 'Waiting', class: tone.neutral },
		problem: { word: 'Problem', class: tone.bad },
		unknown: { word: 'Unknown', class: tone.neutral }
	};
	const linkClass =
		'rounded-sm font-semibold text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400';
	const STALE_MS = 5 * 60_000;
	const stale = (iso: string | undefined) => !iso || Date.now() - Date.parse(iso) > STALE_MS;

	const details = $derived.by(() => {
		const items: DescriptionItem[] = [
			{ term: 'Handle', value: `@${p.handle}`, mono: true },
			{ term: 'Kind', value: kindWord(p.kind) }
		];
		if (p.title && p.title !== p.role) items.push({ term: 'Title', value: p.title });
		if (p.focus) items.push({ term: 'Focus', value: p.focus });
		if (p.kind === 'person') items.push({ term: 'Contact', value: p.contact || 'None recorded' });
		items.push({ term: 'Home team', content: homeTeam });
		items.push({ term: 'Reports to', content: reportsTo });
		if (p.leads.length) items.push({ term: 'Leads', value: p.leads.map((t) => t.name).join(', ') });
		if (p.directReports)
			items.push({
				term: 'Direct reports',
				value: `${p.directReports} ${p.directReports === 1 ? 'person or agent' : 'people and agents'}`
			});
		if (p.hired)
			items.push({
				term: 'Hired',
				value: `${whenText(p.hired.at)}${p.hired.by ? ` · hired by ${p.hired.by.name}` : ''}`
			});
		if (!active && p.inactiveSince)
			items.push({ term: 'Former member since', value: whenText(p.inactiveSince) });
		items.push({ term: 'Allow hiring', content: allowHiring });
		return items;
	});

	const execution = $derived.by((): DescriptionItem[] => {
		const e = p.execution;
		if (!e) return [{ term: 'Execution', value: 'Execution not configured' }];
		return [
			{
				term: 'Requested',
				value: e.requested
					? `${e.requested.model} · pool ${e.requested.pool}`
					: 'Execution not configured'
			},
			{ term: 'Connection', value: e.connection ?? 'No attempt yet' },
			{ term: 'Turn', content: turn },
			{ term: 'Office', content: office },
			{ term: 'Readiness', content: readiness },
			{ term: 'Managed home', value: e.home ?? 'Not reported by the office', mono: !!e.home }
		];
	});
</script>

{#snippet homeTeam()}
	{#if p.homeTeam}
		<a href={resolve(`/teams/${encodeURIComponent(p.homeTeam.id)}` as '/')} class={linkClass}
			>{p.homeTeam.name}</a
		>{p.homeTeam.ref ? ` · ${p.homeTeam.ref}` : ''}
	{:else if !active}
		None: former member
	{:else}
		<span class="text-red-300"
			>None on record. Every employee needs one home team; this record is invalid.</span
		>
	{/if}
{/snippet}
{#snippet reportsTo()}
	{#if p.reportsTo}
		<a href={resolve(`/members/${encodeURIComponent(p.reportsTo.id)}` as '/')} class={linkClass}
			>{p.reportsTo.name}</a
		>
	{:else}No explicit manager{/if}
{/snippet}
{#snippet allowHiring()}
	<AllowHiring organizationId={ctx.organizationId} profile={p} onchanged={ctx.changed} />
{/snippet}
{#snippet turn()}
	{@const e = p.execution!}
	{TURN_STATE[e.turnState]}{#if p.openTurn}
		· started {ageText(p.openTurn.startedAt)}{/if}
{/snippet}
{#snippet office()}
	{@const o = p.execution?.office}
	{#if o}
		{o.name} · placement generation {o.generation} · reported {ageText(o.observedAt)}
		{#if stale(o.observedAt)}<span class="block text-xs/5 text-gray-400"
				>Last observed state: the office hasn't reported recently, so this may be out of date.</span
			>{/if}
	{:else if p.office === null}Unplaced{:else}Not reported{/if}
{/snippet}
{#snippet health()}
	{#if p.supervision}
		<span class="flex flex-wrap items-center gap-x-2 gap-y-1">
			<StatusPill
				label={HEALTH[p.supervision.tone].word}
				class={HEALTH[p.supervision.tone].class}
			/>
			<span class="text-gray-300">{p.supervision.health}</span>
		</span>
	{:else}Status unknown{/if}
{/snippet}
{#snippet holds()}
	{#if p.supervision?.holds.length}
		<ul role="list" class="space-y-1">
			{#each p.supervision.holds as hold (hold)}<li class="text-gray-300">{hold}</li>{/each}
		</ul>
	{:else}None{/if}
{/snippet}
{#snippet readiness()}
	{@const r = p.execution!.readiness}
	{#if !r.known}Runtime readiness unknown
	{:else if r.reasons.length === 0}Ready to run
	{:else}
		<ul role="list" class="space-y-1">
			{#each r.reasons as reason (reason)}<li class="text-gray-300">{reason}</li>{/each}
		</ul>
	{/if}
{/snippet}

<Section level={3} title="Current work">
	<div class="px-4 text-sm/6 min-[640px]:px-6 min-[1024px]:px-8">
		{#if p.currentTask}
			<p class="text-gray-300 [overflow-wrap:anywhere]">
				Working on
				<a href={resolve(`/work/${encodeURIComponent(p.currentTask.ref)}` as '/')} class={linkClass}
					><span class="font-mono font-normal">{p.currentTask.ref}</span> {p.currentTask.title}</a
				>{p.currentTask.project ? ` · ${p.currentTask.project}` : ''}
			</p>
		{:else}
			<p class="text-gray-400">Not working on anything right now.</p>
		{/if}
	</div>
</Section>

{#if p.kind === 'agent' && active}
	{@const s = p.supervision}
	<Section
		level={3}
		title="Supervision"
		description="What the office observes, outside the model. Monitoring an idle or paused agent uses no turns."
		class="border-t border-white/5"
	>
		{#snippet actions()}
			{#if p.viewer.isOwnerOrAdmin && ctx.snapshot.members.some((m) => m.id === p.id)}
				{#if p.status === 'paused'}
					<Button
						variant="outline"
						class={mobileButton}
						data-lifecycle-control
						onclick={(e) => ctx.act('resume', e.currentTarget)}>Resume</Button
					>
				{:else}
					<Button
						variant="outline"
						class={mobileButton}
						data-lifecycle-control
						onclick={(e) => ctx.act('pause', e.currentTarget)}>Pause…</Button
					>
				{/if}
			{/if}
		{/snippet}
		<DescriptionList
			items={[
				{ term: 'Health', content: health },
				{ term: 'Holds', content: holds },
				...(s?.stopPending
					? [
							{
								term: 'Stop',
								value:
									"Stop pending: HQ accepted the pause; the office hasn't confirmed the process stopped yet."
							}
						]
					: [])
			]}
		/>
	</Section>

	<Section
		level={3}
		title="How it runs"
		description="Separate from who it is. Configuration lives in Agent setup."
		class="border-t border-white/5"
	>
		{#snippet actions()}
			<a href={resolve('/settings/agents')} class="{linkClass} text-sm/6">Open Agent setup</a>
		{/snippet}
		<DescriptionList items={execution} />
	</Section>
{/if}

<Section level={3} title="Details" class="border-t border-white/5">
	<DescriptionList items={details} />
</Section>

{#if p.kind === 'person'}
	<div class="border-t border-white/5">
		<SignInAccess
			organizationId={ctx.organizationId}
			organizationName={ctx.organizationName}
			profile={p}
			onchanged={ctx.changed}
		/>
	</div>
{/if}
