<script lang="ts">
	import Crown from '@lucide/svelte/icons/crown';
	import type { Member } from '$lib/proto';

	let { owner, organization }: { owner: Member | undefined; organization: string } = $props();
</script>

<!-- The owner is shown once, above every view: never on a team card or in a count (OWNER-1). -->
{#if owner}
	<section aria-label="Owner" class="flex flex-wrap items-center gap-x-4 gap-y-1 rounded-xl bg-white/5 px-4 py-3 outline-1 -outline-offset-1 outline-white/10">
		<span class="flex size-10 shrink-0 items-center justify-center rounded-full bg-amber-400/15 text-amber-200 outline-1 -outline-offset-1 outline-amber-300/30">
			<Crown class="size-5" aria-hidden="true" />
		</span>
		<span class="min-w-0">
			<span class="block text-sm/6 font-semibold text-white">{owner.name} <span class="font-normal text-gray-400">(you)</span> · Owner of {organization}</span>
			<span class="block text-xs/5 text-gray-400">Full access to every team, member and project. Any agent can message, ask or assign you.</span>
		</span>
	</section>
{/if}
