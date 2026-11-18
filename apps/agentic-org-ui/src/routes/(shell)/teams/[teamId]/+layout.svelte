<script lang="ts">
	import { TeamPage } from '$lib/modules/teams/features/team-page';

	let { data, children } = $props();
</script>

<!-- Another team is another page: its reads start again. -->
{#key data.teamId}
	<TeamPage
		connection={data.connection}
		snapshot={data.snapshot}
		teamId={data.teamId}
		content={children}
	/>
{/key}
