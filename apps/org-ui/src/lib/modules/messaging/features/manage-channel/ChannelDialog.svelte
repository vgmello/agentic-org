<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import type { ChatMember } from '$lib/modules/messaging/engine';
	import { channelName } from '$lib/modules/messaging/engine';

	/**
	 * Add a channel (name and members), rename one, or choose who is in one (CHAT-2). The owner is always in a channel they
	 * add; automatic channels cannot be edited here.
	 */
	let {
		open = $bindable(false),
		mode,
		members,
		initialName = '',
		initialMembers = [],
		onsubmit,
		returnFocus
	}: {
		open: boolean;
		returnFocus?: () => void;
		mode: 'create' | 'rename' | 'members';
		members: ChatMember[];
		initialName?: string;
		initialMembers?: string[];
		onsubmit: (result: { name: string; members: string[] }) => Promise<void>;
	} = $props();

	let name = $state('');
	let picked = $state<string[]>([]);
	let error = $state<string | undefined>();
	let busy = $state(false);
	let touched = $state(false);

	$effect(() => {
		if (open) {
			untrack(() => {
				name = initialName;
				picked = [...initialMembers];
				error = undefined;
				busy = false;
				touched = false;
			});
		}
	});

	const clean = $derived(channelName(name));
	const nameError = $derived(mode !== 'members' && touched && clean === '' ? 'Give the channel a name with letters or numbers.' : undefined);
	const choices = $derived(members.filter((m) => !m.former && !m.owner));
	const title = $derived(mode === 'create' ? 'Add a channel' : mode === 'rename' ? 'Rename channel' : 'Add people');

	/** Navigation uses the same busy guard as the dialog's own dismissal controls. */
	export function dismiss() {
		if (!busy) open = false;
	}

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		touched = true;
		if (busy || (mode !== 'members' && clean === '')) return;
		busy = true;
		error = undefined;
		try {
			await onsubmit({ name: clean, members: picked });
			open = false;
		} catch (err) {
			error = (err as { message?: string }).message ?? 'That did not work.';
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent class="sm:max-w-md max-[640px]:max-h-[100dvh] max-[640px]:max-w-none max-[640px]:overflow-y-auto" onCloseAutoFocus={(event) => { if (returnFocus) { event.preventDefault(); returnFocus(); } }}>
		<DialogHeader>
			<DialogTitle text={title} />
			<DialogDescription>{mode === 'members' ? 'People and agents in the channel can read it. Agents read only while they are members.' : 'Channels are lowercase, with dashes instead of spaces.'}</DialogDescription>
		</DialogHeader>
		<form class="space-y-4" onsubmit={submit} novalidate>
			{#if mode !== 'members'}
				<div class="space-y-2">
					<Label for="channel-name">Name</Label>
					<Input id="channel-name" bind:value={name} maxlength={60} disabled={busy} autocomplete="off" placeholder="release-v2" aria-invalid={nameError || error ? true : undefined} aria-describedby={nameError ? 'channel-name-error' : error ? 'channel-submit-error' : undefined} oninput={() => (error = undefined)} />
					{#if clean && clean !== name}<p class="text-xs/5 text-gray-400">It will be #{clean}.</p>{/if}
					{#if nameError}<p id="channel-name-error" class="text-sm/6 text-red-400" role="alert">{nameError}</p>{/if}
				</div>
			{/if}
			{#if mode !== 'rename'}
				<fieldset class="space-y-1">
					<legend class="text-sm/6 font-medium text-white">Members</legend>
					<ul class="max-h-56 space-y-1 overflow-y-auto" data-testid="channel-members">
						{#each choices as m (m.id)}
							<li>
								<label class="flex items-center gap-x-2 rounded-md px-2 py-1 text-sm/6 text-gray-200 hover:bg-white/5">
									<input type="checkbox" value={m.id} bind:group={picked} disabled={busy} class="accent-indigo-500" />
									{m.name} <span class="text-xs/5 text-gray-400">{m.kind === 'agent' ? 'agent' : 'person'}</span>
								</label>
							</li>
						{/each}
					</ul>
				</fieldset>
			{/if}
			{#if error}<p id="channel-submit-error" class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
			<DialogFooter>
				<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={busy}>{mode === 'create' ? 'Add channel' : mode === 'rename' ? 'Rename' : 'Save members'}</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
