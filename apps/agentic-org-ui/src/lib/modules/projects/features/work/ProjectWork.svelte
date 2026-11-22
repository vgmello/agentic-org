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

<div class="flex min-w-0 flex-col gap-5 px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8">
	<div
		role="tablist"
		aria-label="Work"
		class="exec-well inline-flex w-fit flex-wrap items-center gap-1 p-1"
	>
		{#each views as v (v.id)}
			<button
				type="button"
				role="tab"
				aria-selected={view === v.id}
				onclick={() => (view = v.id)}
				class={[
					'proj-work-tab min-h-9 rounded-lg px-3.5 text-sm/6 font-medium transition-colors focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 [@media(pointer:coarse)]:min-h-11',
					view === v.id ? 'is-on font-semibold' : 'hover:text-fg'
				]}>{v.name}</button
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

<style>
	.proj-work-tab {
		color: var(--exec-muted);
	}
	.proj-work-tab.is-on {
		background: var(--tone-accent-bg);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
</style>
