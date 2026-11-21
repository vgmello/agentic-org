<!--
  @component
  A project's Work tab (work-items-ux §9, PRJ-1). Until Work items (plan step 5) builds the board and list, it shows the
  task board and bug list that v1's project page served, as its Tasks and Bugs tabs, so the release keeps them.
-->
<script lang="ts">
	import BugsPanel from '$lib/modules/work/features/bugs/BugsPanel.svelte';
	import TasksBoard from '$lib/modules/work/features/tasks/TasksBoard.svelte';
	import type { OrgSnapshot } from '$lib/proto';
	import { projectFrame } from '../frame/context';

	let { snapshot }: { snapshot: OrgSnapshot } = $props();

	const frame = projectFrame();
	const uid = $props.id();
	const views = [
		{ id: 'tasks', name: 'Tasks' },
		{ id: 'bugs', name: 'Bugs' }
	] as const;
	let view = $state<'tasks' | 'bugs'>('tasks');
	let tablist = $state<HTMLDivElement | null>(null);

	function handleTabKey(event: KeyboardEvent, index: number) {
		let next: number | undefined;
		if (event.key === 'ArrowRight') next = (index + 1) % views.length;
		else if (event.key === 'ArrowLeft') next = (index - 1 + views.length) % views.length;
		else if (event.key === 'Home') next = 0;
		else if (event.key === 'End') next = views.length - 1;
		if (next === undefined) return;
		event.preventDefault();
		view = views[next].id;
		tablist
			?.querySelector<HTMLButtonElement>(`#${CSS.escape(`${uid}-tab-${views[next].id}`)}`)
			?.focus();
	}
</script>

<div class="min-w-0 space-y-5 px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8">
	<div
		bind:this={tablist}
		role="tablist"
		aria-label="Work"
		class="flex flex-wrap gap-x-1 border-b border-line"
	>
		{#each views as v, index (v.id)}
			{@const selected = view === v.id}
			<button
				id="{uid}-tab-{v.id}"
				type="button"
				role="tab"
				aria-selected={selected}
				aria-controls="{uid}-panel"
				tabindex={selected ? 0 : -1}
				onclick={() => (view = v.id)}
				onkeydown={(e) => handleTabKey(e, index)}
				class="px-3 py-2 text-sm/6 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 {selected
					? 'border-b-2 border-brand font-semibold text-fg'
					: 'text-fg-secondary hover:text-fg'}">{v.name}</button
			>
		{/each}
	</div>
	<div id="{uid}-panel" role="tabpanel" aria-labelledby="{uid}-tab-{view}" class="min-w-0">
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
</div>
