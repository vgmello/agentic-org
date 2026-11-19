<!--
  @component
  The Recovery row (server-installation-and-access-ux §4a.6): the state's own words, and Copy command where there is a
  command to run. It shows only the state: the code never reaches the browser.
-->
<script lang="ts">
	import CopyButton from '$lib/components/CopyButton.svelte';
	import { NOT_SUPPORTED, recoveryMessage } from '../../recovery';
	import type { GeneralSettings } from '../../types';

	let { general }: { general: GeneralSettings } = $props();
	const date = (value: string) =>
		new Intl.DateTimeFormat(undefined, { day: 'numeric', month: 'short', year: 'numeric' }).format(
			new Date(value)
		);
	const message = $derived(
		recoveryMessage(general.recovery, {
			organization: general.organization.name,
			hqMachine: general.hqMachine,
			instance: general.instance,
			viewerIsAdministrator: general.viewer.isAdministrator,
			date
		})
	);
</script>

<div
	class="px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8"
	data-testid="recovery-row"
	data-state={general.recovery.state}
>
	<p class="max-w-3xl text-sm/6 text-fg-secondary [overflow-wrap:anywhere]">
		{#each message.segments as segment, index (index)}{#if typeof segment === 'string'}{segment}{:else}<code
					class="rounded-sm bg-shade/30 px-1.5 py-0.5 font-mono text-xs/5 text-fg [overflow-wrap:anywhere]"
					>{segment.code}</code
				>{/if}{/each}
		{NOT_SUPPORTED}
	</p>
	{#if message.command}<div class="mt-3">
			<CopyButton text={message.command} label="Copy command" />
		</div>{/if}
</div>
