<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import CloudOff from '@lucide/svelte/icons/cloud-off';
	import FileText from '@lucide/svelte/icons/file-text';
	import Lock from '@lucide/svelte/icons/lock';
	import PowerOff from '@lucide/svelte/icons/power-off';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import ProcessBadges, { KIND_ICON } from '$lib/modules/processes/components/ProcessBadges.svelte';
	import { copyTime, hhmm, KIND_WORD, problemWords } from '$lib/modules/processes/present';
	import ProcessSheet from '$lib/modules/processes/features/inspect-effective/ProcessSheet.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { knownPlace, parsePlace, placeKey, viewOf } from '$lib/modules/processes/view';
	import { sampleWorld, type ProcessWorld } from '$lib/modules/processes/world';
	import type { Entry, TurnedOff } from '$lib/modules/processes/types';

	// Until HQ serves processes this screen reads the sample organization's files (Sample data). `?handbook=down` shows the
	// unreachable-handbook state, `?processes=empty` the empty one.
	let world = $state.raw<ProcessWorld | undefined>();
	onMount(() => {
		const timer = setTimeout(() => (world = sampleWorld()), 250);
		return () => clearTimeout(timer);
	});
	const handbookDown = $derived(page.url.searchParams.get('handbook') === 'down');
	const empty = $derived(page.url.searchParams.get('processes') === 'empty');
	// `?processes=copied`: a process file copied with git, so two files share a doc id.
	const copied = $derived(page.url.searchParams.get('processes') === 'copied');

	const asked = $derived(parsePlace(page.url.searchParams.get('place')));
	// A stale or mistyped place falls back to the organization, and says so.
	const resolved = $derived(world ? knownPlace(world, asked) : { place: asked, missing: false });
	const place = $derived(resolved.place);
	const shownWorld = $derived(world ? (empty ? { ...world, docs: [] } : copied ? { ...world, docs: world.docs.map((d) => (d.key === 'standup' ? { ...d, id: 'doc-copied' } : d)) } : world) : undefined);
	const view = $derived(shownWorld ? viewOf(shownWorld, place) : undefined);
	function choose(value: string) {
		const next = new URL(page.url);
		if (value === 'organization') next.searchParams.delete('place');
		else next.searchParams.set('place', value);
		void goto(next, { replaceState: true, keepFocus: true, noScroll: true });
	}

	let sheetOpen = $state(false);
	let shownEntry = $state<Entry | undefined>();
	let shownOff = $state<TurnedOff | undefined>();
	function openEntry(entry: Entry) {
		shownEntry = entry;
		shownOff = undefined;
		sheetOpen = true;
	}
	function openOff(off: TurnedOff) {
		shownEntry = undefined;
		shownOff = off;
		sheetOpen = true;
	}

	const SKELETON = [0, 1, 2, 3];
</script>

<svelte:head><title>Processes · Agentic Org</title></svelte:head>

