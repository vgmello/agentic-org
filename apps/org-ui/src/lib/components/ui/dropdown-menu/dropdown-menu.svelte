<script lang="ts">
	import { DropdownMenu as DropdownMenuPrimitive } from "bits-ui";
	import { setMenuReturn } from "./dropdown-menu-return.svelte.js";

	let { open = $bindable(false), ...restProps }: DropdownMenuPrimitive.RootProps = $props();
	setMenuReturn();
</script>

<DropdownMenuPrimitive.Root bind:open {...restProps} />
