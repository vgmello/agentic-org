<script lang="ts">
	import { heroicons, type HeroIconProps } from './heroicons';

	let { name, variant, class: className = 'text-gray-400' }: HeroIconProps = $props();
	// The props union allows only combinations present in this typed icon subset.
	const icon = $derived(heroicons[`${variant}-${name}` as keyof typeof heroicons]);
</script>

<svg {...icon.attributes} focusable="false" class="inline-block shrink-0 align-middle {icon.size} {className}">
	{#each icon.paths as path}<path {...path} />{/each}
</svg>