<div class="min-w-0 space-y-5 px-8 py-6">
	<div class="flex flex-wrap items-start justify-between gap-3">
		<div class="min-w-0 flex-1 space-y-1">
			<h2 class="text-base/7 font-semibold text-white [overflow-wrap:anywhere]">{view?.title ?? 'Processes'}</h2>
			<p class="max-w-2xl text-sm/6 text-gray-400 [overflow-wrap:anywhere]">{view?.subtitle ?? 'How agents work, written down and inherited.'}</p>
			<p class="flex items-center gap-x-2 text-xs/5 text-gray-400" data-testid="sample-data-note"><Badge variant="secondary">Sample data</Badge> An invented organization's processes, not yours.</p>
		</div>
		{#if world}
			<label class="flex min-w-0 max-w-full items-center gap-x-2 text-sm/6 text-gray-300">
				Show
				<select aria-label="Show processes for" value={placeKey(place)} onchange={(e) => choose(e.currentTarget.value)} class="min-w-0 max-w-64 truncate rounded-md bg-gray-800 px-2 py-1.5 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
					<option value="organization">Organization</option>
					{#if world.teams.length > 0}<optgroup label="Team">{#each world.teams as t (t.id)}<option value={`team:${t.id}`}>{t.name}</option>{/each}</optgroup>{/if}
					{#if world.projects.length > 0}<optgroup label="Project">{#each world.projects as p (p.key)}<option value={`project:${p.key}`}>{p.name}</option>{/each}</optgroup>{/if}
					{#if world.agents.length > 0}<optgroup label="Agent">{#each world.agents as a (a.id)}<option value={`agent:${a.id}`}>{a.name}</option>{/each}</optgroup>{/if}
				</select>
			</label>
		{/if}
	</div>

	{#if resolved.missing}
		<p class="text-sm/6 text-amber-200" role="status" data-testid="place-missing">That place isn't here any more. Showing the organization's processes.</p>
	{/if}

	{#if handbookDown && world}
		<p class="flex items-center gap-x-2 rounded-lg bg-amber-400/10 px-3 py-2 text-sm/6 text-amber-200 outline-1 -outline-offset-1 outline-amber-400/20" role="status" data-testid="handbook-down">
			<CloudOff class="size-4 shrink-0" aria-hidden="true" /> Can't reach the handbook. Showing the last copy from {copyTime(world.handbookCopyAt)}.
		</p>
	{/if}

	{#if !view}
		<ul class="space-y-2" aria-label="Loading processes" aria-busy="true" data-testid="processes-loading">
			{#each SKELETON as i (i)}<li class="h-14 animate-pulse rounded-lg bg-white/5"></li>{/each}
		</ul>
	{:else}
		{#if view.rule}
			<p class="text-sm/6 text-gray-300 [overflow-wrap:anywhere]" data-testid="branch-rule">One open branch per agent: <span class="font-medium text-white">{view.rule.text}</span></p>
		{/if}

		{#each view.set.conflicts as c (c.key)}
			<p class="flex items-start gap-x-2 rounded-lg bg-amber-400/10 px-3 py-2 text-sm/6 text-amber-200 outline-1 -outline-offset-1 outline-amber-400/20" data-testid="conflict">
				<TriangleAlert class="mt-1 size-4 shrink-0" aria-hidden="true" />
				<span class="min-w-0 [overflow-wrap:anywhere]"><span class="font-medium">{c.name}</span>: {c.teams.join(' and ')} disagree. {c.applies}.</span>
			</p>
		{/each}

		{#if view.set.entries.length === 0 && view.set.turnedOff.length === 0 && view.set.problems.length === 0}
			<div class="flex flex-col items-center px-8 py-16 text-center" data-testid="no-processes">
				<span class="flex size-12 items-center justify-center rounded-full bg-white/5 text-gray-300"><FileText class="size-6" aria-hidden="true" /></span>
				<h3 class="mt-4 text-base/7 font-semibold text-white">No processes yet</h3>
				<p class="mt-1 max-w-sm text-sm/6 text-gray-400">Write down how your agents work. Start with how changes get reviewed and merged.</p>
			</div>
		{:else}
			{#if view.set.entries.length > 0 || view.set.turnedOff.length > 0}
			<ul class="divide-y divide-white/5 rounded-lg bg-white/5 outline-1 -outline-offset-1 outline-white/10" aria-label="Processes" data-testid="process-list">
				{#each view.set.entries as entry (entry.key + entry.doc.id)}
					{@const KindIcon = entry.kind === 'inherited' ? undefined : KIND_ICON[entry.kind]}
					<li data-testid="process-row" data-key={entry.key}>
						<button type="button" class="flex w-full flex-wrap items-center gap-x-3 gap-y-1 px-4 py-3 text-left hover:bg-white/5 focus-visible:outline-2 focus-visible:outline-indigo-400" onclick={() => openEntry(entry)}>
							<span class="min-w-48 flex-1">
								<span class="block font-semibold text-white [overflow-wrap:anywhere]">{entry.doc.name}</span>
								<span class="block text-xs/5 text-gray-400 [overflow-wrap:anywhere]">{entry.doc.summary}</span>
							</span>
							<span class="flex min-w-0 max-w-full flex-wrap items-center gap-2">
								<ProcessBadges level={entry.source} scopeName={entry.scopeName} version={entry.doc.version} />
								{#if KindIcon}<span class="inline-flex items-center gap-x-1 text-xs/5 text-gray-300" data-testid="kind"><KindIcon class="size-3.5" aria-hidden="true" /> {KIND_WORD[entry.kind]}{entry.kind === 'override' && entry.doc.overrides ? ` of ${entry.doc.overrides.key}@${entry.doc.overrides.version}` : ''}{entry.kind === 'addition' && entry.doc.location.level === place.kind && (place.kind === 'organization' || entry.doc.location.scope === (place.kind === 'project' ? place.key : place.id)) ? ' (added here)' : ''}</span>{/if}
								{#if entry.sealed}<span class="inline-flex items-center gap-x-1 text-xs/5 text-amber-200" data-testid="sealed"><Lock class="size-3.5" aria-hidden="true" /> {entry.sealedNote ?? 'Sealed'}</span>{/if}
								{#if entry.drift}<span class="inline-flex items-center gap-x-1 text-xs/5 text-amber-200" data-testid="drift"><TriangleAlert class="size-3.5" aria-hidden="true" /> Parent changed: {entry.drift.parentName} is now v{entry.drift.parentVersion}</span>{/if}
								{#if view.agent}
									{@const read = Object.hasOwn(view.agent.reads, entry.key) ? view.agent.reads[entry.key] : undefined}
									<span class="text-xs/5 text-gray-400" data-testid="read">{read ? `Read v${read.version} at ${hhmm(read.at)}${read.docId !== entry.doc.id || read.version !== entry.doc.version ? ` · now ${entry.source === 'organization' ? '' : `${entry.scopeName ?? entry.source} `}v${entry.doc.version}` : ''}` : 'Not read yet'}</span>
								{/if}
								{#if entry.driftUnknown}<span class="text-xs/5 text-gray-400" data-testid="drift-unknown">Parent changes unknown</span>{/if}
							</span>
						</button>
					</li>
				{/each}
				{#each view.set.turnedOff as off (off.key + off.doc.id)}
					<li data-testid="turned-off-row" data-key={off.key}>
						<button type="button" class="flex w-full flex-wrap items-center gap-x-3 gap-y-1 px-4 py-3 text-left hover:bg-white/5 focus-visible:outline-2 focus-visible:outline-indigo-400" onclick={() => openOff(off)}>
							<span class="min-w-0 flex-1 font-semibold text-gray-300 [overflow-wrap:anywhere]">{off.name}</span>
							<span class="flex min-w-0 max-w-full flex-wrap items-center gap-2"><ProcessBadges level={off.level} scopeName={off.scopeName} version={off.doc.version} /><span class="inline-flex items-center gap-x-1 text-xs/5 text-gray-300 [overflow-wrap:anywhere]"><PowerOff class="size-3.5 shrink-0" aria-hidden="true" /> Turned off{off.scopeName ? ` for ${off.scopeName}` : ''}: {off.reason}</span></span>
						</button>
					</li>
				{/each}
			</ul>
			{/if}

			{#if view.set.problems.length > 0}
				<section aria-labelledby="problems-title" class="space-y-2">
					<h3 id="problems-title" class="text-sm/6 font-semibold text-white">Needs attention</h3>
					<ul class="space-y-2">
						{#each view.set.problems as p, i (i)}
							<li class="flex items-start gap-x-2 rounded-lg bg-red-500/10 px-3 py-2 text-sm/6 text-red-200 outline-1 -outline-offset-1 outline-red-400/20" data-testid="problem" data-kind={p.kind}>
								<TriangleAlert class="mt-1 size-4 shrink-0" aria-hidden="true" />
								<span class="[overflow-wrap:anywhere]">{#if p.kind === 'invalid' || p.kind === 'mismatch' || p.kind === 'clash'}<span class="font-medium">{p.doc.name}</span>{': '}{/if}{#each (p.kind === 'clash' ? p.text.split('`') : [problemWords(p)]) as text, index (index)}{#if p.kind === 'clash' && index % 2 === 1}<code>{text}</code>{:else}{text}{/if}{/each}</span>
							</li>
						{/each}
					</ul>
				</section>
			{/if}
		{/if}

		{#if view.agent && !view.noCurrentWork}
			<p class="text-xs/5 text-gray-400">Reads are recorded when the agent starts a task or wakes.</p>
		{/if}
	{/if}
</div>

<ProcessSheet bind:open={sheetOpen} entry={shownEntry} off={shownOff} />
