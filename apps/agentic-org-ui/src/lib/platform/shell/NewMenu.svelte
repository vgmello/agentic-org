<!--
  @component
  Top-bar New (app-shell-ux §0.2): an outline menu button that opens the creation flows that exist, in the
  spec's order: Project, Work item, Request, Message, a separator, Hire an agent (the Hire page), Add a person.
  Project opens New project on the Projects page. Work item and Request open the Work items capture dialog.
  Below 640 px it is a 44 px icon button named New. `openMenu()` is what the N / Shift+N shortcut calls.
  Closing or finishing a flow returns focus to this button.
-->
<script lang="ts">
	import { goto, invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import { Tooltip, mergeProps } from 'bits-ui';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import AddPersonSheet from '$lib/modules/organization/features/add-person/AddPersonSheet.svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { hireUrl } from '$lib/modules/employees';
	import { newProjectUrl } from '$lib/modules/projects';
	import { CaptureDialog, taskHref } from '$lib/modules/workitems';
	import { areaCapability } from './capabilities';
	import { focusDestinationHeading } from './focus-destination';
	import { NewMessageDialog } from '$lib/modules/messaging';
	import { chatMembers } from '$lib/modules/messaging/model';
	import { toAppError } from '$lib/proto';
	import { orgApi } from '$lib/modules/organization/api';
	import { withoutFormer } from '$lib/modules/organization/model';
	import { sessionFor } from '$lib/modules/organization/session';
	import type { Connection, OrgSnapshot } from '$lib/proto';

	let { connection, shortcutHint = false }: { connection: Connection; shortcutHint?: boolean } =
		$props();

	const organizationId = $derived(connection.organization.id);
	const workAvailable = $derived(areaCapability('/work').available);
	let menuOpen = $state(false);
	let trigger = $state<HTMLButtonElement | null>(null);
	let snapshot = $state.raw<OrgSnapshot | undefined>();
	let flow = $state<'work_item' | 'request' | 'message' | 'person' | undefined>();
	let loading = $state(false);

	/** Opens the menu; the N / Shift+N shortcut calls this. */
	export function openMenu() {
		menuOpen = true;
	}

	// Message and Add a person read the current organization snapshot; Work item / Request load their own choices.
	async function startOrgFlow(name: 'message' | 'person') {
		loading = true;
		try {
			snapshot = (await orgApi.view(organizationId)).snapshot;
			flow = name;
		} catch (e) {
			toast.error("Couldn't open that form", { description: toAppError(e).message });
			trigger?.focus();
		} finally {
			loading = false;
		}
	}

	const session = $derived(sessionFor(orgApi, organizationId));

	async function openDm(ids: string[]) {
		const { sender: chatSender } = await import('$lib/modules/messaging/api');
		const accepted = await chatSender(organizationId)({ method: 'dm.open', members: ids });
		flow = undefined;
		await goto(
			resolve(
				accepted.conversation_id
					? `/chat?c=${encodeURIComponent(accepted.conversation_id)}`
					: '/chat'
			)
		);
	}
	async function added(r: { name: string; problems: string[] }) {
		await invalidate('app:org');
		if (r.problems.length === 0) toast.success(`Added ${r.name}.`);
		else
			toast.warning(`Added ${r.name}, but not everything went through.`, {
				description: r.problems.join(' ')
			});
	}
	const itemClass = 'min-h-9 max-[639px]:min-h-11';
</script>

<Tooltip.Provider delayDuration={150}>
	<DropdownMenu.Root bind:open={menuOpen}>
		<Tooltip.Root disabled={menuOpen}>
			<DropdownMenu.Trigger bind:ref={trigger}>
				{#snippet child({ props: menuProps })}
					<Tooltip.Trigger>
						{#snippet child({ props: tipProps })}
							<button
								{...mergeProps(menuProps, tipProps)}
								type="button"
								aria-label="New"
								aria-busy={loading}
								aria-keyshortcuts={shortcutHint ? 'N Shift+N' : undefined}
								class="inline-flex shrink-0 items-center justify-center gap-x-1.5 rounded-md text-sm/6 font-semibold text-fg outline-1 -outline-offset-1 outline-field-border hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:size-11 min-[640px]:h-9 min-[640px]:px-3 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11"
							>
								<MiniIcon name="plus" class="size-5 text-fg-muted" />
								<span class="max-[639px]:sr-only" aria-hidden="true">New</span>
								{#if shortcutHint}
									<kbd
										aria-hidden="true"
										class="rounded border border-line px-1.5 font-sans text-xs/5 font-normal text-fg-muted max-[1023px]:hidden"
										>N</kbd
									>
								{/if}
							</button>
						{/snippet}
					</Tooltip.Trigger>
				{/snippet}
			</DropdownMenu.Trigger>
			<Tooltip.Portal>
				<Tooltip.Content
					side="bottom"
					sideOffset={6}
					class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line min-[640px]:hidden"
					>New</Tooltip.Content
				>
			</Tooltip.Portal>
		</Tooltip.Root>
		<DropdownMenu.Content align="end" class="w-56 bg-surface ring-line">
			<!-- The Projects page opens New project; the sheet takes focus, and returns it to that page's heading. -->
			<DropdownMenu.Item class={itemClass} onSelect={() => goto(resolve(newProjectUrl() as '/'))}
				>Project</DropdownMenu.Item
			>
			{#if workAvailable}
				<DropdownMenu.Item class={itemClass} onSelect={() => (flow = 'work_item')}
					>Work item</DropdownMenu.Item
				>
				<DropdownMenu.Item class={itemClass} onSelect={() => (flow = 'request')}
					>Request</DropdownMenu.Item
				>
			{/if}
			<DropdownMenu.Item class={itemClass} onSelect={() => startOrgFlow('message')}
				>Message</DropdownMenu.Item
			>
			<DropdownMenu.Separator class="bg-surface-overlay" />
			<!-- Hire is a page of its own (our-org-in-the-app §7): opening it moves focus to its heading. -->
			<DropdownMenu.Item
				class={itemClass}
				onSelect={() => goto(resolve(hireUrl() as '/')).then(focusDestinationHeading)}
				>Hire an agent</DropdownMenu.Item
			>
			<DropdownMenu.Item class={itemClass} onSelect={() => startOrgFlow('person')}
				>Add a person</DropdownMenu.Item
			>
		</DropdownMenu.Content>
	</DropdownMenu.Root>
</Tooltip.Provider>

{#if workAvailable}
	<CaptureDialog
		bind:open={
			() => flow === 'work_item' || flow === 'request',
			(open) => {
				if (!open) flow = undefined;
			}
		}
		{organizationId}
		initialType={flow === 'request' ? 'request' : 'task'}
		allowTypeSwitch={flow === 'work_item'}
		oncreated={(created) => {
			void goto(resolve(taskHref(created.ref) as '/'));
		}}
	/>
{/if}

{#if snapshot}
	<NewMessageDialog
		bind:open={() => flow === 'message', (open) => (flow = open ? 'message' : undefined)}
		members={chatMembers(snapshot)}
		onsubmit={openDm}
	/>
	<AddPersonSheet
		bind:open={() => flow === 'person', (open) => (flow = open ? 'person' : undefined)}
		snapshot={withoutFormer(snapshot)}
		{session}
		organization={connection.organization.name}
		ondone={added}
	/>
{/if}
