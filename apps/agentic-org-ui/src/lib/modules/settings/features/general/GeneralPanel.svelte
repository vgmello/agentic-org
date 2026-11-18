<script lang="ts">
	import DescriptionList from '$lib/components/DescriptionList.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import Section from '$lib/components/Section.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Button } from '$lib/components/ui/button';
	import { settingsApi } from '../../api';
	import Loaded from '../../components/Loaded.svelte';
	import { tone } from '../../components/pills';
	import { Resource } from '../../resource.svelte';
	import type { RuntimeSupport } from '../../types';
	import HandOverDialog from './HandOverDialog.svelte';
	import RecoveryRow from './RecoveryRow.svelte';

	let { organizationId }: { organizationId: string } = $props();
	const resource = new Resource(() => settingsApi.general(organizationId));
	let handOver = $state(false);
	let handOverButton = $state<HTMLButtonElement | null>(null);

	const availability: Record<RuntimeSupport['availability'], { word: string; tone: string }> = {
		ready: { word: 'Ready', tone: tone.ok },
		needs_setup: { word: 'Needs setup', tone: tone.owner },
		unknown: { word: 'Status unknown', tone: tone.neutral }
	};
</script>

<PageHeader title="General" description="The organization's name and workspace, the runtimes its agents can use, recovery and who administers this instance." />
<Loaded {resource} label="settings" error="Couldn't load the organization's settings" place="General">
	{#snippet children(general)}
		{#snippet adminText()}
			{general.administrator.person ?? 'An account without a person here'} ({general.administrator.username}){general.administrator.isYou ? ' · you' : ''}
		{/snippet}
		{#snippet handOverAction()}
			<Button bind:ref={handOverButton} variant="outline" onclick={() => (handOver = true)}>Hand over…</Button>
		{/snippet}
		<Section level={3} title="Organization" description="Read-only here. Where the organization's records live, on the HQ machine.">
			<DescriptionList
				items={[
					{ term: 'Name', value: general.organization.name },
					{ term: 'Workspace', value: general.workspace, mono: true },
					{ term: 'HQ machine', value: general.hqMachine }
				]}
			/>
		</Section>

		<Section level={3} title="Runtimes" description="The runtimes agents can run on. Credentials stay on each office and are never shown here.">
			<ul role="list" class="divide-y divide-white/5 border-y border-white/5" data-testid="runtimes">
				{#each general.runtimes as runtime (runtime.id)}
					<li class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8">
						<div class="min-w-0">
							<p class="text-sm/6 font-semibold text-white">{runtime.label}</p>
							<p class="text-xs/5 text-gray-400 [overflow-wrap:anywhere]">{runtime.note}</p>
						</div>
						<StatusPill label={availability[runtime.availability].word} class={availability[runtime.availability].tone} />
					</li>
				{/each}
			</ul>
		</Section>

		{#if general.viewer.isOwner}
			<Section level={3} title="Recovery" description="The organization's recovery code. Only its owner sees this row.">
				<div class="border-y border-white/5"><RecoveryRow {general} /></div>
			</Section>
		{/if}

		<Section level={3} title="Administrator" description="The one account on this instance that creates organizations and collects recovery codes.">
			<DescriptionList
				items={[
					{
						term: 'Administrator',
						content: adminText,
						...(general.viewer.isAdministrator ? { action: handOverAction } : {})
					}
				]}
			/>
		</Section>

		<HandOverDialog bind:open={handOver} {general} ondone={() => resource.reload()} onCloseAutoFocus={(e) => (e.preventDefault(), handOverButton?.focus())} />
	{/snippet}
</Loaded>
