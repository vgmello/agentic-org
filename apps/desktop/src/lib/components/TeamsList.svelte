<script lang="ts">
	import { leadOf, membersOfTeam, projectsOfTeam, type TeamNode } from '$lib/org/model';
	import type { OrgSnapshot } from '$lib/proto';

	let { snapshot, tree }: { snapshot: OrgSnapshot; tree: TeamNode[] } = $props();
</script>

<!-- The same tree as the chart, as a plain nested list: for screen readers and for reading it in order (TEAM-10). -->
{#snippet items(nodes: TeamNode[])}
	<ul class="space-y-2 pl-5 first:pl-0">
		{#each nodes as node (node.team.id)}
			{@const members = membersOfTeam(snapshot, node.team.id)}
			{@const lead = leadOf(snapshot, node.team)}
			<li data-testid="team-list-item">
				<p class="text-sm/6 font-semibold text-white">{node.team.name}</p>
				<p class="text-xs/5 text-gray-400">
					{lead ? `Lead: ${lead.name}` : 'No lead'} ·
					{members.length === 0
						? 'No members'
						: members.map((m) => `${m.name} (${m.kind})`).join(', ')}
					{#if projectsOfTeam(snapshot, node.team.id).length > 0}
						· Projects: {projectsOfTeam(snapshot, node.team.id).map((p) => p.name).join(', ')}
					{/if}
				</p>
				{#if node.children.length > 0}{@render items(node.children)}{/if}
			</li>
		{/each}
	</ul>
{/snippet}

{@render items(tree)}
