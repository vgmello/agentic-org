<!--
  @component
  One Settings section: a titled form with its own Save. The button reads its -ing word and keeps its width while HQ
  answers; "Saved" appears beside it only after HQ accepted the change; a refusal that names no field shows above the
  button, announced once. `locked` (no rights, or HQ unreachable) disables the fields, and the page says why once.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import type { SectionSave } from './save.svelte';

	let {
		id,
		title,
		description,
		save,
		locked,
		saveLabel = 'Save',
		busyLabel = 'Saving…',
		dirty,
		working = false,
		onsubmit,
		children
	}: {
		id: string;
		title: string;
		description: string;
		save: SectionSave;
		locked: boolean;
		saveLabel?: string;
		busyLabel?: string;
		/** Nothing to save until something changed: Save says so beside it instead of being silently disabled. */
		dirty: boolean;
		/** Work before the save itself (counting what a change would move): the button is busy meanwhile. */
		working?: boolean;
		onsubmit: (form: HTMLFormElement) => void;
		children: Snippet;
	} = $props();

	let unchanged = $state(false);
	const busy = $derived(save.busy || working);
</script>

<section aria-labelledby="{id}-heading" class="border-b border-line-subtle py-8">
	<div
		class="grid grid-cols-1 gap-x-8 gap-y-6 min-[1024px]:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]"
	>
		<div>
			<h4 id="{id}-heading" class="text-sm/6 font-semibold text-fg">{title}</h4>
			<p class="mt-1 max-w-prose text-sm/6 text-fg-muted">{description}</p>
		</div>
		<form
			novalidate
			class="min-w-0 max-w-2xl"
			onsubmit={(event) => {
				event.preventDefault();
				if (!dirty) {
					unchanged = true;
					return;
				}
				unchanged = false;
				onsubmit(event.currentTarget);
			}}
			oninput={() => (unchanged = false)}
		>
			<fieldset disabled={locked || busy} class="space-y-6">
				{@render children()}
			</fieldset>
			{#if save.refusal && !save.refusal.field}
				<div role="alert" class="mt-6 rounded-md border border-line p-3 text-sm/6">
					<p class="font-semibold text-danger-text">Not saved</p>
					<p class="mt-1 text-fg-secondary [overflow-wrap:anywhere]">{save.refusal.message}</p>
				</div>
			{/if}
			{#if !locked}
				<div class="mt-6 flex flex-wrap items-center gap-x-4 gap-y-2">
					<Button
						type="submit"
						variant="outline"
						disabled={busy}
						aria-busy={busy}
						class="min-w-24 {mobileButton}">{busy ? busyLabel : saveLabel}</Button
					>
					<p class="text-xs/5 text-fg-muted" role="status">
						{#if save.saved && !dirty}Saved{:else if unchanged}Nothing has changed yet{/if}
					</p>
				</div>
			{/if}
		</form>
	</div>
</section>
