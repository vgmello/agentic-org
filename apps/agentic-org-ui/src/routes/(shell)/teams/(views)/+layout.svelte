<script lang="ts">
	// The Teams view switch (app-shell-ux §0): Teams · Members · Org chart. A team's own page
	// (/teams/[teamId]) is outside this group, so it has no switch. Switching focuses the heading.
	import TabsNav from '$lib/components/TabsNav.svelte';

	let { children } = $props();
	const views = [
		{ label: 'Teams', href: '/teams' },
		{ label: 'Members', href: '/teams/members' },
		{ label: 'Org chart', href: '/teams/org-chart' }
	];
</script>

<TabsNav label="Teams views" tabs={views} focusHeading />
{@render children()}
