<!--
  @component
  The organization repository can't be reached (processes-ux §2 States): "Can't reach the organization repository.
  Showing the last copy from <time>." with Try again. One quiet callout below the page header; the actions it
  pauses say so beside themselves.
-->
<script lang="ts">
	import Callout from '$lib/components/Callout.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import type { StaleCopy } from '../types';
	import When from './When.svelte';

	let {
		stale,
		retrying = false,
		onretry
	}: { stale: StaleCopy; retrying?: boolean; onretry: () => void } = $props();
</script>

<div class="px-4 pt-6 min-[640px]:px-6 min-[1024px]:px-8">
	<Callout data-testid="stale">
		<p class="text-sm/6 font-semibold text-fg">
			Can't reach the organization repository. Showing the last copy from <When
				at={stale.copiedAt}
			/>.
		</p>
		<Button
			variant="outline"
			class="mt-3 min-w-24 {mobileButton}"
			disabled={retrying}
			aria-busy={retrying}
			onclick={onretry}>{retrying ? 'Trying again…' : 'Try again'}</Button
		>
	</Callout>
</div>
