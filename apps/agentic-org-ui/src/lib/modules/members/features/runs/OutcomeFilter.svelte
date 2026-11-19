<!--
  @component
  The Runs tab's outcome filter (agent-run-modes-ux §1 "Filter by outcome"; app-shell-ux §6a filters). Where the five
  choices fit on one line, a segmented radio group: a 36 px track with the selected choice drawn as an inset tile plus
  weight, and each choice's count. Where they don't (a phone, or large text), a named button with a radio menu
  ("Outcome: All"). Which one shows depends on the room the filter has, measured in rem, so large text switches too.
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
	import { OUTCOME_FILTERS, type OutcomeFilter } from '../../present';

	let {
		value = $bindable(),
		counts
	}: { value: OutcomeFilter; counts: Record<OutcomeFilter, number> } = $props();

	const ids = $props.id();
	let open = $state(false);
	const current = $derived(OUTCOME_FILTERS.find((f) => f.id === value)?.label ?? 'All');
</script>

<div class="@container">
	<fieldset
		class="hidden h-9 rounded-md bg-white/5 outline-1 -outline-offset-1 outline-white/10 @min-[40rem]:inline-flex"
	>
		<legend class="sr-only">Outcome</legend>
		{#each OUTCOME_FILTERS as f (f.id)}
			<label
				class="relative flex h-full cursor-pointer items-center gap-x-1.5 px-3 text-sm/6 font-medium text-gray-400 before:absolute before:inset-1 before:rounded-sm hover:text-white has-checked:font-semibold has-checked:text-white has-checked:before:bg-white/10 has-focus-visible:outline-2 has-focus-visible:-outline-offset-2 has-focus-visible:outline-indigo-400"
			>
				<input type="radio" name="{ids}-outcome" value={f.id} bind:group={value} class="sr-only" />
				<span class="relative whitespace-nowrap">{f.label}</span>
				<span class="relative font-normal text-gray-400 tabular-nums">{counts[f.id]}</span>
			</label>
		{/each}
	</fieldset>
	<div class="@min-[40rem]:hidden">
		<DropdownMenu bind:open>
			<DropdownMenuTrigger>
				{#snippet child({ props })}
					<Button
						{...props}
						variant="outline"
						class="h-auto min-h-9 max-w-full py-1.5 text-left whitespace-normal max-[639px]:min-h-11"
					>
						<span class="min-w-0 [overflow-wrap:anywhere]">Outcome: {current}</span>
						<MiniIcon name="chevronDown" class="size-5 text-gray-400" />
					</Button>
				{/snippet}
			</DropdownMenuTrigger>
			<DropdownMenuContent
				inert={!open}
				preventScroll={false}
				align="start"
				class="w-64 max-w-[calc(100vw-2rem)]"
			>
				<DropdownMenuRadioGroup {value} onValueChange={(next) => (value = next as OutcomeFilter)}>
					{#each OUTCOME_FILTERS as f (f.id)}
						<DropdownMenuRadioItem value={f.id} class="min-h-9 max-[639px]:min-h-11">
							<span class="flex-1 [overflow-wrap:anywhere]">{f.label}</span>
							<span class="text-gray-400 tabular-nums">{counts[f.id]}</span>
						</DropdownMenuRadioItem>
					{/each}
				</DropdownMenuRadioGroup>
			</DropdownMenuContent>
		</DropdownMenu>
	</div>
</div>
