<script lang="ts">
	import { Button, type ButtonVariant } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	const variants: ButtonVariant[] = ['default', 'outline', 'secondary', 'ghost', 'destructive', 'link'];
	let activations = $state(0), selected = $state('Nothing selected');
</script>
<section aria-label="Focus contracts" class="fixed inset-2 z-40 overflow-auto rounded-md bg-gray-900 p-4">
	<button data-testid="focus-start">Start here</button>
	{#each variants as variant}
		<div role="group" aria-label={variant} class="my-3 flex flex-wrap gap-3 p-1">
			<Button {variant} onclick={() => activations++}>Activate</Button>
			<Button {variant} href="#focus-link">Navigate</Button>
		</div>
	{/each}
	<div class="flex flex-wrap gap-3 p-1">
		<Button disabled>Disabled</Button>
		<Button href="#disabled" disabled>Disabled link</Button>
		<Button aria-invalid="true">Invalid</Button>
		<Button class="focus-visible:outline-emerald-400">Override</Button>
		<DropdownMenu.Root>
			<DropdownMenu.Trigger>Commands</DropdownMenu.Trigger>
			<DropdownMenu.Content>
				<DropdownMenu.Item disabled>Native disabled</DropdownMenu.Item>
				<DropdownMenu.Item onSelect={() => selected = 'Renamed'}>Rename item</DropdownMenu.Item>
				<DropdownMenu.Item closeOnSelect={false} onSelect={(event) => event.preventDefault()}>
					{#snippet child({ props })}<div {...props} aria-disabled="true">Blocked item</div>{/snippet}
				</DropdownMenu.Item>
				<DropdownMenu.Item variant="destructive" onSelect={() => selected = 'Deleted'}>Delete item</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	</div>
	<output aria-label="Activations">{activations}</output>
	<output aria-label="Selection">{selected}</output>
	<div id="focus-link">Link destination</div>
</section>
