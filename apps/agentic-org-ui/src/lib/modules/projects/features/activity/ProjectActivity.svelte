<!--
  @component
  A project's Activity tab (Prototype Releases 5da47e1d & 53728c05):
  Opens on the project-scoped Assembly line with the collapsible Release gates dock (stations,
  milestones, needs a human, gate status, 7d throughput + avg cycle time + waiting on people now,
  and chronological line events). Registers the Assembly line / Activity feed switcher and
  view-specific Display/Filter controls on the project toolbar via ProjectFrameContext.
-->
<script lang="ts">
	import { Resource } from '$lib/components/resource.svelte';
	import { ActivityCommandCenter } from '$lib/modules/activity';
	import { releasesApi } from '$lib/modules/releases/api';
	import type { ReleaseRow } from '$lib/modules/releases/types';
	import { useShellData } from '$lib/platform/shell/shell-data.svelte';
	import type { OrgSnapshot } from '$lib/proto';
	import { projectsApi } from '../../api';
	import { projectFrame } from '../frame/context';

	const frame = projectFrame();
	const shellData = useShellData();

	const activityRead = new Resource(() =>
		projectsApi.activity(frame.organizationId, frame.projectId)
	);
	const releasesRead = new Resource(() => releasesApi.list(frame.organizationId, frame.projectId));

	const activeRelease = $derived.by<ReleaseRow | undefined>(() => {
		const list = releasesRead.data?.releases ?? [];
		if (list.length === 0) return undefined;
		return (
			list.find((r) => r.status === 'in_progress') ??
			list.find((r) => r.status === 'planned') ??
			list[0]
		);
	});

	const releaseLabel = $derived.by(() => {
		if (!activeRelease) return frame.project.name;
		const label = activeRelease.versionLabel?.trim();
		return label
			? `Release ${label} · ${activeRelease.title}`
			: `R-${activeRelease.number} · ${activeRelease.title}`;
	});

	const fallbackSnapshot = $derived<OrgSnapshot>({
		position: { epoch: 0, seq: 0 },
		projects: [
			{
				id: frame.project.id,
				name: frame.project.name,
				key: `P-${frame.project.number}`,
				archived: frame.project.archived,
				version: frame.project.revision
			}
		],
		teams: [],
		members: [],
		areas: []
	});

	const effectiveSnapshot = $derived(shellData.snapshot ?? fallbackSnapshot);
</script>

<ActivityCommandCenter
	organizationId={frame.organizationId}
	organizationName={frame.project.name}
	snapshot={effectiveSnapshot}
	lockedProjectId={frame.projectId}
	{releaseLabel}
	extraProjectActivity={activityRead.data ?? []}
	onRegisterToolbar={frame.setToolbarControls}
/>
