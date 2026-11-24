<!--
  @component
  Organization settings › Updates (app-shell-ux §0.1; server-installation-and-access-ux §5): the release this
  instance runs and what HQ last learned about a newer one, every office's release, and how to update. Read only:
  there is no restart-to-update before a supported distribution and update flow exists.
-->
<script lang="ts">
	import DescriptionList from '$lib/components/DescriptionList.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import Section from '$lib/components/Section.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { updatesApi } from '../../api';
	import { checkSummary } from '../../present';
	import HowToUpdate from './HowToUpdate.svelte';
	import OfficeVersions from './OfficeVersions.svelte';
	import VersionPill from './VersionPill.svelte';

	let { organizationId }: { organizationId: string } = $props();
	const resource = new Resource(() => updatesApi.overview(organizationId));

	// Retry is disabled while it runs, so focus falls to the body (§6a forbids leaving it there). When a Retry
	// settles: back to Retry if it failed again, else to the page heading, since the Retry button is gone.
	// Only a reload seen starting after the first run counts, so the first read (however fast it settles) never
	// moves focus: the shell has already placed it on arrival.
	let started = false;
	let wasReloading = false;
	$effect(() => {
		const reloading = resource.reloading;
		if (!started) {
			started = true;
			return;
		}
		if (reloading) {
			wasReloading = true;
			return;
		}
		if (!wasReloading) return;
		wasReloading = false;
		const failedAgain = resource.state === 'error';
		setTimeout(() => {
			const active = document.activeElement;
			if (active && active !== document.body && active.isConnected) return;
			const target = failedAgain
				? document.querySelector<HTMLElement>('[data-updates-panel] [role="alert"] button')
				: document.getElementById('updates-heading');
			if (!target) return;
			if (!failedAgain) target.tabIndex = -1;
			target.focus({ preventScroll: true });
		}, 0);
	});
</script>

<PageHeader
	id="updates-heading"
	title="Updates"
	description="The release of Agentic Org this organization runs, office by office, and how to update it."
	class="[&_h2:focus]:outline-none"
/>
<div data-updates-panel>
	<Loaded
		{resource}
		label="updates"
		rows={5}
		error="Couldn't load the release information"
		place="Updates"
		area="Organization settings"
	>
		{#snippet children(overview)}
			{@const check = checkSummary(overview.check)}
			{@const hq = overview.offices.find((o) => o.role === 'hq')}
			{#snippet checkContent()}
				<span class="flex flex-col items-start gap-y-2">
					<VersionPill label={check.word} tone={check.tone} />
					<span data-testid="check-detail" class="text-fg-secondary">{check.detail}</span>
				</span>
			{/snippet}
			<Section
				level={3}
				title="This instance"
				description="The instance that hosts HQ, on {hq?.host ?? 'the HQ machine'}."
			>
				<div class="border-y border-line-subtle" data-testid="instance">
					<DescriptionList
						items={[
							{ term: 'Instance', value: overview.instance.name, mono: true },
							{ term: 'Version', value: overview.instance.version },
							{ term: 'Newer release', content: checkContent }
						]}
					/>
				</div>
			</Section>
			<OfficeVersions offices={overview.offices} hqVersion={overview.instance.version} />
			<HowToUpdate instance={overview.instance.name} machine={hq?.host ?? 'the HQ machine'} />
		{/snippet}
	</Loaded>
</div>
