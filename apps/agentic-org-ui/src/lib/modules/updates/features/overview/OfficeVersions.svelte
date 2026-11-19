<!--
  @component
  The release each office runs, HQ first then A–Z (offices-and-teams-ux §6.1), compared with HQ in words. An office
  that holds new features back is the one amber row; the offices spec's quiet note above the list links to it.
-->
<script lang="ts">
	import Section from '$lib/components/Section.svelte';
	import { gutters } from '$lib/components/classes';
	import {
		holdBackNote,
		listSeparator,
		officeStanding,
		officeUpgradeHint,
		sortOffices
	} from '../../present';
	import type { OfficeVersion } from '../../types';
	import VersionPill from './VersionPill.svelte';

	let { offices, hqVersion }: { offices: OfficeVersion[]; hqVersion: string } = $props();

	const rows = $derived(sortOffices(offices));
	const note = $derived(holdBackNote(offices));
	const rowId = (office: OfficeVersion) => `office-version-${office.id}`;
	const code = 'font-mono text-fg-bright [overflow-wrap:anywhere]';
</script>

<Section
	level={3}
	title="Offices"
	description="The release each office runs. Offices can be updated in any order; each is updated on its own machine."
>
	{#if note}
		<p data-testid="hold-back-note" class="mb-4 max-w-3xl text-sm/6 text-fg-secondary {gutters}">
			{#each note.held as office, i (office.id)}{listSeparator(i, note.held.length)}<a
					href="#{rowId(office)}"
					class="rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
					>{office.name}</a
				>{/each}{note.tail}
		</p>
	{/if}
	<ul
		role="list"
		class="divide-y divide-line-subtle border-y border-line-subtle"
		data-testid="office-versions"
	>
		{#each rows as office (office.id)}
			{@const standing = officeStanding(office, hqVersion)}
			{@const hint = officeUpgradeHint(office)}
			<li
				id={rowId(office)}
				tabindex="-1"
				class="flex scroll-mt-4 flex-wrap items-center justify-between gap-x-4 gap-y-2 py-4 outline-none target:bg-surface-raised {gutters}"
			>
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
						<p
							class="mt-1 text-xs/5 text-fg-muted [overflow-wrap:anywhere]"
							data-testid="office-hint"
						>
							{#if hint.known}
								To update it, run <code class={code}>{hint.command}</code> on {office.host}.
							{:else}
								To update it, run <code class={code}>{hint.command}</code> on {office.host}, with
								the name of the instance this office runs in. HQ doesn't know it, and without it the
								command updates that machine's <code class={code}>default</code> instance.
							{/if}
						</p>
					{/if}
				</div>
				{#if standing}<VersionPill label={standing.word} tone={standing.tone} />{/if}
			</li>
		{/each}
	</ul>
</Section>
