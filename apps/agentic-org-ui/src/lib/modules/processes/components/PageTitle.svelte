<!--
  @component
  The document title of a Processes screen, rendered only where the screen itself is shown, so an unavailable
  screen keeps NotBuilt's title and no second <title> appears (app-shell-ux §4).
-->
<script lang="ts">
	let { title }: { title: string } = $props();
</script>

<svelte:head><title>{title}</title></svelte:head>
