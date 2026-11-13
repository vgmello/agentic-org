<script lang="ts">
	import { DropdownMenu as DropdownMenuPrimitive } from "bits-ui";
	import { getMenuReturn } from "./dropdown-menu-return.svelte.js";

	let { ref = $bindable(null), ...restProps }: DropdownMenuPrimitive.TriggerProps = $props();
	const menu = getMenuReturn();
	$effect(() => {
		if (menu) menu.trigger = ref;
	});
</script>

<DropdownMenuPrimitive.Trigger bind:ref data-slot="dropdown-menu-trigger" {...restProps} />
