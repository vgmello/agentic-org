<script lang="ts">
	import { Tooltip, mergeProps } from 'bits-ui';
	import { DropdownMenuItem } from './ui/dropdown-menu';

	let { fit, unavailable = false, ontoggle }: { fit: boolean; unavailable?: boolean; ontoggle: () => void } = $props();
	const uid = $props.id();
	const itemId = `${uid}-fit`;
	const descriptionId = `${uid}-reason`;
	const reason = 'The chart is already at its smallest readable size. Scroll to see the rest.';
</script>

<Tooltip.Provider delayDuration={0}>
	<Tooltip.Root disabled={!unavailable}>
		<!-- Keep one enabled menu item mounted; Tooltip's disabled flag must not hide it from roving focus. -->
		<DropdownMenuItem id={itemId} closeOnSelect={!unavailable} onSelect={(event) => unavailable ? event.preventDefault() : ontoggle()} class={unavailable ? 'text-gray-400 focus:text-gray-400' : undefined}>
			{#snippet child({ props: itemProps })}
				<Tooltip.Trigger id={itemId} type={undefined}>
					{#snippet child({ props: tooltipProps })}
						<div {...mergeProps(tooltipProps, itemProps)} data-disabled={undefined} aria-disabled={unavailable} aria-describedby={unavailable ? descriptionId : undefined}>{unavailable || !fit ? 'Fit to width' : 'Actual size'}</div>
					{/snippet}
				</Tooltip.Trigger>
			{/snippet}
		</DropdownMenuItem>
		<!-- Keep the description available after activation; outside pointer dismisses the parent menu. -->
		<Tooltip.Portal><Tooltip.Content id={descriptionId} role="tooltip" forceMount interactOutsideBehavior="defer-otherwise-close" sideOffset={6} class="z-[100] max-w-xs max-w-[calc(100vw-2rem)] rounded-md bg-gray-950 px-3 py-2 text-xs/5 text-white shadow-lg wrap-break-word data-[state=closed]:hidden">{reason}</Tooltip.Content></Tooltip.Portal>
	</Tooltip.Root>
</Tooltip.Provider>
