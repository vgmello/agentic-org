<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { TeamOverview } from '$lib/modules/teams/features/team-page';
	import { teamHref } from '$lib/modules/teams/team-page';

	// Preserve legacy deep links to team task details (`/teams/:id?item=...` -> `/teams/:id/work?item=...`).
	const hasLegacyItem = $derived(page.url.searchParams.has('item'));
	$effect(() => {
		if (!hasLegacyItem) return;
		const teamId = page.params.teamId;
		if (!teamId) return;
		const target = `${teamHref(teamId, 'work')}${page.url.search}`;
		void goto(resolve(target as '/'), { replaceState: true, noScroll: true });
	});
</script>

{#if !hasLegacyItem}
	<TeamOverview />
{/if}
