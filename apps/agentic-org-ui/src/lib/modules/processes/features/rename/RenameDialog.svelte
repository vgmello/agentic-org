<!--
  @component
  Rename… for two documents that use one key at one level and scope (processes-ux §2 States): a new key for this
  document. Saving writes the next revision; a refusal shows on the field and focuses it, and the dialog stays.
-->
<script lang="ts">
	import { errorClass, helpClass, inputClass, mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/modules/offices/office-api';
	import { registerModalBack } from '$lib/platform/shell/modal-back';
	import { processesApi } from '../../api';
	import { KIND_WORD, draftOf, validKey } from '../../present';
	import type { ProcessDocument } from '../../types';

	let {
		organizationId,
		onrenamed,
		onCloseAutoFocus
	}: {
		organizationId: string;
		onrenamed: (doc: ProcessDocument) => void;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	let open = $state(false);
	let doc = $state.raw<ProcessDocument | undefined>();
	let key = $state('');
	let error = $state<string | undefined>();
	let busy = $state(false);
	let field = $state<HTMLInputElement | null>(null);

	export function start(next: ProcessDocument) {
		doc = next;
		key = next.key ?? '';
		error = undefined;
		busy = false;
		open = true;
	}

	$effect(() => {
		if (open) return registerModalBack(() => (open = false));
	});

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!doc || busy) return;
		const next = key.trim();
		if (!validKey(next)) {
			error = 'Use 1 to 64 lowercase letters, digits and single hyphens, like daily-standup.';
			field?.focus();
			return;
		}
		if (next === doc.key) {
			error = 'That is the key it has now. Choose a key no other document here uses.';
			field?.focus();
			return;
		}
		busy = true;
		error = undefined;
		try {
			const saved = await processesApi.update(
				organizationId,
				doc.id,
				{ ...draftOf(doc), key: next },
				doc.revision
			);
			open = false;
			onrenamed(saved);
		} catch (e) {
			error = toAppError(e).message;
			busy = false;
			setTimeout(() => field?.focus(), 0);
		}
	}
</script>

<Dialog
	bind:open
	onOpenChange={(next) => !next && busy && (open = true)}
	onOpenChangeComplete={(next) => !next && (busy = false)}
>
	<DialogContent
		{onCloseAutoFocus}
		onOpenAutoFocus={(e) => {
			e.preventDefault();
			field?.focus();
			field?.select();
		}}
		class="max-h-[calc(100dvh-2rem)] overflow-y-auto"
	>
		{#if doc}
			<form onsubmit={submit} novalidate>
				<DialogHeader>
					<DialogTitle text="Rename {doc.title}" />
					<DialogDescription class="[overflow-wrap:anywhere]"
						>A new key for this {KIND_WORD[doc.kind]}. Once the two keys differ, both apply again.</DialogDescription
					>
				</DialogHeader>
				<div class="mt-4">
					<Label for="rename-key">Key</Label>
					<Input
						bind:ref={field}
						id="rename-key"
						bind:value={key}
						autocomplete="off"
						spellcheck={false}
						class="mt-2 font-mono {inputClass}"
						aria-invalid={error ? true : undefined}
						aria-describedby={error ? 'rename-key-error' : 'rename-key-help'}
					/>
					{#if error}<p id="rename-key-error" class={errorClass} role="alert">{error}</p>
					{:else}<p id="rename-key-help" class={helpClass}>
							Lowercase letters, digits and hyphens. A lower level uses the same key to replace it.
						</p>{/if}
				</div>
				<DialogFooter class="mt-6">
					<Button
						type="button"
						variant="outline"
						class={mobileButton}
						disabled={busy}
						onclick={() => (open = false)}>Cancel</Button
					>
					<Button type="submit" class={mobileButton} aria-busy={busy} aria-disabled={busy}
						>{busy ? 'Renaming…' : 'Rename'}</Button
					>
				</DialogFooter>
			</form>
		{/if}
	</DialogContent>
</Dialog>
