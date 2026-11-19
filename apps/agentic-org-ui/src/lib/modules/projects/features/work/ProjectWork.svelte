<!--
  @component
  A project's Work tab (work-items-ux §9, PRJ-1). Until Work items (plan step 5) builds the board and list, it shows the
  task board and bug list that v1's project page served, as its Tasks and Bugs tabs, so the release keeps them.
-->
<script lang="ts">
	import BugsPanel from '$lib/components/BugsPanel.svelte';
	import TasksBoard from '$lib/components/TasksBoard.svelte';
	import type { OrgSnapshot } from '$lib/proto';
	import { projectFrame } from '../frame/context';

	let { snapshot }: { snapshot: OrgSnapshot } = $props();

	const frame = projectFrame();
	const views = [
		{ id: 'tasks', name: 'Tasks' },
		{ id: 'bugs', name: 'Bugs' }
	] as const;
	let view = $state<'tasks' | 'bugs'>('tasks');
</script>

<div class="min-w-0 space-y-5 px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8">
	<div role="tablist" aria-label="Work" class="flex flex-wrap gap-x-1 border-b border-line">
		{#each views as v (v.id)}
			<button
				type="button"
				role="tab"
				aria-selected={view === v.id}
				onclick={() => (view = v.id)}
				class="px-3 py-2 text-sm/6 focus-visible:outline-2 focus-visible:outline-ring {view === v.id
					? 'border-b-2 border-brand font-semibold text-fg'
					: 'text-fg-secondary hover:text-fg'}">{v.name}</button
			>
		{/each}
	</div>
	{#if view === 'tasks'}
		<TasksBoard
			organizationId={frame.organizationId}
			projectId={frame.projectId}
			projectName={frame.project.name}
			{snapshot}
		/>
	{:else}
		<BugsPanel
			organizationId={frame.organizationId}
			projectId={frame.projectId}
			projectName={frame.project.name}
			{snapshot}
		/>
	{/if}
</div>
