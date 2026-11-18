<!--
  @component
  A team row's menu on the Teams index (§7.2): Move, Rename, Change lead, Add team under, then Archive and
  Delete apart, last. Every item opens its reviewed dialog.
-->
<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import {
		DropdownMenu,
		DropdownMenuContent,
		DropdownMenuItem,
		DropdownMenuSeparator,
		DropdownMenuTrigger
	} from '$lib/components/ui/dropdown-menu';
	import type { TeamRow } from '../../types';

	export type TeamRowAction = 'move' | 'rename' | 'lead' | 'add-team' | 'archive' | 'delete';
	let {
		team,
		onaction
	}: { team: TeamRow; onaction: (action: TeamRowAction, team: TeamRow) => void } = $props();
</script>

<DropdownMenu>
	<DropdownMenuTrigger>
		{#snippet child({ props })}
			<Button
				{...props}
				variant="ghost"
				size="icon"
				class="relative text-gray-400 hover:text-white max-[639px]:size-11"
				aria-label="Actions for {team.name}"
			>
				<svg viewBox="0 0 20 20" fill="currentColor" class="size-5" aria-hidden="true"
					><path
						d="M3 10a1.5 1.5 0 1 1 3 0 1.5 1.5 0 0 1-3 0ZM8.5 10a1.5 1.5 0 1 1 3 0 1.5 1.5 0 0 1-3 0ZM15.5 8.5a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3Z"
					/></svg
				>
			</Button>
		{/snippet}
	</DropdownMenuTrigger>
	<DropdownMenuContent align="end">
		<DropdownMenuItem onSelect={() => onaction('move', team)}>Move to…</DropdownMenuItem>
		<DropdownMenuItem onSelect={() => onaction('rename', team)}>Rename…</DropdownMenuItem>
		<DropdownMenuItem onSelect={() => onaction('lead', team)}>Change lead…</DropdownMenuItem>
		<DropdownMenuItem onSelect={() => onaction('add-team', team)}
			>Add team under {team.name}…</DropdownMenuItem
		>
		<DropdownMenuSeparator />
		<DropdownMenuItem onSelect={() => onaction('archive', team)}>Archive…</DropdownMenuItem>
		<DropdownMenuItem variant="destructive" onSelect={() => onaction('delete', team)}
			>Delete…</DropdownMenuItem
		>
	</DropdownMenuContent>
</DropdownMenu>
