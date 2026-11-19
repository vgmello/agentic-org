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
		class="rounded-md bg-surface-overlay px-3 py-1.5 text-sm font-semibold text-fg hover:bg-tint/20 focus-visible:outline-2 focus-visible:outline-ring"
		>{session.account.login}</button
	>
	{#if open}
		<div
			id="account-menu"
			class="absolute right-0 z-10 mt-2 w-56 rounded-md bg-surface-sunken py-1 outline-1 outline-line"
		>
			<p class="px-3 py-2 text-sm/6 text-fg-secondary">
				Signed in as <span class="font-semibold text-fg">{session.account.login}</span>
			</p>
			{#if session.admin}<p class="px-3 pb-2 text-xs/5 text-fg-muted">Administrator</p>{/if}
			<div class="border-t border-line"></div>
			<button
				type="button"
				disabled={signOutFlow.signingOut}
				onclick={() => signOutFlow.request()}
				class="block w-full px-3 py-2 text-left text-sm/6 text-fg hover:bg-surface-raised disabled:opacity-60 focus-visible:outline-2 focus-visible:outline-ring"
				>{signOutFlow.signingOut ? 'Signing out…' : 'Sign out'}</button
			>
		</div>
	{/if}
</div>
