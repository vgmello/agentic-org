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
	import { agentSetupHref } from '$lib/modules/agent-setup';
	import DescriptionList, { type DescriptionItem } from '$lib/components/DescriptionList.svelte';
	import Section from '$lib/components/Section.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { Button } from '$lib/components/ui/button';
	import { reportedText, turnLine, whenText } from '../../present';
	import type { Supervision } from '../../types';
	import AllowHiring from '../allow-hiring/AllowHiring.svelte';
	import { profileContext } from '../profile/context.svelte';
	import SignInAccess from '../sign-in-access/SignInAccess.svelte';

	const ctx = profileContext();
	const p = $derived(ctx.profile);
	const active = $derived(p.status !== 'inactive');

	const HEALTH: Record<Supervision['tone'], { word: string; class: string }> = {
		ok: { word: 'Healthy', class: 't-green' },
		waiting: { word: 'Waiting', class: tone.neutral },
		problem: { word: 'Problem', class: 't-red' },
		unknown: { word: 'Unknown', class: tone.neutral }
	};
	const linkClass =
		'rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus';
	const STALE_MS = 5 * 60_000;
	const stale = (iso: string | undefined) => !iso || Date.now() - Date.parse(iso) > STALE_MS;

	// What the header doesn't already say (handle, kind, home team and reports to are there): no fact twice.
	const details = $derived.by(() => {
		const items: DescriptionItem[] = [];
		if (p.title && p.title !== p.role) items.push({ term: 'Title', value: p.title });
		if (p.focus) items.push({ term: 'Focus', value: p.focus });
		if (p.kind === 'person' && !p.contact) items.push({ term: 'Contact', value: 'None recorded' });
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

	/** The Health tone word, unless the health text already starts with it ("Waiting for an answer…"). */
	const healthWord = (s: Supervision) => {
		const word = HEALTH[s.tone].word;
		return s.health.toLowerCase().startsWith(word.toLowerCase()) ? undefined : word;
	};

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

{#snippet allowHiring()}
	<AllowHiring organizationId={ctx.organizationId} profile={p} onchanged={ctx.changed} />
{/snippet}
{#snippet turn()}
	{@const e = p.execution!}
	{turnLine(e.turnState, p.openTurn)}
{/snippet}
{#snippet office()}
	{@const o = p.execution?.office}
	{#if o}
		{o.name} · placement generation {o.generation} · {reportedText(o.observedAt)}
		{#if stale(o.observedAt)}<span class="block text-xs/5 text-fg-muted"
				>Last observed state: the office hasn't reported recently, so this may be out of date.</span
			>{/if}
	{:else if p.office === null}Unplaced{:else}Not reported{/if}
{/snippet}
{#snippet health()}
	{#if p.supervision}
		{@const word = healthWord(p.supervision)}
		<span class="flex flex-wrap items-center gap-x-2 gap-y-1">
			{#if word}<StatusPill label={word} class={HEALTH[p.supervision.tone].class} />{/if}
			<span class="text-fg-secondary [overflow-wrap:anywhere]">{p.supervision.health}</span>
		</span>
	{:else}Not reported by the office{/if}
{/snippet}
{#snippet holds()}
	{#if p.supervision?.holds.length}
		<ul role="list" class="flex flex-col gap-1">
			{#each p.supervision.holds as hold (hold)}<li class="text-fg-secondary">{hold}</li>{/each}
		</ul>
	{:else}None{/if}
{/snippet}
{#snippet readiness()}
	{@const r = p.execution!.readiness}
	{#if !r.known}Runtime readiness unknown
	{:else if r.reasons.length === 0}Ready to run
	{:else}
		<ul role="list" class="flex flex-col gap-1">
			{#each r.reasons as reason (reason)}<li class="text-fg-secondary">{reason}</li>{/each}
		</ul>
	{/if}
{/snippet}

<Section level={3} title="Current work">
	<div class="px-4 text-sm/6 min-[640px]:px-6 min-[1024px]:px-8">
		{#if p.currentTask}
			<p class="text-fg-secondary [overflow-wrap:anywhere]">
				Working on
				<a href={resolve(`/work/${encodeURIComponent(p.currentTask.ref)}` as '/')} class={linkClass}
					><span class="font-mono font-normal">{p.currentTask.ref}</span> {p.currentTask.title}</a
				>{p.currentTask.project ? ` · ${p.currentTask.project}` : ''}
			</p>
		{:else}
			<p class="text-fg-muted">Not working on anything right now.</p>
		{/if}
	</div>
</Section>

{#if p.kind === 'agent' && active}
	{@const s = p.supervision}
	<Section
		level={3}
		title="Supervision"
		description="What the office observes, outside the model. Monitoring an idle or paused agent uses no turns."
		class="border-t border-line-subtle"
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
		class="border-t border-line-subtle"
	>
		{#snippet actions()}
			<a href={resolve(agentSetupHref(p.id) as '/')} class="{linkClass} text-sm/6"
				>Open Agent setup</a
			>
		{/snippet}
		<DescriptionList items={execution} />
	</Section>
{/if}

<Section level={3} title="Details" class="border-t border-line-subtle">
	<DescriptionList items={details} />
</Section>

{#if p.kind === 'person'}
	<div class="border-t border-line-subtle">
		<SignInAccess
			organizationId={ctx.organizationId}
			organizationName={ctx.organizationName}
			profile={p}
			onchanged={ctx.changed}
		/>
	</div>
{/if}
