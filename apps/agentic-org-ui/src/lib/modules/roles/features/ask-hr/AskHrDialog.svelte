<!--
  @component
  Ask HR to draft a role (roles-ux §1): what the role should do, then the HR lead's draft. Nothing is created here:
  the draft opens in the New role editor for the person to review. While the HR lead drafts the button reads
  "Drafting…" and the dialog can't be dismissed; a refusal keeps what was typed and says why. Openers call `start()`;
  each opening clears the last attempt's refusal but keeps what was typed. Back closes the dialog only (§6).
-->
<script lang="ts">
	import { errorClass, helpClass, mobileButton, textareaClass } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/office/api';
	import { registerModalBack } from '$lib/platform/shell/modal-back';
	import { rolesApi } from '../../api';
	import type { RoleDraft } from '../../types';

	let {
		organizationId,
		ondrafted,
		onclosed,
		onCloseAutoFocus
	}: {
		organizationId: string;
		/** The HR lead's draft, for the New role editor. */
		ondrafted: (draft: RoleDraft) => void;
		/** The dialog has finished closing: the next overlay may open. */
		onclosed?: () => void;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	let open = $state(false);
	let description = $state('');
	let error = $state<string | undefined>();
	let failure = $state<string | undefined>();
	let busy = $state(false);
	let field = $state<HTMLTextAreaElement | null>(null);

	/** Opens the dialog, without the last attempt's refusal; what was typed stays. */
	export function start() {
		error = undefined;
		failure = undefined;
		open = true;
	}

	/** Whether the dialog is open (or still closing): the list waits for it before opening the editor. */
	export function isOpen() {
		return open;
	}

	// Back with the dialog open closes it only, and focus returns to its opener (§6).
	$effect(() => {
		if (open) return registerModalBack(() => (open = false));
	});

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		if (!description.trim()) {
			error = 'Say what the role should do, in a sentence or two.';
			field?.focus();
			return;
		}
		busy = true;
		failure = undefined;
		try {
			const draft = await rolesApi.askHrToDraft(organizationId, description.trim());
			open = false;
			description = '';
			ondrafted(draft);
		} catch (e) {
			failure = toAppError(e).message;
		} finally {
			busy = false;
		}
	}
</script>

<Dialog
	bind:open
	onOpenChange={(next) => !next && busy && (open = true)}
	onOpenChangeComplete={(next) => !next && onclosed?.()}
>
	<DialogContent {onCloseAutoFocus} class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
		<form class="grid gap-6" onsubmit={submit} novalidate>
			<DialogHeader>
				<DialogTitle text="Ask HR to draft a role" />
				<DialogDescription class="[overflow-wrap:anywhere]">
					The HR lead writes a draft. You review it before anything is created.
				</DialogDescription>
			</DialogHeader>
			<div>
				<Label for="role-ask" class="block text-sm/6 font-medium text-fg"
					>What should this role do?</Label
				>
				<textarea
					bind:this={field}
					id="role-ask"
					bind:value={description}
					rows="4"
					disabled={busy}
					aria-invalid={error ? true : undefined}
					aria-describedby="role-ask-help{error ? ' role-ask-error' : ''}"
					oninput={() => (error = undefined)}
					class="{textareaClass} mt-2"></textarea>
				<p id="role-ask-help" class={helpClass}>
					For example: "Keep releases moving: cut them, check them, write the notes."
				</p>
				{#if error}<p id="role-ask-error" class={errorClass}>{error}</p>{/if}
			</div>
			{#if failure}
				<p class="text-sm/6 text-danger-text [overflow-wrap:anywhere]" role="alert">{failure}</p>
			{/if}
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={busy}
					onclick={() => (open = false)}>Cancel</Button
				>
				<Button type="submit" class="min-w-28 {mobileButton}" aria-busy={busy} aria-disabled={busy}
					>{busy ? 'Drafting…' : 'Ask HR'}</Button
				>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
