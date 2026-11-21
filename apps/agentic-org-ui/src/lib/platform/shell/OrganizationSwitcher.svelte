<script lang="ts">
	import { goto, invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import Check from '@lucide/svelte/icons/check';
	import ChevronsUpDown from '@lucide/svelte/icons/chevrons-up-down';
	import Plus from '@lucide/svelte/icons/plus';
	import { toast } from 'svelte-sonner';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { officeApi } from '$lib/modules/offices/office-api';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import { toAppError, type Connection } from '$lib/proto';
	import { areaCapability } from './capabilities';

	/**
	 * Quick organization switcher in the shell sidebar header. Lists authorized organizations via
	 * `officeApi.peekOrganizations()` (backed by `GET /api/v1/organizations` in production),
	 * switches the active organization cleanly, and links to `/welcome` (All organizations).
	 */
	let { connection }: { connection: Connection } = $props();

	type OrgItem = { id: string; name: string; unavailable?: boolean };

	let open = $state(false);
	let loading = $state(false);
	let switching = $state<string>();
	let items = $state<OrgItem[]>([]);

	const session = $derived(
		appSession.state.kind === 'signed-in' ? appSession.state.session : undefined
	);
	const shownItems = $derived.by(() => {
		const current: OrgItem = {
			id: connection.organization.id,
			name: connection.organization.name
		};
		if (items.length === 0) return [current];
		return items.some((item) => item.id === current.id) ? items : [current, ...items];
	});

	async function fetchOrganizations() {
		try {
			return await officeApi.peekOrganizations();
		} catch (error) {
			if (toAppError(error).code !== 'not_connected') throw error;
			await officeApi.ensureRunning();
			return await officeApi.peekOrganizations();
		}
	}

	async function loadOrganizations() {
		if (loading) return;
		loading = true;
		try {
			const list = await fetchOrganizations();
			items = list.map((org) => ({
				id: org.id,
				name: org.name,
				...(org.offices.length === 0 ? { unavailable: true } : {})
			}));
		} catch {
			// Keep the current organization in the list if HQ is unreachable.
		} finally {
			loading = false;
		}
	}

	async function openById(id: string) {
		try {
			await officeApi.openOrganization(id);
		} catch (error) {
			if (toAppError(error).code !== 'not_connected') throw error;
			await officeApi.ensureRunning();
			await officeApi.openOrganization(id);
		}
	}

	async function switchTo(org: OrgItem) {
		if (org.id === connection.organization.id || switching || org.unavailable) return;
		switching = org.id;
		try {
			await openById(org.id);
			await invalidate('app:connections');
			await invalidate('app:org');
			await goto(resolve(areaCapability('/home').available ? '/home' : '/teams'));
		} catch (error) {
			const failure = toAppError(error);
			if (failure.code === 'not_found') {
				toast.error(`${org.name} is no longer on this HQ.`);
			} else {
				toast.error(failure.message);
			}
		} finally {
			switching = undefined;
		}
	}

	const focusRing =
		'focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus';
</script>

<DropdownMenu.Root
	bind:open
	onOpenChange={(next) => {
		if (next) void loadOrganizations();
	}}
>
	<DropdownMenu.Trigger>
		{#snippet child({ props })}
			<button
				{...props}
				type="button"
				aria-label="Switch organization"
				data-testid="org-switcher"
				class="flex size-9 shrink-0 items-center justify-center rounded-md text-fg-muted hover:bg-surface-raised hover:text-fg {focusRing}"
			>
				<ChevronsUpDown class="size-4" aria-hidden="true" />
			</button>
		{/snippet}
	</DropdownMenu.Trigger>
	<DropdownMenu.Content side="bottom" align="start" class="w-64 bg-surface ring-line">
		<DropdownMenu.Group>
			<DropdownMenu.Label class="text-xs font-medium text-fg-muted"
				>Organizations</DropdownMenu.Label
			>
			{#each shownItems as org (org.id)}
				{@const active = org.id === connection.organization.id}
				<DropdownMenu.Item
					class="flex min-h-9 items-center justify-between gap-2 max-[639px]:min-h-11"
					disabled={Boolean(org.unavailable || switching)}
					onSelect={() => void switchTo(org)}
				>
					<span class="min-w-0 truncate">{org.name}</span>
					{#if active}
						<span class="inline-flex shrink-0 items-center gap-1 text-xs text-brand-text">
							<Check class="size-3.5" aria-hidden="true" />
							<span class="sr-only">Current</span>
						</span>
					{:else if org.unavailable}
						<span class="shrink-0 text-xs text-fg-muted">Unavailable</span>
					{/if}
				</DropdownMenu.Item>
			{/each}
		</DropdownMenu.Group>
		<DropdownMenu.Separator class="bg-surface-overlay" />
		<DropdownMenu.Item
			class="min-h-9 max-[639px]:min-h-11"
			onSelect={() => void goto(resolve('/welcome'))}>All organizations</DropdownMenu.Item
		>
		{#if session?.admin}
			<DropdownMenu.Item
				class="flex min-h-9 items-center gap-2 max-[639px]:min-h-11"
				onSelect={() => void goto(resolve('/create'))}
			>
				<Plus class="size-4" aria-hidden="true" />
				<span>Create organization</span>
			</DropdownMenu.Item>
		{/if}
	</DropdownMenu.Content>
</DropdownMenu.Root>
