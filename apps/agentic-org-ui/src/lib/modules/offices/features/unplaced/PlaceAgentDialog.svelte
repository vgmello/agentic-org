<script lang="ts">
	import { untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { toAppError } from '$lib/office/api';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { Label } from '$lib/components/ui/label';
	import { errorClass, mobileButton, selectClass } from '$lib/modules/settings/components/classes';
	import type { Member } from '$lib/proto';
	import { officesApi } from '../../api';

	let {
		open = $bindable(false),
		agent,
		offices,
		organizationId,
		onchanged,
		onCloseAutoFocus
	}: {
		open: boolean;
		agent: Member | undefined;
		offices: { id: string; name: string }[];
		organizationId: string;
		onchanged: () => void | Promise<void>;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	let shown = $state<Member | undefined>();
	let office = $state('');
	let busy = $state(false);
	let error = $state<string | undefined>();
	$effect(() => {
		if (open)
			untrack(() => {
				shown = agent;
				office = '';
				error = undefined;
			});
	});

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!shown || busy) return;
		if (!office) {
			error = 'Choose an office.';
			return;
		}
		busy = true;
		error = undefined;
		try {
			await officesApi.placeAgent(organizationId, shown.id, office);
			open = false;
			toast.success(`${shown.name} is placed in ${offices.find((o) => o.id === office)?.name ?? 'the office'}.`);
			await onchanged();
		} catch (e) {
			error = toAppError(e).message;
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent {onCloseAutoFocus} class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
		<form class="grid gap-5" onsubmit={submit} novalidate>
			<DialogHeader>
				<DialogTitle text={`Place ${shown?.name ?? ''}`} />
				<DialogDescription>{shown?.name} keeps its teams, reporting and history. It runs in the office you choose.</DialogDescription>
			</DialogHeader>
			<div>
				<Label for="place-office" class="block text-sm/6 font-medium text-white">Office</Label>
				<div class="mt-2">
					<select id="place-office" bind:value={office} class={selectClass} aria-invalid={error ? true : undefined} aria-describedby={error ? 'place-office-error' : undefined}>
						<option value="">Choose an office…</option>
						{#each offices as o (o.id)}<option value={o.id}>{o.name}</option>{/each}
					</select>
				</div>
				{#if error}<p id="place-office-error" class={errorClass} role="alert">{error}</p>{/if}
			</div>
			<DialogFooter>
				<Button type="button" variant="outline" class={mobileButton} disabled={busy} onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" class={mobileButton} aria-busy={busy} aria-disabled={busy}>{busy ? 'Placing…' : 'Place'}</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
