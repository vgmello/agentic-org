<!--
  @component
  Your preferences › Keyboard (app-shell-ux §0.1): the Single-key shortcuts switch, on by default, saved on this
  device only. If the browser will not keep it, the switch stays as it was and says so.
-->
<script lang="ts">
	import Section from '$lib/components/Section.svelte';
	import { singleKeyShortcutsOn, writeSingleKeyShortcuts } from '$lib/platform/shell/shortcuts';

	const on = $derived(singleKeyShortcutsOn());
	let refused = $state(false);
	const labelId = 'single-key-label';
	const helpId = 'single-key-help';

	function toggle() {
		refused = !writeSingleKeyShortcuts(!on);
	}
</script>

<Section
	title="Keyboard"
	description="Saved on this device only. It is not synced to other devices or to your account."
>
	<div class="px-4 min-[640px]:px-6 min-[1024px]:px-8">
		<div
			class="flex items-center justify-between gap-x-6 gap-y-3 border-y border-line-subtle py-4 max-[639px]:flex-wrap"
		>
			<div class="min-w-0 basis-64 flex-1">
				<p id={labelId} class="text-sm/6 font-medium text-fg">Single-key shortcuts</p>
				<p id={helpId} class="mt-1 text-sm/6 text-fg-muted">
					N opens New, / moves to Search, S jumps to Sidebar navigation, and M jumps to Inbox
					categories when you are not typing in a field. Turn this off if single keys get in the way
					of your screen reader or voice control.
				</p>
			</div>
			<button
				type="button"
				role="switch"
				aria-checked={on}
				aria-labelledby={labelId}
				aria-describedby={helpId}
				onclick={toggle}
				class="group relative inline-flex min-h-11 shrink-0 items-center gap-x-3 rounded-md px-1 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
			>
				<span class="w-7 text-sm/6 font-medium text-fg" aria-hidden="true">{on ? 'On' : 'Off'}</span
				>
				<span
					class="relative inline-flex h-6 w-11 shrink-0 rounded-full p-0.5 outline-1 -outline-offset-1 transition-colors duration-200 motion-reduce:transition-none {on
						? 'bg-primary outline-primary'
						: 'switch-track-off outline-line-strong'}"
				>
					<span
						class="size-5 rounded-full bg-on-emphasis shadow-xs ring-1 ring-line-strong transition-transform duration-200 motion-reduce:transition-none {on
							? 'translate-x-5'
							: 'translate-x-0'}"
					></span>
				</span>
			</button>
		</div>
		{#if refused}
			<p role="alert" class="mt-3 text-sm/6 text-danger-text">
				This browser would not keep the change, so single-key shortcuts stay {on ? 'on' : 'off'}.
				Check that site data is allowed for this page, then try again.
			</p>
		{/if}
	</div>
</Section>

<style>
	.switch-track-off {
		background: var(--bar-track);
	}
</style>
