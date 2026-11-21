<script lang="ts">
	import { resolve } from '$app/paths';
	import { appSession } from './app-session.svelte';
	import type { SessionView } from './session-gateway';
	import { signOutFlow } from './sign-out-flow.svelte';

	let { session }: { session: SessionView } = $props();
	let open = $state(false);
	const authEnabled = $derived(appSession.authEnabled);
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
		>{authEnabled ? session.account.login : 'Local mode · Authentication off'}</button
	>
	{#if open}
		<div
			id="account-menu"
			class="absolute right-0 z-10 mt-2 w-60 rounded-md bg-surface-sunken py-1 outline-1 outline-line"
		>
			{#if authEnabled}
				<p class="px-3 py-2 text-sm/6 text-fg-secondary">
					Signed in as <span class="font-semibold text-fg">{session.account.login}</span>
				</p>
				{#if session.admin}<p class="px-3 pb-2 text-xs/5 text-fg-muted">Administrator</p>{/if}
			{:else}
				<p class="px-3 py-2 text-sm/6 text-fg-secondary">Local mode · Authentication off</p>
			{/if}
			<div class="border-t border-line"></div>
			<a
				href={resolve('/security')}
				onclick={() => (open = false)}
				class="block w-full px-3 py-2 text-left text-sm/6 text-fg hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-ring"
				>Security &amp; Access</a
			>
			{#if authEnabled}
				<button
					type="button"
					disabled={signOutFlow.signingOut}
					onclick={() => signOutFlow.request()}
					class="block w-full px-3 py-2 text-left text-sm/6 text-fg hover:bg-surface-raised disabled:opacity-60 focus-visible:outline-2 focus-visible:outline-ring"
					>{signOutFlow.signingOut ? 'Signing out…' : 'Sign out'}</button
				>
			{/if}
		</div>
	{/if}
</div>
