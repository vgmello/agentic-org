<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { onDestroy, onMount, tick } from 'svelte';
	import Plus from '@lucide/svelte/icons/plus';
	import AccountMenu from '$lib/platform/auth/AccountMenu.svelte';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import { signOutFlow } from '$lib/platform/auth/sign-out-flow.svelte';
	import { signInHref } from '$lib/platform/auth/return-target';
	import { createOrganizationsApi } from '$lib/platform/organizations/api';
	import type { CreatingView, OrganizationList } from '$lib/platform/organizations/readers';
	import { selection } from '$lib/platform/organizations/selection';
	import { sleep } from '$lib/platform/organizations/sleep';
	import { openOrganization, organizationRows } from '$lib/platform/organizations/welcome-list';

	// The signed-in organizations page: SIGN-9 when there is none, otherwise the list (FC-018).
	type Listing =
		{ kind: 'loading' } | { kind: 'ready'; list: OrganizationList } | { kind: 'failed' };
	let listing = $state<Listing>({ kind: 'loading' });
	let selected = $state<string>();
	let opening = $state<string>();
	let notice = $state<{ title: string; hint: string }>();
	let noticeElement = $state<HTMLElement | null>(null);
	const session = $derived(
		appSession.state.kind === 'signed-in' ? appSession.state.session : undefined
	);
	const owner = $derived(
		session ? { instanceId: session.instance_id, accountId: session.account.id } : undefined
	);
	const rows = $derived(
		listing.kind === 'ready' ? organizationRows(listing.list.organizations, selected) : []
	);
	const creating = $derived(listing.kind === 'ready' ? listing.list.creating : []);

	const api = createOrganizationsApi({
		http: {
			read: (path, signal) => appSession.controller().read(path, signal),
			submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
		},
		instanceId: () => {
			if (!session) throw new Error('Sign in required');
			return session.instance_id;
		},
		// This page only reads: it never mints an operation.
		newId: () => {
			throw new Error('The organizations page starts no operation');
		}
	});
	const stop = new AbortController();
	// Creates this page is following now. One that HQ stops answering for leaves the set, so
	// Check again on its row, or the next reading of the list, follows it again.
	// Bookkeeping only, never rendered or read by an effect: a plain Set on purpose.
	// eslint-disable-next-line svelte/prefer-svelte-reactivity
	const followed = new Set<string>();
	/** Creates whose lookup stopped because HQ could not be reached. */
	let stalled = $state<string[]>([]);

	function show(list: OrganizationList) {
		listing = { kind: 'ready', list };
		selected = owner ? selection.current(owner) : undefined;
		for (const entry of list.creating) void follow(entry);
	}
	/** A create still in progress resumes its lookup; when it ends either way the list is read again. */
	async function follow(entry: CreatingView) {
		if (followed.has(entry.operation_id)) return;
		const id = entry.operation_id;
		followed.add(id);
		stalled = stalled.filter((other) => other !== id);
		const outcome = await api.follow(id, { sleep, signal: stop.signal });
		if (outcome.kind === 'stopped') return;
		followed.delete(id);
		if (outcome.kind === 'session') return;
		if (outcome.kind === 'unreachable') {
			// The row stays and says why nothing is known; nothing is sent until it is asked for.
			stalled = [...stalled, id];
			return;
		}
		const reply = await api.list(stop.signal);
		if (reply.kind === 'listed' && !stop.signal.aborted) show(reply.list);
	}
	async function load() {
		listing = { kind: 'loading' };
		notice = undefined;
		followed.clear();
		stalled = [];
		const reply = await api.list(stop.signal);
		if (stop.signal.aborted) return;
		if (reply.kind === 'listed') show(reply.list);
		else if (reply.kind === 'unreachable') listing = { kind: 'failed' };
		// An ended session is shown by the session itself (the effect below).
	}
	function checkAgain(entry: CreatingView, event: MouseEvent) {
		// The button leaves with the message: focus stays on the row it was in.
		(event.currentTarget as HTMLElement).closest('li')?.focus();
		void follow(entry);
	}
	async function say(next: { title: string; hint: string }) {
		notice = next;
		await tick();
		noticeElement?.focus();
	}
	async function open(id: string, name: string) {
		if (opening || !owner) return;
		opening = id;
		notice = undefined;
		try {
			const result = await openOrganization(api, id, stop.signal);
			if (stop.signal.aborted) return;
			if (result.kind === 'opened') {
				selection.select(owner, result.organization.id);
				selected = result.organization.id;
				// Entering the organization is the shell's: routing decides from the selection.
				await goto(resolve('/'));
			} else if (result.kind === 'vanished') {
				show(result.list);
				await say({ title: "Couldn't open it", hint: `${name} is no longer on this HQ.` });
			} else if (result.kind === 'unavailable') {
				show(result.list);
				await say({
					title: "Couldn't open it",
					hint: `${name}'s HQ office is unavailable right now.`
				});
			} else if (result.kind === 'unreachable')
				await say({
					title: "Couldn't open it",
					hint: "Can't reach HQ. Check that the instance is running on the HQ machine, then try again."
				});
		} finally {
			opening = undefined;
		}
	}
	onDestroy(() => stop.abort());

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

