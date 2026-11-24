<script lang="ts">
	// The Teams view switch (Prototype Release 1149b4f1, app-shell-ux §0): Teams · Members · Org chart.
	// Uses the shared .section-header.org-directory-header primary-tab underline strip; switching focuses the heading.
	import TabsNav from '$lib/components/TabsNav.svelte';

	let { children } = $props();
	const views = [
		{ label: 'Teams', href: '/teams' },
		{ label: 'Members', href: '/teams/members' },
		{ label: 'Org chart', href: '/teams/org-chart' }
	];
</script>

<div class="section-header org-directory-header">
	<TabsNav label="Teams views" tabs={views} focusHeading />
</div>
{@render children()}
