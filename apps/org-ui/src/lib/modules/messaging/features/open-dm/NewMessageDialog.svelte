<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import type { ChatMember } from '$lib/modules/messaging/engine';
	import { CHAT_LIMITS } from '$lib/modules/messaging/types';

	/** New message (CHAT-3): one member opens a DM, up to seven more make a group of up to 8 with you. */
	let { open = $bindable(false), members, onsubmit, returnFocus }: { open: boolean; returnFocus?: () => void; members: ChatMember[]; onsubmit: (ids: string[]) => Promise<void> } = $props();

	let picked = $state<string[]>([]);
	let error = $state<string | undefined>();
	let busy = $state(false);
	$effect(() => {
		if (open) {
			untrack(() => {
				picked = [];
				error = undefined;
				busy = false;
			});
		}
	});
	const choices = $derived(members.filter((m) => !m.former && !m.owner));
	const max = CHAT_LIMITS.groupDmMax - 1;

	/** Navigation uses the same busy guard as the dialog's own dismissal controls. */
	export function dismiss() {
		if (!busy) open = false;
	}

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		if (busy || picked.length === 0) return;
		busy = true;
		error = undefined;
		try {
			await onsubmit(picked);
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
			<DialogTitle text={'New message'} />
			<DialogDescription>Pick who to talk to. Up to {max} others makes a group.</DialogDescription>
		</DialogHeader>
		<form class="space-y-4" onsubmit={submit}>
			<fieldset class="space-y-1">
				<legend class="sr-only">Who</legend>
				<ul class="max-h-64 space-y-1 overflow-y-auto" data-testid="dm-members">
					{#each choices as m (m.id)}
						<li>
							<label class="flex items-center gap-x-2 rounded-md px-2 py-1 text-sm/6 text-gray-200 hover:bg-white/5">
								<input type="checkbox" value={m.id} bind:group={picked} disabled={busy || (picked.length >= max && !picked.includes(m.id))} class="accent-indigo-500" />
								{m.name} <span class="text-xs/5 text-gray-400">{m.kind === 'agent' ? 'agent' : 'person'}</span>
							</label>
						</li>
					{/each}
				</ul>
			</fieldset>
			{#if picked.length >= max}<p class="text-xs/5 text-gray-400">A group has up to {CHAT_LIMITS.groupDmMax} members, you included.</p>{/if}
			{#if error}<p class="text-sm/6 text-red-400" role="alert">{error}</p>{/if}
			<DialogFooter>
				<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={busy || picked.length === 0}>{picked.length > 1 ? 'Start group' : 'Open'}</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
