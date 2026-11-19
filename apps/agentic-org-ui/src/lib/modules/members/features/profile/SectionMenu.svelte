<!--
  @component
  The profile's tabs below 640 px (app-shell-ux §6a: sort, filter and view choices use named buttons and radio menus):
  nine tabs don't fit a phone, and a strip that scrolls hides most of them. A labelled button names the current
  section ("Section: Overview"); its radio menu lists every section and picking one opens its URL.
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
		current,
		sections,
		onchoose,
		class: className = ''
	}: {
		/** The landmark's name: "Kai Lindqvist's profile". */
		label: string;
		/** The current section's id; empty when the URL names a section this member doesn't have. */
		current: string;
		sections: { id: string; label: string }[];
		onchoose: (id: string) => void;
		class?: string;
	} = $props();

	let open = $state(false);
	const currentLabel = $derived(sections.find((s) => s.id === current)?.label ?? 'Choose');
</script>

<nav aria-label={label} class="border-b border-white/10 px-4 py-3 {className}">
	<div class="flex flex-wrap items-center gap-x-3 gap-y-2">
		<span aria-hidden="true" class="text-sm/6 font-medium text-gray-300">Section</span>
		<DropdownMenu bind:open>
			<DropdownMenuTrigger>
				{#snippet child({ props })}
					<Button
						{...props}
						variant="outline"
						class="h-auto min-h-11 min-w-0 flex-1 basis-48 justify-between py-2 text-left whitespace-normal"
						aria-label="Section: {currentLabel}"
					>
						<span class="min-w-0 [overflow-wrap:anywhere]">{currentLabel}</span>
						<MiniIcon name="chevronDown" class="size-5 text-gray-400" />
					</Button>
				{/snippet}
			</DropdownMenuTrigger>
			<DropdownMenuContent
				inert={!open}
				preventScroll={false}
				align="start"
				class="w-(--bits-dropdown-menu-anchor-width) max-w-[calc(100vw-2rem)]"
			>
				<DropdownMenuRadioGroup
					value={current}
					onValueChange={(id) => {
						// Close before moving: the route change would otherwise leave the menu marked open.
						open = false;
						onchoose(id);
					}}
				>
					{#each sections as section (section.id)}
						<DropdownMenuRadioItem value={section.id} class="min-h-11">
							<span class="[overflow-wrap:anywhere]">{section.label}</span>
						</DropdownMenuRadioItem>
					{/each}
				</DropdownMenuRadioGroup>
			</DropdownMenuContent>
		</DropdownMenu>
	</div>
</nav>
