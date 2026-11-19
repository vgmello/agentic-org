<!--
  @component
  The release each office runs, HQ first then A–Z (offices-and-teams-ux §6.1), compared with HQ in words. An office
  that holds new features back is the one amber row, with the offices spec's quiet note, as plain text, above the list.
-->
<script lang="ts">
	import Section from '$lib/components/Section.svelte';
	import { gutters } from '$lib/components/classes';
	import { holdBackNote, officeStanding, sortOffices } from '../../present';
	import type { OfficeVersion } from '../../types';
	import VersionPill from './VersionPill.svelte';

	let { offices, hqVersion }: { offices: OfficeVersion[]; hqVersion: string } = $props();

	const rows = $derived(sortOffices(offices));
	const note = $derived(holdBackNote(offices));
</script>

<Section
	level={3}
	title="Offices"
	description="The release each office runs. Offices can be updated in any order; each is updated on its own machine."
>
	{#if note}
		<p data-testid="hold-back-note" class="mb-4 max-w-3xl text-sm/6 text-fg-secondary {gutters}">
			{note}
		</p>
	{/if}
	<ul
		role="list"
		class="divide-y divide-line-subtle border-y border-line-subtle"
		data-testid="office-versions"
	>
		{#each rows as office (office.id)}
			{@const standing = officeStanding(office, hqVersion)}
			<li class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 py-4 {gutters}">
				<div class="min-w-0 flex-1 basis-48">
					<p class="text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]">
						{office.name}
					</p>
					<p class="mt-1 text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
						{#if office.role === 'hq' && office.name.trim().toLowerCase() !== 'hq'}HQ ·
						{/if}{office.host} ·
						{#if office.version}Version <span class="text-fg-secondary">{office.version}</span
							>{:else}Version not reported{/if}
					</p>
					{#if office.holdsBack}
						<p class="mt-1 text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
							To update it, run <code class="font-mono text-fg-bright">agentic-org upgrade</code> on
							{office.host}.
						</p>
					{/if}
				</div>
				{#if standing}<VersionPill label={standing.word} tone={standing.tone} />{/if}
			</li>
		{/each}
	</ul>
</Section>
