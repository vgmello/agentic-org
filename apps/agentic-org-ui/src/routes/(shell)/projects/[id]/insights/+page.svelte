<script lang="ts">
	// Project › Insights `/projects/[id]/insights`: thin route reading project context and viewer time zone.
	import { ProjectInsightsView } from '$lib/modules/costs';
	import { accountZone } from '$lib/modules/preferences';
	import { projectFrame } from '$lib/modules/projects';

	const frame = projectFrame();
	$effect(() => void accountZone.ensure(frame.organizationId));
</script>

{#if accountZone.loaded}
	{#key `${frame.organizationId}|${frame.projectId}|${accountZone.zone}`}
		<ProjectInsightsView
			organizationId={frame.organizationId}
			projectId={frame.projectId}
			projectName={frame.project.name}
			zone={accountZone.zone}
		/>
	{/key}
{/if}
