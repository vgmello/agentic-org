<!--
  @component
  A button label that keeps its width while busy (app-shell-ux §6a "Pending buttons retain width"): both words share
  one grid cell and the one not shown is `invisible`, so it takes space but is neither seen nor read.
  Usage: <Button aria-busy={busy}><BusyLabel {busy} idle="Save manager" working="Saving…" /></Button>
-->
<script lang="ts">
	let { busy, idle, working }: { busy: boolean; idle: string; working: string } = $props();
</script>

<span class="grid [&>*]:[grid-area:1/1]">
	<span class={[busy && 'invisible']}>{idle}</span>
	<span class={[!busy && 'invisible']}>{working}</span>
</span>
