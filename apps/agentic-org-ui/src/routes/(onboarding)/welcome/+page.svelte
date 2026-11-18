<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { onMount } from 'svelte';
	import Plus from '@lucide/svelte/icons/plus';
	import AccountMenu from '$lib/platform/auth/AccountMenu.svelte';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import { signOutFlow } from '$lib/platform/auth/sign-out-flow.svelte';
	import { signInHref } from '$lib/platform/auth/return-target';
	import { readOrganizationList, readResponse } from '$lib/platform/http/wire';

	type Listing = { kind: 'loading' } | { kind: 'ready'; names: string[] } | { kind: 'failed' };
	let listing = $state<Listing>({ kind: 'loading' });
	const session = $derived(
		appSession.state.kind === 'signed-in' ? appSession.state.session : undefined
	);

	async function load() {
		listing = { kind: 'loading' };
		try {
			const response = await appSession.controller().read('/api/v1/organizations');
			const body = readResponse(response.body, readOrganizationList);
			listing =
				'result' in body
					? { kind: 'ready', names: body.result.organizations.map((o) => o.name) }
					: { kind: 'failed' };
		} catch {
			listing = { kind: 'failed' };
		}
	}

	onMount(async () => {
		await appSession.boot();
		if (appSession.state.kind !== 'signed-in') {
			await goto(resolve(signInHref('/welcome', location.origin)), { replaceState: true });
			return;
		}
		await load();
	});

	// Another tab signed out, or this one did: every tab shows the sign-in page (SIGN-20).
	$effect(() => {
		const state = appSession.state;
		if (state.kind === 'signed-out' && state.reason === 'signed-out')
			void goto(resolve('/sign-in?note=signed-out'), { replaceState: true });
		// SIGN-31: another account signed in from another tab; this tab keeps none of the previous account's data.
		else if (state.kind === 'signed-out' && state.reason === 'changed-account')
			void goto(resolve('/sign-in'), { replaceState: true });
		else if (state.kind === 'signed-out' && state.reason === 'ended')
			void goto(resolve(signInHref('/welcome', location.origin, 'ended')), { replaceState: true });
	});

	const signOut = () => signOutFlow.request();
</script>

<svelte:head><title>Organizations · Agentic Org</title></svelte:head>

{#if !session || listing.kind === 'loading'}
	<p class="text-sm/6 text-gray-400" aria-live="polite">Loading…</p>
{:else}
	<div class="mb-8 flex justify-end"><AccountMenu {session} /></div>
	{#if listing.kind === 'failed'}
		<div role="alert" class="rounded-md bg-red-500/10 p-3 text-sm/6 text-red-300">
			Can't reach HQ. Check that the instance is running on the HQ machine, then try again.
			<button type="button" class="ml-2 font-semibold underline" onclick={load}>Try again</button>
		</div>
	{:else if listing.names.length > 0}
		<h1 class="text-2xl/9 font-semibold tracking-tight text-white">Organizations</h1>
		<ul class="mt-4 space-y-2">
			{#each listing.names as name (name)}<li class="text-sm/6 text-white">{name}</li>{/each}
		</ul>
	{:else if session.admin}
		<!-- SIGN-9, administrator: what the administrator sees right after registering. -->
		<h1 class="text-2xl/9 font-semibold tracking-tight text-white">
			No organizations yet. Create one to get started.
		</h1>
		<div class="mt-6 flex items-center gap-x-4">
			<a
				href={resolve('/create')}
				class="inline-flex items-center gap-x-2 rounded-md bg-indigo-500 px-3 py-2 text-sm font-semibold text-white hover:bg-indigo-600 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-500"
			>
				<Plus class="size-4" aria-hidden="true" />Create organization
			</a>
			<button
				type="button"
				disabled={signOutFlow.signingOut}
				onclick={signOut}
				class="rounded-md bg-white/10 px-3 py-2 text-sm font-semibold text-white hover:bg-white/20"
				>Sign out</button
			>
		</div>
	{:else}
		<!-- SIGN-9, any other account. -->
		<h1 class="text-2xl/9 font-semibold tracking-tight text-white">
			This account can't open any organization on this HQ. Ask the owner of your organization to
			invite you again.
		</h1>
		<div class="mt-6">
			<button
				type="button"
				disabled={signOutFlow.signingOut}
				onclick={signOut}
				class="rounded-md bg-white/10 px-3 py-2 text-sm font-semibold text-white hover:bg-white/20"
				>Sign out</button
			>
		</div>
	{/if}
{/if}
