<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { onMount } from 'svelte';
	import AccountMenu from '$lib/platform/auth/AccountMenu.svelte';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import { signInHref } from '$lib/platform/auth/return-target';
	import InstanceSecurityPanel from '$lib/modules/settings/features/security/InstanceSecurityPanel.svelte';

	const session = $derived(
		appSession.state.kind === 'signed-in' ? appSession.state.session : undefined
	);

	onMount(async () => {
		await appSession.boot();
		if (appSession.state.kind !== 'signed-in') {
			await goto(resolve(signInHref('/security', location.origin)), { replaceState: true });
		}
	});

	$effect(() => {
		const state = appSession.state;
		if (state.kind !== 'signed-out') return;
		if (state.reason === 'signed-out')
			void goto(resolve('/sign-in?note=signed-out'), { replaceState: true });
		else if (state.reason === 'changed-account')
			void goto(resolve('/sign-in'), { replaceState: true });
		else if (state.reason === 'ended')
			void goto(resolve(signInHref('/security', location.origin, 'ended')), { replaceState: true });
	});
</script>

<svelte:head><title>Security &amp; Access · Agentic Org</title></svelte:head>

{#if !session}
	<p class="text-sm/6 text-fg-muted" aria-live="polite">Loading…</p>
{:else}
	<div class="mb-6 flex flex-wrap items-center justify-between gap-3">
		<a
			href={resolve('/welcome')}
			class="text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
			>Back to organizations</a
		>
		<AccountMenu {session} />
	</div>

	<div class="mb-6 flex flex-col gap-1">
		<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">Security &amp; Access</h1>
		<p class="text-sm/6 text-fg-muted">
			Manage browser sign-in authentication and HTTPS for this HQ instance.
		</p>
	</div>

	<InstanceSecurityPanel standalone />
{/if}
