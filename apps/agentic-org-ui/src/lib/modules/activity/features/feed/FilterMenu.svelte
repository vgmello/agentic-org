<!--
  @component
  A named filter button with a radio menu (app-shell-ux §6a: sort and filter use named text buttons and radio
  menus). The button shows the current choice ("All teams", "Runs"); the menu is modal.
-->
<script lang="ts">
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import { Button } from '$lib/components/ui/button';
	import {
		DropdownMenu,
		DropdownMenuContent,
		DropdownMenuRadioGroup,
		DropdownMenuRadioItem,
		DropdownMenuTrigger
	} from '$lib/components/ui/dropdown-menu';

	let {
		label,
		value,
		options,
		onchange
	}: {
		/** What the menu filters, spoken before the choice: "Team". */
		label: string;
		value: string;
		options: { id: string; label: string }[];
		onchange: (id: string) => void;
	} = $props();

	let open = $state(false);
	const current = $derived(options.find((o) => o.id === value)?.label ?? options[0]?.label ?? '');
</script>

<DropdownMenu bind:open>
	<DropdownMenuTrigger>
		{#snippet child({ props })}
			<Button
				{...props}
				variant="outline"
				class="max-w-full max-[639px]:min-h-11 max-[639px]:flex-1"
				aria-label="{label}: {current}"
			>
				<span class="truncate">{current}</span>
				<MiniIcon name="chevronDown" class="size-5 text-gray-400" />
			</Button>
		{/snippet}
	</DropdownMenuTrigger>
	<DropdownMenuContent
		inert={!open}
		preventScroll={false}
		align="start"
		class="w-56 max-w-[calc(100vw-2rem)]"
	>
		<DropdownMenuRadioGroup {value} onValueChange={onchange}>
			{#each options as option (option.id)}
				<DropdownMenuRadioItem value={option.id} class="min-h-9 max-[639px]:min-h-11">
					<span class="truncate">{option.label}</span>
				</DropdownMenuRadioItem>
			{/each}
		</DropdownMenuRadioGroup>
	</DropdownMenuContent>
</DropdownMenu>