{#snippet createLink()}
	<a
		href={resolve('/create')}
		class="inline-flex items-center gap-x-2 rounded-md bg-primary px-3 py-2 text-sm font-semibold text-on-emphasis hover:bg-primary-hover focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
	>
		<Plus class="size-4" aria-hidden="true" />Create organization
	</a>
{/snippet}

{#if !session || listing.kind === 'loading'}
	<p class="text-sm/6 text-fg-muted" aria-live="polite">Loading…</p>
{:else}
	<div class="mb-8 flex justify-end"><AccountMenu {session} /></div>
	{#if listing.kind === 'failed'}
		<div role="alert" class="rounded-md bg-danger-emphasis/10 p-3 text-sm/6 text-danger-text">
			Can't reach HQ. Check that the instance is running on the HQ machine, then try again.
			<button type="button" class="ml-2 font-semibold underline" onclick={load}>Try again</button>
		</div>
		<!-- FC-018 (9): the list can't be read, and the administrator can still create. -->
		{#if session.admin}<div class="mt-6">{@render createLink()}</div>{/if}
	{:else if rows.length > 0 || creating.length > 0}
		<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">Organizations</h1>
		{#if notice}
			<div
				bind:this={noticeElement}
				tabindex="-1"
				role="alert"
				class="mt-4 rounded-md bg-danger-emphasis/10 p-3 text-sm/6 text-danger-text focus:outline-none"
			>
				<p class="font-semibold">{notice.title}</p>
				<p>{notice.hint}</p>
			</div>
		{/if}
		<ul
			aria-label="Organizations"
			class="mt-4 divide-y divide-line rounded-lg bg-surface-raised outline-1 -outline-offset-1 outline-line"
		>
			{#each rows as row (row.id)}
				<li data-testid="organization-row" class="flex items-center gap-x-4 px-4 py-3">
					<div class="min-w-0 flex-1">
						<p class="flex flex-wrap items-center gap-x-2 text-sm/6 font-semibold text-fg">
							<span class="truncate">{row.name}</span>
							{#if row.openNow}<span
									class="rounded-md bg-primary/20 px-1.5 py-0.5 text-xs/5 font-medium text-brand-text"
									>Open now</span
								>{/if}
							{#if row.unavailable}<span
									class="rounded-md bg-danger-emphasis/10 px-1.5 py-0.5 text-xs/5 font-medium text-danger-text"
									>Unavailable</span
								>{/if}
						</p>
						<p class="text-sm/6 text-fg-secondary">
							{row.office ??
								'No office'}{#if !row.unavailable}{` · ${row.status}`}{/if}{#if row.created}{` · Created ${row.created}`}{/if}
						</p>
					</div>
					<button
						type="button"
						aria-label="Open {row.name}"
						disabled={row.unavailable || opening !== undefined}
						onclick={() => open(row.id, row.name)}
						class="shrink-0 rounded-md bg-surface-overlay px-3 py-2 text-sm font-semibold text-fg hover:bg-tint/20 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:cursor-not-allowed disabled:opacity-50"
						>{opening === row.id ? 'Opening…' : 'Open'}</button
					>
				</li>
			{/each}
			{#each creating as entry (entry.operation_id)}
				<li
					data-testid="organization-creating"
					tabindex="-1"
					class="flex items-center gap-x-4 px-4 py-3 focus:outline-none"
				>
					<div class="min-w-0 flex-1">
						<p class="text-sm/6 font-semibold text-fg">{entry.name}</p>
						<p class="text-sm/6 text-fg-secondary" role="status">
							{stalled.includes(entry.operation_id) ? "Can't reach HQ" : 'Being created…'}
						</p>
					</div>
					{#if stalled.includes(entry.operation_id)}
						<button
							type="button"
							aria-label="Check again on {entry.name}"
							onclick={(event) => checkAgain(entry, event)}
							class="shrink-0 rounded-md bg-surface-overlay px-3 py-2 text-sm font-semibold text-fg hover:bg-tint/20 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
							>Check again</button
						>
					{/if}
				</li>
			{/each}
		</ul>
		{#if session.admin}<div class="mt-6">{@render createLink()}</div>{/if}
	{:else if session.admin}
		<!-- SIGN-9, administrator: what the administrator sees right after registering. -->
		<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">
			No organizations yet. Create one to get started.
		</h1>
		<div class="mt-6 flex items-center gap-x-4">
			{@render createLink()}
			<button
				type="button"
				disabled={signOutFlow.signingOut}
				onclick={signOut}
				class="rounded-md bg-surface-overlay px-3 py-2 text-sm font-semibold text-fg hover:bg-tint/20"
				>Sign out</button
			>
		</div>
	{:else}
		<!-- SIGN-9, any other account. -->
		<h1 class="text-2xl/9 font-semibold tracking-tight text-fg">
			This account can't open any organization on this HQ. Ask the owner of your organization to
			invite you again.
		</h1>
		<div class="mt-6">
			<button
				type="button"
				disabled={signOutFlow.signingOut}
				onclick={signOut}
				class="rounded-md bg-surface-overlay px-3 py-2 text-sm font-semibold text-fg hover:bg-tint/20"
				>Sign out</button
			>
		</div>
	{/if}
{/if}
