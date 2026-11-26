<script lang="ts">
	import DescriptionList from '$lib/components/DescriptionList.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import Section from '$lib/components/Section.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Button } from '$lib/components/ui/button';
	import { selectClass } from '$lib/components/classes';
	import {
		ARCHIVE_AFTER_DAYS_OPTIONS,
		uiPrefs,
		type ArchiveAfterDays
	} from '$lib/platform/shell/ui-preferences.svelte';
	import { settingsApi } from '../../api';
	import Loaded from '$lib/components/Loaded.svelte';
	import { tone } from '$lib/components/pills';
	import { Resource } from '$lib/components/resource.svelte';
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

	const uid = $props.id();

	function archiveLabel(days: ArchiveAfterDays): string {
		if (days === 0) return 'Off (keep all closed tasks visible)';
		if (days === 10) return '10 days (default)';
		return `${days} days`;
	}

	function handleArchiveChange(event: Event) {
		const raw = Number((event.currentTarget as HTMLSelectElement).value);
		uiPrefs.setArchiveAfterDays(raw as ArchiveAfterDays);
	}
</script>

<PageHeader
	title="General"
	description="The organization's name and workspace, the runtimes its agents can use, recovery and who administers this instance."
/>
<Loaded
	{resource}
	label="settings"
	error="Couldn't load the organization's settings"
	place="General"
	area="Organization settings"
	rows={5}
>
	{#snippet children(general)}
		{#snippet adminText()}
			{general.administrator.person ?? 'An account without a person here'} ({general.administrator
				.username}){general.administrator.isYou ? ' · you' : ''}
		{/snippet}
		{#snippet handOverAction()}
			<Button bind:ref={handOverButton} variant="outline" onclick={() => (handOver = true)}
				>Hand over…</Button
			>
		{/snippet}
		<Section
			level={3}
			title="Organization"
			description="Read-only here. Where the organization's records live, on the HQ machine."
		>
			<DescriptionList
				items={[
					{ term: 'Name', value: general.organization.name },
					{ term: 'Workspace', value: general.workspace, mono: true },
					{ term: 'HQ machine', value: general.hqMachine }
				]}
			/>
		</Section>

		<Section
			level={3}
			title="Auto-archive closed tasks"
			description="Closed tasks older than this stay hidden from Work, Project, Team, and Member task lists unless “Show archived” is turned on for this session."
		>
			<div
				class="flex flex-wrap items-center justify-between gap-4 border-y border-line-subtle px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8"
			>
				<div class="min-w-0 flex-1">
					<label for="{uid}-archive-days" class="block text-sm/6 font-semibold text-fg">
						Archive closed items after
					</label>
					<p class="mt-0.5 text-xs/5 text-fg-muted">
						Applies across Work, Project Work, Team Work, and Member Work lists.
					</p>
				</div>
				<select
					id="{uid}-archive-days"
					value={String(uiPrefs.archiveAfterDays)}
					onchange={handleArchiveChange}
					class="w-auto min-w-52 {selectClass}"
					data-testid="archive-after-days-select"
				>
					{#each ARCHIVE_AFTER_DAYS_OPTIONS as days (days)}
						<option value={String(days)}>{archiveLabel(days)}</option>
					{/each}
				</select>
			</div>
		</Section>

		<Section
			level={3}
			title="Runtimes"
			description="The runtimes agents can run on. Credentials stay on each office and are never shown here."
		>
			<ul
				role="list"
				class="divide-y divide-line-subtle border-y border-line-subtle"
				data-testid="runtimes"
			>
				{#each general.runtimes as runtime (runtime.id)}
					<li
						class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8"
					>
						<div class="min-w-0">
							<p class="text-sm/6 font-semibold text-fg">{runtime.label}</p>
							<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">{runtime.note}</p>
						</div>
						<StatusPill
							label={availability[runtime.availability].word}
							class={availability[runtime.availability].tone}
						/>
					</li>
				{/each}
			</ul>
		</Section>

		{#if general.viewer.isOwner}
			<Section
				level={3}
				title="Recovery"
				description="The organization's recovery code. Only its owner sees this row."
			>
				<div class="border-y border-line-subtle"><RecoveryRow {general} /></div>
			</Section>
		{/if}

		<Section
			level={3}
			title="Administrator"
			description="The one account on this instance that creates organizations and collects recovery codes."
		>
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

		<HandOverDialog
			bind:open={handOver}
			{general}
			ondone={() => resource.reload()}
			onCloseAutoFocus={(e) => (e.preventDefault(), handOverButton?.focus())}
		/>
	{/snippet}
</Loaded>
