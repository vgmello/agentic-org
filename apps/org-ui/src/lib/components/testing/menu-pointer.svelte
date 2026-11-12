<script lang="ts">
	import { flushSync, tick } from 'svelte';
	import * as Menu from '$lib/components/ui/dropdown-menu';
	import * as Dialog from '$lib/components/ui/dialog';
	let { mode = 'normal' }: { mode?: string } = $props();
	let open = $state(false), shown = $state(true), disabled = $state(false), dialog = $state(false);
	let count = $state(0), checked = $state(false), choice = $state('one');
	let trigger = $state<HTMLElement | null>(null), caller = $state<HTMLButtonElement>(), content = $state<HTMLElement | null>(null);
	async function close(event: Event) {
		if (mode === 'prevent-close' || mode === 'prevent-close-only') { event.preventDefault(); if (mode === 'prevent-close') caller?.focus(); }
		if (mode === 'remove-trigger') shown = false;
		if (mode === 'disable-trigger') disabled = true;
		if (mode === 'caller-focus') { await tick(); caller?.focus(); }
		if (mode === 'reopen') { open = true; await tick(); content?.focus({ preventScroll: true }); }
	}
	function outside(event: PointerEvent) {
		if (mode === 'prevent-outside') event.preventDefault();
		if (mode === 'remove-content') flushSync(() => shown = false);
	}
</script>
<section aria-label="Pointer menu contracts" class="fixed inset-2 z-40 overflow-auto bg-gray-900 p-4">
	<button bind:this={caller}>Caller destination</button>
	<button onclick={() => count++}><span>Outside action</span></button>
	<label>Outside input <input aria-label="Outside input" /></label>
	<button disabled>Disabled outside</button>
	<div inert><button>Inert outside</button></div>
	<div data-testid="blank" class="h-10">Blank surface</div>
	{#if shown}
		<Menu.Root bind:open>
			<Menu.Trigger bind:ref={trigger} {disabled}>Contract commands</Menu.Trigger>
			<Menu.Content bind:ref={content} isValidEvent={mode === 'caller-valid' ? () => true : undefined} preventScroll={mode === 'scroll-lock' ? true : undefined} onCloseAutoFocus={close} onInteractOutside={outside}>
				<Menu.Item onSelect={() => count++}>Act in place</Menu.Item>
				<Menu.Item onSelect={(event) => { event.preventDefault(); dialog = true; open = false; }}>Open dialog</Menu.Item>
				<Menu.Item closeOnSelect={false} onSelect={(event) => event.preventDefault()}>Keep open</Menu.Item>
				<Menu.CheckboxItem bind:checked closeOnSelect={false}>Checked option</Menu.CheckboxItem>
				<Menu.RadioGroup bind:value={choice}><Menu.RadioItem value="one">Choice one</Menu.RadioItem><Menu.RadioItem value="two">Choice two</Menu.RadioItem><Menu.RadioItem value="close" closeOnSelect>Closing choice</Menu.RadioItem></Menu.RadioGroup>
				<Menu.Sub><Menu.SubTrigger>More commands</Menu.SubTrigger><Menu.SubContent><Menu.Item onSelect={() => count++}>Nested action</Menu.Item></Menu.SubContent></Menu.Sub>
			</Menu.Content>
		</Menu.Root>
	{/if}
	<Menu.Root><Menu.Trigger>Other menu</Menu.Trigger><Menu.Content><Menu.Item>Other action</Menu.Item></Menu.Content></Menu.Root>
	<output aria-label="Pointer activations">{count}</output>
	<output aria-label="Checkbox value">{String(checked)}</output>
	<output aria-label="Radio value">{choice}</output>
	<button onclick={() => dialog = true}>Outside dialog</button>
	<Dialog.Root bind:open={dialog}><Dialog.Content onCloseAutoFocus={(event) => { event.preventDefault(); trigger?.focus({ preventScroll: true }); }}><Dialog.Title>Destination dialog</Dialog.Title><Dialog.Description>Focus belongs to this layer.</Dialog.Description><input aria-label="Dialog field" /><button onclick={() => dialog = false}>Close destination</button></Dialog.Content></Dialog.Root>
	<div class="h-[150vh]" data-testid="scroll-space">Scrollable space</div>
</section>
