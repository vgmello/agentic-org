<!--
  @component
  What updating takes (server-installation-and-access-ux §5, SRV-15, SRV-16; §4a.4 for sessions): the local command
  for the selected instance, where to run it and what it does. The browser offers no restart-to-update (app-shell-ux
  §0.1), so the only control here copies the command.
-->
<script lang="ts">
	import CopyButton from '$lib/components/CopyButton.svelte';
	import Section from '$lib/components/Section.svelte';
	import { gutters } from '$lib/components/classes';
	import { upgradeCommand } from '../../present';

	let { instance, machine }: { instance: string; machine: string } = $props();

	const command = $derived(upgradeCommand(instance));
</script>

<Section
	level={3}
	title="How to update"
	description="Updating is done on the HQ machine with the Agentic Org command line, not from the browser."
>
	<div class="max-w-3xl {gutters}">
		<p class="text-sm/6 text-fg-body">
			On <span class="font-semibold text-fg [overflow-wrap:anywhere]">{machine}</span>, run:
		</p>
		<div class="mt-3 flex flex-wrap items-center gap-3">
			<code
				data-testid="upgrade-command"
				class="min-w-0 flex-1 basis-56 rounded-md bg-surface-sunken max-[639px]:basis-full px-3 py-2 font-mono text-sm/6 text-fg-bright [overflow-wrap:anywhere]"
				>{command}</code
			>
			<CopyButton text={command} label="Copy command" />
		</div>
		<ul
			role="list"
			class="mt-4 list-disc space-y-2 pl-5 text-sm/6 text-fg-body marker:text-fg-subtle"
			data-testid="upgrade-effects"
		>
			<li>It downloads and verifies the complete release, unless it's already downloaded.</li>
			<li>
				It stops this instance: new work waits, saving finishes and its agents stop. Then it starts
				on the new release and checks that it's ready.
			</li>
			<li>You stay signed in. Pages show that live updates are paused until HQ is back.</li>
			<li>
				The previous release and your data are kept, so an upgrade that fails can be recovered.
			</li>
			<li>
				Other instances on that machine, and offices on other machines, keep their release until
				they're upgraded there.
			</li>
		</ul>
	</div>
</Section>
