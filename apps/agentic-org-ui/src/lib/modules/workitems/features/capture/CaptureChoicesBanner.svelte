<!--
  @component
  Choices load-state banner (unavailable / unsupported / error) for CaptureDialog.
-->
<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { mobileButton } from '$lib/components/classes';
	import type { ChoicesState } from './capture-helpers';

	let {
		choicesState,
		choicesMessage,
		onretry
	}: {
		choicesState: ChoicesState;
		choicesMessage: string;
		onretry: () => void;
	} = $props();
</script>

{#if choicesState === 'unavailable'}
	<div
		class="rounded-md bg-surface-raised px-3 py-2.5 text-sm/6 text-fg-secondary ring-1 ring-line"
		data-testid="capture-choices-unavailable"
	>
		Creating work items isn't served on this server yet.
	</div>
{:else if choicesState === 'unsupported'}
	<div
		class="rounded-md bg-surface-raised px-3 py-2.5 text-sm/6 text-fg-secondary ring-1 ring-line"
		data-testid="capture-choices-unsupported"
	>
		<p class="font-medium text-fg">Not available in this version</p>
		<p class="mt-0.5 text-xs/5 text-fg-muted">
			{choicesMessage || "This version of the office doesn't support creating work items."}
		</p>
	</div>
{:else if choicesState === 'error'}
	<div
		class="flex flex-col gap-2 rounded-md bg-danger-emphasis/10 px-3 py-2.5 text-sm/6 text-danger-text ring-1 ring-danger-emphasis/20"
		role="alert"
		data-testid="capture-choices-error"
	>
		<p>Couldn't load teams and projects: {choicesMessage}</p>
		<div>
			<Button type="button" variant="outline" size="sm" class={mobileButton} onclick={onretry}>
				Retry
			</Button>
		</div>
	</div>
{/if}
