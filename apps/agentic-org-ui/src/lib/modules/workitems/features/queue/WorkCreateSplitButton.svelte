<!--
  @component
  Primary split Create CTA for WorkToolbarMenus (New task / Report bug / New request)
  with hover+focus Esc-dismissible tooltip on the secondary dropdown chevron.
-->
<script lang="ts">
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import Plus from '@lucide/svelte/icons/plus';
	import { mergeProps } from 'bits-ui';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import type { TaskType } from '../../types';

	let {
		primaryCreate,
		secondaryCreates,
		disabled,
		menuMotion,
		itemHit,
		onCreate
	}: {
		primaryCreate: { kind: TaskType; label: string };
		secondaryCreates: readonly { kind: TaskType; label: string }[];
		disabled: boolean;
		menuMotion: string;
		itemHit: string;
		onCreate: (kind: TaskType) => void;
	} = $props();

	let menuOpen = $state(false);
</script>

<div class="work-create-group">
	<button
		type="button"
		{disabled}
		onclick={() => onCreate(primaryCreate.kind)}
		class="work-create-primary"
	>
		<Plus class="size-3.5 shrink-0" aria-hidden="true" />
		<span>{primaryCreate.label}</span>
	</button>
	<DropdownMenu.Root bind:open={menuOpen}>
		<Tooltip.Provider delayDuration={150}>
			<Tooltip.Root disabled={menuOpen}>
				<Tooltip.Trigger>
					{#snippet child({ props: tipProps })}
						<DropdownMenu.Trigger>
							{#snippet child({ props: menuProps })}
								<button
									{...mergeProps(tipProps, menuProps)}
									type="button"
									{disabled}
									aria-label="More create options"
									class="work-create-chevron"
								>
									<ChevronDown class="size-3.5" aria-hidden="true" />
								</button>
							{/snippet}
						</DropdownMenu.Trigger>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Content>More create options</Tooltip.Content>
			</Tooltip.Root>
		</Tooltip.Provider>
		<DropdownMenu.Content preventScroll={false} align="end" class="{menuMotion} w-44">
			{#each secondaryCreates as item (item.kind)}
				<DropdownMenu.Item onSelect={() => onCreate(item.kind)} class={itemHit}>
					{item.label}
				</DropdownMenu.Item>
			{/each}
		</DropdownMenu.Content>
	</DropdownMenu.Root>
</div>

<style>
	.work-create-group {
		display: inline-flex;
		align-items: stretch;
		border-radius: var(--exec-ctrl-radius, 6px);
		overflow: hidden;
		background: var(--exec-cta-bg, var(--primary));
		box-shadow: var(--exec-cta-shadow, none);
	}
	.work-create-primary,
	.work-create-chevron {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-height: var(--work-control-height, 36px);
		border: 0;
		background: transparent;
		color: var(--exec-on-accent, var(--on-emphasis));
		font-size: 0.8125rem;
		font-weight: 600;
		cursor: pointer;
		transition: background-color 120ms ease-out;
	}
	.work-create-primary {
		gap: 5px;
		padding: 0 11px 0 10px;
		white-space: nowrap;
	}
	.work-create-chevron {
		min-width: 28px;
		padding: 0 6px;
		border-inline-start: 1px solid var(--exec-border-hi, var(--line-strong));
	}
	.work-create-primary:hover:not(:disabled),
	.work-create-chevron:hover:not(:disabled) {
		background: var(--exec-cta-fill, var(--primary-hover));
	}
	.work-create-primary:disabled,
	.work-create-chevron:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}
	:is(.work-create-primary, .work-create-chevron):focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	@media (max-width: 720px), (pointer: coarse) {
		.work-create-primary {
			min-height: 44px;
		}
		.work-create-chevron {
			min-width: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.work-create-primary,
		.work-create-chevron {
			transition: none;
		}
	}
</style>
