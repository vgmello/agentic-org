<script lang="ts">
	import Ellipsis from '@lucide/svelte/icons/ellipsis';
	import { Button } from '$lib/components/ui/button';
	import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '$lib/components/ui/dropdown-menu';
	import type { Team } from '$lib/proto';

	export type TeamAction = 'rename' | 'members' | 'add-project' | 'move' | 'add-team' | 'delete';

	let { team, onaction }: { team: Team; onaction: (action: TeamAction, team: Team) => void } = $props();
</script>

<DropdownMenu>
	<DropdownMenuTrigger>
		{#snippet child({ props })}
			<Button {...props} variant="ghost" size="icon-sm" aria-label="Actions for {team.name}" data-testid="team-menu"><Ellipsis aria-hidden="true" /></Button>
		{/snippet}
	</DropdownMenuTrigger>
	<DropdownMenuContent align="end">
		<DropdownMenuItem onSelect={() => onaction('rename', team)}>Rename…</DropdownMenuItem>
		<DropdownMenuItem onSelect={() => onaction('members', team)}>Edit members…</DropdownMenuItem>
		<DropdownMenuItem onSelect={() => onaction('add-project', team)}>Add project…</DropdownMenuItem>
		<DropdownMenuItem onSelect={() => onaction('move', team)}>Move to…</DropdownMenuItem>
		<DropdownMenuItem onSelect={() => onaction('add-team', team)}>Add team under {team.name}…</DropdownMenuItem>
		<DropdownMenuSeparator />
		<DropdownMenuItem variant="destructive" onSelect={() => onaction('delete', team)}>Delete…</DropdownMenuItem>
	</DropdownMenuContent>
</DropdownMenu>
