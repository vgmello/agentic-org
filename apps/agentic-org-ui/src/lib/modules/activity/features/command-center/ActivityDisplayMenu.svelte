<script lang="ts">
	import { mergeProps, Tooltip } from 'bits-ui';
	import SlidersHorizontal from '@lucide/svelte/icons/sliders-horizontal';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { tooltipContentClass } from '$lib/components/classes';
	import { uiPrefs } from '$lib/platform/workspace/ui-preferences.svelte';

	let { open = $bindable(false) }: { open?: boolean } = $props();
</script>

<DropdownMenu.Root bind:open>
	<Tooltip.Root delayDuration={250} ignoreNonKeyboardFocus disabled={open}>
		<DropdownMenu.Trigger>
			{#snippet child({ props: menuProps })}
				<Tooltip.Trigger>
					{#snippet child({ props: tipProps })}
						<button
							{...mergeProps(menuProps, tipProps)}
							type="button"
							aria-label="Assembly line display"
							data-testid="assembly-display-btn"
							data-active={uiPrefs.assemblyDividers ? '' : undefined}
							class="activity-icon-btn min-h-9 min-w-9 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
						>
							<SlidersHorizontal class="size-5" aria-hidden="true" />
						</button>
					{/snippet}
				</Tooltip.Trigger>
			{/snippet}
		</DropdownMenu.Trigger>
		<Tooltip.Portal>
			<Tooltip.Content
				side="bottom"
				sideOffset={6}
				escapeKeydownBehavior="defer-otherwise-close"
				class={tooltipContentClass}
			>
				Assembly line display
			</Tooltip.Content>
		</Tooltip.Portal>
	</Tooltip.Root>
	<DropdownMenu.Content align="end" class="triage-menu-surface w-48">
		<DropdownMenu.Label>Display</DropdownMenu.Label>
		<DropdownMenu.CheckboxItem
			checked={uiPrefs.assemblyDividers}
			onCheckedChange={(checked) => uiPrefs.setAssemblyDividers(Boolean(checked))}
			data-testid="assembly-row-dividers-toggle"
			class="min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
		>
			Row dividers
		</DropdownMenu.CheckboxItem>
	</DropdownMenu.Content>
</DropdownMenu.Root>

<style>
	.activity-icon-btn {
		position: relative;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		border: 0;
		border-radius: var(--exec-ctrl-radius, 6px);
		background: transparent;
		color: var(--fg-secondary);
		cursor: pointer;
	}
	.activity-icon-btn:hover,
	.activity-icon-btn[aria-expanded='true'] {
		background: var(--surface-raised);
		color: var(--fg);
	}
	.activity-icon-btn[data-active] {
		color: var(--tone-accent-ink);
	}
	.activity-icon-btn[data-active]::after {
		content: '';
		position: absolute;
		inset: 5px 5px auto auto;
		width: 5px;
		height: 5px;
		border-radius: 50%;
		background: currentColor;
	}
	.activity-icon-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
</style>
