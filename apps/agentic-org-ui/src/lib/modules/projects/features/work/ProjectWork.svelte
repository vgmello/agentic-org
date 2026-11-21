<!--
  @component
  A project's Work tab (work-items-ux §3, §9, PRJ-1): uses the unified Work items queue scoped to this project,
  with Tasks, Bugs and Requests tabs, board/list views, filters, and project-derived accountable team capture.
-->
<script lang="ts">
	import { WorkQueueView } from '$lib/modules/workitems';
	import { projectFrame } from '../frame/context';

	const frame = projectFrame();
</script>

{#key `${frame.organizationId}:${frame.projectId}`}
	<WorkQueueView
		organizationId={frame.organizationId}
		scope={{ kind: 'project', projectId: frame.projectId }}
		projectArchived={frame.project.archived}
	/>
{/key}
