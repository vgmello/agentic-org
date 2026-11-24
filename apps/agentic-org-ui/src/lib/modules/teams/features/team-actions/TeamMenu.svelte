<!--
  @component
  A team's actions menu, on a Teams index row and in a team page's header (§7.2): Move, Rename, Change lead,
  Add team under, then Archive and Delete apart, last. Every item opens its reviewed dialog (TeamDialogs).
  Icon-only button includes an Escape-dismissible hover/focus Tooltip and shared .triage-menu-surface menu.
-->
<script lang="ts">
	import { mergeProps, Tooltip } from 'bits-ui';
	import { Button } from '$lib/components/ui/button';
	import {
		DropdownMenu,
		DropdownMenuContent,
		DropdownMenuItem,
		DropdownMenuSeparator,
		DropdownMenuTrigger
	} from '$lib/components/ui/dropdown-menu';
	import type { TeamAction, TeamRow } from '../../types';

	let { team, onaction }: { team: TeamRow; onaction: (action: TeamAction, team: TeamRow) => void } =
		$props();

	let open = $state(false);
</script>

<Tooltip.Provider delayDuration={200} ignoreNonKeyboardFocus>
	<DropdownMenu bind:open>
		<Tooltip.Root delayDuration={250} ignoreNonKeyboardFocus disabled={open}>
			<DropdownMenuTrigger>
				{#snippet child({ props: menuProps })}
					<Tooltip.Trigger>
						{#snippet child({ props: tipProps })}
							<Button
								{...mergeProps(menuProps, tipProps)}
								variant="ghost"
								size="icon"
								class="relative text-fg-muted hover:text-fg max-[639px]:size-11"
								aria-label="Actions for {team.name}"
							>
								<svg viewBox="0 0 20 20" fill="currentColor" class="size-5" aria-hidden="true"
									><path
										d="M3 10a1.5 1.5 0 1 1 3 0 1.5 1.5 0 0 1-3 0ZM8.5 10a1.5 1.5 0 1 1 3 0 1.5 1.5 0 0 1-3 0ZM15.5 8.5a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3Z"
									/></svg
								>
							</Button>
						{/snippet}
					</Tooltip.Trigger>
				{/snippet}
			</DropdownMenuTrigger>
			<Tooltip.Portal>
				<Tooltip.Content
					side="bottom"
					sideOffset={6}
					escapeKeydownBehavior="defer-otherwise-close"
					class="z-60 rounded-md border border-line-strong bg-surface-overlay px-2 py-1 text-xs/5 font-medium text-fg shadow-md"
				>
					Team actions
				</Tooltip.Content>
			</Tooltip.Portal>
		</Tooltip.Root>
		<DropdownMenuContent align="end" class="triage-menu-surface">
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
</Tooltip.Provider>
