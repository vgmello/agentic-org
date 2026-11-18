<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import type { BrowseRow } from '$lib/modules/messaging/model';

	/**
	 * Browse channels (chat-ux §4.1): every channel the viewer may find, with its purpose, its team or project and its members,
	 * and **Join** or **Request invitation**. Restricted channels the viewer isn't in are never listed. No message bodies.
	 */
	let {
		open = $bindable(false),
		rows,
		officeRunning = true,
		returnFocus,
		onopen,
		onjoin,
		onrequest
	}: {
		open: boolean;
		rows: BrowseRow[];
		officeRunning?: boolean;
		returnFocus?: () => void;
		onopen: (conversationId: string) => void;
		onjoin: (conversationId: string) => Promise<void>;
		onrequest: (conversationId: string) => Promise<void>;
	} = $props();

	let busy = $state<string | undefined>();
	let error = $state<string | undefined>();

	export function dismiss() {
		if (!busy) open = false;
	}
	async function run(id: string, call: (id: string) => Promise<void>) {
		busy = id;
		error = undefined;
		try {
			await call(id);
		} catch (e) {
			error = (e as { message?: string }).message ?? 'That did not work.';
		} finally {
			busy = undefined;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent
		class="sm:max-w-xl max-[640px]:max-h-[100dvh] max-[640px]:max-w-none max-[640px]:overflow-y-auto"
		onCloseAutoFocus={(event) => {
			if (returnFocus) {
				event.preventDefault();
				returnFocus();
			}
		}}
	>
		<DialogHeader>
			<DialogTitle text="Browse channels" />
			<DialogDescription
				>Channels you can find in this organization. Project and team channels follow their members.</DialogDescription
			>
		</DialogHeader>
		<ul
			role="list"
			class="-mx-2 max-h-[60dvh] divide-y divide-white/5 overflow-y-auto"
			data-testid="browse-list"
		>
			{#each rows as r (r.conversation.id)}
				<li class="flex flex-wrap items-center gap-x-4 gap-y-2 px-2 py-3" data-testid="browse-row">
					<div class="min-w-0 flex-1 basis-56">
						<p class="text-sm/6 font-semibold text-white">#{r.conversation.name}</p>
						{#if r.conversation.purpose}<p class="text-sm/6 text-gray-300">
								{r.conversation.purpose}
							</p>{/if}
						<!-- eslint-disable svelte/no-useless-mustaches -- the spaces must be inside a mustache: Svelte trims a block's leading whitespace -->
						<p class="text-xs/5 text-gray-400">
							{r.scope ??
								(r.conversation.access === 'invite' ? 'By invitation' : 'Open to everyone')} · {r
								.conversation.members.length}
							{r.conversation.members.length === 1
								? 'member'
								: 'members'}{#if r.member}{" · You're a member"}{/if}
						</p>
						<!-- eslint-enable svelte/no-useless-mustaches -->
					</div>
					{#if r.action === 'open'}
						<Button
							type="button"
							size="sm"
							variant="ghost"
							onclick={() => onopen(r.conversation.id)}>Open</Button
						>
					{:else if r.action === 'join'}
						<Button
							type="button"
							size="sm"
							variant="outline"
							disabled={!officeRunning || !!busy}
							onclick={() => run(r.conversation.id, onjoin)}>Join</Button
						>
					{:else if r.action === 'request'}
						<Button
							type="button"
							size="sm"
							variant="outline"
							disabled={!officeRunning || !!busy}
							onclick={() => run(r.conversation.id, onrequest)}>Request invitation</Button
						>
					{:else if r.action === 'requested'}
						<p class="text-xs/5 text-gray-400">Invitation requested</p>
					{:else}
						<p class="text-xs/5 text-gray-400">
							Follows its {r.conversation.auto?.type === 'team' ? 'team' : 'project'}
						</p>
					{/if}
				</li>
			{/each}
		</ul>
		{#if error}<p class="text-sm/6 text-red-400" role="alert">{error}</p>{/if}
	</DialogContent>
</Dialog>
