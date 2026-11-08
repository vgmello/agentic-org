<script lang="ts">
	import { goto, invalidate } from '$app/navigation';
	import { onMount } from 'svelte';
	import BuildingOffice from '@lucide/svelte/icons/building-2';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Plus from '@lucide/svelte/icons/plus';
	import { Alert, AlertDescription, AlertTitle } from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { describeFailure } from '$lib/create-flow';
	import { officeApi } from '$lib/office/api';
	import { formatDate, loadOrganizations, localOffice, openOrganization, openable, type Listing } from '$lib/open-flow';
	import { initialsOf } from '$lib/format';
	import type { AppError } from '$lib/proto';

	let listing = $state<Listing>({ kind: 'loading' });
	// The organization being opened, so every Open button is disabled meanwhile.
	let opening = $state<string | null>(null);
	let openError = $state<AppError | null>(null);

	async function load(retry = false) {
		if (retry) openError = null;
		listing = { kind: 'loading' };
		listing = await loadOrganizations(officeApi);
	}

	async function open(id: string) {
		if (opening) return;
		opening = id;
		openError = null;
		const result = await openOrganization(officeApi, id);
		if (!result.ok) {
			openError = result.error;
			opening = null;
			// Whatever went wrong (it's gone, its office went down, the office stopped), look again so
			// the list shows how things are now.
			await load();
			return;
		}
		try {
			await invalidate('app:connections');
			await goto('/agents', { replaceState: true });
		} catch {
			window.location.assign('/agents');
		}
	}

	onMount(load);

	const organizations = $derived(listing.kind === 'ready' ? listing.organizations : []);
	const failure = $derived(listing.kind === 'failed' ? describeFailure(listing.error, 'list') : undefined);
</script>

<svelte:head><title>Welcome · Agentic Org</title></svelte:head>

<h1 class="text-3xl/9 font-semibold tracking-tight text-white">Welcome to Agentic Org</h1>
<p class="mt-2 text-base/7 text-gray-400">Run an organization of AI agents. Open one you already have, or start a new one.</p>

<section class="mt-10" aria-labelledby="existing">
	<h2 id="existing" class="text-sm/6 font-semibold text-white">Organizations on this Mac</h2>

	<div aria-live="polite">
		{#if listing.kind === 'loading'}
			<p class="mt-3 flex items-center gap-x-2 text-sm/6 text-gray-400">
				<LoaderCircle class="size-4 animate-spin text-indigo-400" aria-hidden="true" />
				Looking for organizations on this Mac…
			</p>
		{:else if failure}
			<Alert variant="destructive" class="mt-3">
				<CircleAlert aria-hidden="true" />
				<AlertTitle>{failure.title}</AlertTitle>
				<AlertDescription>
					<p>{failure.hint}</p>
					<div class="mt-3 flex gap-x-2">
						{#if failure.action === 'open_login_items'}
							<Button variant="outline" size="sm" onclick={() => officeApi.openLoginSettings()}>Open Login Items</Button>
						{:else if failure.action === 'turn_on'}
							<Button variant="outline" size="sm" onclick={async () => { await officeApi.setLoginItem(true); await load(true); }}>Turn on</Button>
						{/if}
						<Button variant="outline" size="sm" onclick={() => load(true)}>Try again</Button>
					</div>
				</AlertDescription>
			</Alert>
		{:else if organizations.length === 0}
			<p class="mt-3 text-sm/6 text-gray-400">No organizations on this Mac yet.</p>
		{:else}
			<ul class="mt-3 divide-y divide-white/5 overflow-hidden rounded-xl bg-white/5 outline-1 -outline-offset-1 outline-white/10">
				{#each organizations as organization (organization.id)}
					{@const office = localOffice(organization)}
					{@const available = openable(organization)}
					<li data-testid="organization-row" data-organization={organization.id} class="flex items-center gap-x-4 px-4 py-3 {available ? '' : 'opacity-60'}">
						<span class="flex size-10 shrink-0 items-center justify-center rounded-lg bg-indigo-500/20 text-sm font-semibold text-indigo-300" aria-hidden="true">
							{initialsOf(organization.name)}
						</span>
						<span class="min-w-0 flex-1">
							<span class="flex items-center gap-x-2">
								<span class="truncate text-sm/6 font-semibold text-white">{organization.name}</span>
								{#if listing.kind === 'ready' && listing.current === organization.id}
									<Badge variant="secondary">Open now</Badge>
								{/if}
								{#if !available}<Badge variant="secondary">Unavailable</Badge>{/if}
							</span>
							<span class="block truncate text-xs/5 text-gray-400">
								{office ? `${office.name} · ${office.host}` : 'No office on this Mac'} · created {formatDate(organization.created_at)}
							</span>
						</span>
						<Button
							variant="outline"
							size="sm"
							disabled={!available || opening !== null}
							aria-label="Open {organization.name}"
							onclick={() => open(organization.id)}
						>
							{#if opening === organization.id}<LoaderCircle class="animate-spin" aria-hidden="true" />{/if}
							Open
						</Button>
					</li>
				{/each}
			</ul>
		{/if}

		{#if openError}
			<Alert variant="destructive" class="mt-3">
				<CircleAlert aria-hidden="true" />
				<AlertTitle>Couldn't open it</AlertTitle>
				<AlertDescription>{openError.message}</AlertDescription>
			</Alert>
		{/if}
	</div>
</section>

<a
	href="/create"
	class="group mt-8 flex items-center gap-x-4 rounded-xl bg-white/5 p-4 outline-1 -outline-offset-1 outline-white/10 hover:bg-white/10 focus-visible:outline-2 focus-visible:outline-indigo-500"
>
	<span class="flex size-10 shrink-0 items-center justify-center rounded-lg bg-indigo-500 text-white">
		<Plus class="size-5" aria-hidden="true" />
	</span>
	<span>
		<span class="block text-sm/6 font-semibold text-white">Create organization</span>
		<span class="block text-xs/5 text-gray-400">Set up a new organization. Its first office, HQ, runs on this Mac.</span>
	</span>
	<BuildingOffice class="ml-auto size-5 text-gray-500 group-hover:text-gray-300" aria-hidden="true" />
</a>
