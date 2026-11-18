<script lang="ts">
	import type { SessionView } from './session-gateway';
	import { signOutFlow } from './sign-out-flow.svelte';

	let { session }: { session: SessionView } = $props();
	let open = $state(false);
</script>

<div class="relative">
	<button
		type="button"
		aria-haspopup="true"
		aria-expanded={open}
		aria-controls="account-menu"
		onclick={() => (open = !open)}
		onkeydown={(event) => {
			if (event.key === 'Escape') open = false;
		}}
		class="rounded-md bg-white/10 px-3 py-1.5 text-sm font-semibold text-white hover:bg-white/20 focus-visible:outline-2 focus-visible:outline-indigo-500"
		>{session.account.login}</button
	>
	{#if open}
		<div
			id="account-menu"
			class="absolute right-0 z-10 mt-2 w-56 rounded-md bg-gray-900 py-1 outline-1 outline-white/10"
		>
			<p class="px-3 py-2 text-sm/6 text-gray-300">
				Signed in as <span class="font-semibold text-white">{session.account.login}</span>
			</p>
			{#if session.admin}<p class="px-3 pb-2 text-xs/5 text-gray-400">Administrator</p>{/if}
			<div class="border-t border-white/10"></div>
			<button
				type="button"
				disabled={signOutFlow.signingOut}
				onclick={() => signOutFlow.request()}
				class="block w-full px-3 py-2 text-left text-sm/6 text-white hover:bg-white/5 disabled:opacity-60 focus-visible:outline-2 focus-visible:outline-indigo-500"
				>{signOutFlow.signingOut ? 'Signing out…' : 'Sign out'}</button
			>
		</div>
	{/if}
</div>
