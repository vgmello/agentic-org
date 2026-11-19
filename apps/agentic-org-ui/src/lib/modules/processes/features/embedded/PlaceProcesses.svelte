<!--
  @component
  A place's Processes tab on another page (processes-ux §2 "Where"): a team's, or what an agent follows (PROC-10).
  The organization's processes and standards that apply there, each with where it comes from and whether it applies;
  for an agent, its current work or "No current work…"; a project's development process named above the list, never
  listed (PROC-35). A row opens the document. Writing and overriding happen in Organization settings › Processes,
  one link away. The caller owns the read (`readPlace`), so it can reload it with its page.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import type { Resource } from '$lib/components/resource.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import StaleNotice from '../../components/StaleNotice.svelte';
	import {
		EMPTY_TEXT,
		NO_CURRENT_WORK,
		agentWorkText,
		devProcessHref,
		listHref
	} from '../../present';
	import type { EffectiveRow, Place } from '../../types';
	import EffectiveList from '../effective-list/EffectiveList.svelte';
	import DocumentSheet from '../inspect/DocumentSheet.svelte';
	import type { PlaceRead } from '../place-view/load';

	let {
		resource,
		place,
		name,
		headingLevel = 3
	}: {
		resource: Resource<PlaceRead>;
		place: Place;
		/** "Web", "Noor Haddad": for the sentences. */
		name: string;
		/** The level of the two section headings under the page's own. */
		headingLevel?: 3 | 4;
	} = $props();

	let sheetOpen = $state(false);
	let sheetRow = $state.raw<EffectiveRow | undefined>();
	let opener: HTMLElement | null = null;
	const id = $derived(`place-processes-${place.kind}`);

	function open(row: EffectiveRow, from: HTMLElement) {
		opener = from;
		sheetRow = row;
		sheetOpen = true;
	}
</script>

<Loaded
	{resource}
	label="processes"
	error="Couldn't load the processes {name} follows"
	errorLevel={headingLevel}
>
	{#snippet children(read)}
		{#if 'notFound' in read}
			<EmptyState
				level={headingLevel}
				title="No processes here"
				description="The office doesn't know this place any more."
			/>
		{:else}
			{@const view = read.view}
			{#if view.stale}
				<StaleNotice
					stale={view.stale}
					retrying={resource.reloading}
					onretry={() => resource.reload()}
				/>
			{/if}
			<div
				class="flex flex-wrap items-start justify-between gap-x-6 gap-y-3 px-4 pt-6 min-[640px]:px-6 min-[1024px]:px-8"
			>
				<div class="min-w-0 flex-1 basis-64 space-y-1 text-sm/6 text-fg-muted">
					{#if view.agent}
						<p data-testid="agent-work">
							{view.agent.work
								? agentWorkText(name, view.agent.work.ref, view.agent.work.title)
								: NO_CURRENT_WORK}
						</p>
					{:else}
						<p>
							The organization's processes and standards that apply to {name}, and where each comes
							from.
						</p>
					{/if}
					{#if view.developmentProcess}
						<p data-testid="dev-process-line">
							This project follows <a
								href={resolve(devProcessHref(view.developmentProcess.id) as '/')}
								class="rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
								>{view.developmentProcess.name}</a
							>. Its skills are shown there.
						</p>
					{/if}
				</div>
				<Button variant="outline" class={mobileButton} href={resolve(listHref(place) as '/')}
					>Open in Processes</Button
				>
			</div>
			{#if view.processes.length === 0 && view.standards.length === 0}
				<EmptyState
					level={headingLevel}
					title="No processes yet"
					description={EMPTY_TEXT.process}
				/>
			{:else}
				{#each [['process', view.processes], ['standard', view.standards]] as const as [kind, rows] (kind)}
					<section aria-labelledby="{id}-{kind}" class="mt-6">
						<svelte:element
							this={`h${headingLevel}`}
							id="{id}-{kind}"
							class="px-4 text-sm/6 font-semibold text-fg min-[640px]:px-6 min-[1024px]:px-8"
							>{kind === 'process' ? 'Organization processes' : 'Standards'}
							<span class="font-normal text-fg-muted">{rows.length}</span></svelte:element
						>
						{#if rows.length === 0}
							<p class="px-4 py-4 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
								None apply here.
							</p>
						{:else}
							<div class="mt-2">
								<EffectiveList {rows} {place} {kind} labelledby="{id}-{kind}" onopen={open} />
							</div>
						{/if}
					</section>
				{/each}
			{/if}
		{/if}
	{/snippet}
</Loaded>

<DocumentSheet
	bind:open={sheetOpen}
	row={sheetRow}
	{place}
	placeName={name}
	onCloseAutoFocus={(e) => {
		e.preventDefault();
		opener?.focus();
	}}
/>
