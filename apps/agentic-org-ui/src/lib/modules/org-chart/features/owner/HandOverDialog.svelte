<!--
  @component
  Hand over ownership… (offices-and-teams-ux §7.1, OWNER-2), for the owner only. Lists active people in the
  organization who have an account; once one is chosen the dialog asks "Make Sam Ortiz the owner of Lumen Labs?
  You'll stay in Lumen Labs as a person, without the owner's access." and the owner's password. Ownership moves in
  one operation on the server (the members module's `handOverOwnership`); nothing is said to have changed until it
  answers. A wrong password is shown on the field, which takes focus, with everything else kept. With nobody to hand
  over to, it says how to get there instead of offering a button that can't work. Mount it in a {#key} that
  changes on each opening.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { errorClass, inputClass, mobileButton } from '$lib/components/classes';
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
	import { membersApi } from '$lib/modules/members';
	import { toAppError } from '$lib/proto';
	import type { ChartEmployee } from '../../types';
	import BusyLabel from '../../components/BusyLabel.svelte';

	let {
		open = $bindable(false),
		organizationId,
		organizationName,
		candidates,
		onCloseAutoFocus,
		ondone
	}: {
		open: boolean;
		organizationId: string;
		organizationName: string;
		/** Active people with an account, other than the owner. */
		candidates: ChartEmployee[];
		onCloseAutoFocus?: (event: Event) => void;
		/** The server confirmed the new owner. */
		ondone: (newOwner: { id: string; name: string }) => void;
	} = $props();

	const ids = $props.id();
	let choice = $state('');
	let password = $state('');
	let busy = $state(false);
	let choiceError = $state<string | undefined>();
	let passwordError = $state<string | undefined>();
	let failure = $state<string | undefined>();

	const chosen = $derived(candidates.find((c) => c.id === choice));

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		const form = event.currentTarget as HTMLFormElement;
		failure = undefined;
		choiceError = chosen ? undefined : `Choose who will own ${organizationName}.`;
		passwordError = password ? undefined : 'Enter your password.';
		if (choiceError || passwordError) {
			await tick();
			form.querySelector<HTMLElement>('[data-invalid], [aria-invalid="true"]')?.focus();
			return;
		}
		busy = true;
		try {
			await membersApi.handOverOwnership(organizationId, chosen!.id, password);
			open = false;
			ondone({ id: chosen!.id, name: chosen!.name });
		} catch (e) {
			const refused = toAppError(e);
			busy = false;
			// `wrong_password` is the members API's own refusal, outside the shared ApiErrorCode list, so it is read
			// before toAppError folds unknown codes into `unknown`.
			if ((e as { code?: unknown } | null)?.code === 'wrong_password') {
				passwordError = refused.message;
				await tick();
				document.getElementById(`${ids}-password`)?.focus();
			} else failure = `${refused.message} Nothing changed.`;
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent
		{onCloseAutoFocus}
		class="max-h-[calc(100dvh-2rem)] grid-cols-[minmax(0,1fr)] overflow-y-auto max-[639px]:max-w-[calc(100%-1rem)] max-[639px]:p-4"
	>
		<form class="grid gap-4" onsubmit={submit} novalidate>
			<DialogHeader class="pr-8">
				<DialogTitle text="Hand over ownership of {organizationName}" />
				<DialogDescription
					>The owner has full access to {organizationName}. There is always exactly one.</DialogDescription
				>
			</DialogHeader>

			{#if candidates.length === 0}
				<p class="text-sm/6 text-fg-secondary">
					Nobody can take over yet: only a person with an account can own {organizationName}. Open a
					person's profile and choose Invite to sign in, then come back once they have signed in.
				</p>
				<DialogFooter>
					<Button
						type="button"
						variant="outline"
						class={mobileButton}
						onclick={() => (open = false)}>Close</Button
					>
				</DialogFooter>
			{:else}
				<fieldset
					disabled={busy}
					aria-describedby={choiceError ? `${ids}-choice-error` : undefined}
				>
					<legend class="text-sm/6 font-medium text-fg">New owner</legend>
					<ul role="list" class="mt-2 divide-y divide-line-subtle rounded-md ring-1 ring-line">
						{#each candidates as c (c.id)}
							<li>
								<label
									class="flex cursor-pointer items-center gap-x-3 px-3 py-2 text-sm/6 has-focus-visible:outline-2 has-focus-visible:-outline-offset-2 has-focus-visible:outline-focus max-[639px]:min-h-11"
								>
									<input
										type="radio"
										name="{ids}-owner"
										value={c.id}
										bind:group={choice}
										data-invalid={choiceError ? true : undefined}
										onchange={() => (choiceError = undefined)}
										class="size-4 shrink-0 accent-primary focus-visible:outline-none"
									/>
									<span class="min-w-0">
										<span class="block text-fg [overflow-wrap:anywhere]">{c.name}</span>
										<span class="block text-xs/5 text-fg-muted [overflow-wrap:anywhere]"
											>@{c.handle}{c.homeTeam ? ` · ${c.homeTeam.name}` : ''}</span
										>
									</span>
								</label>
							</li>
						{/each}
					</ul>
					{#if choiceError}<p id="{ids}-choice-error" class={errorClass}>{choiceError}</p>{/if}
				</fieldset>

				{#if chosen}
					<p class="text-sm/6 text-fg [overflow-wrap:anywhere]" data-handover-effect>
						Make {chosen.name} the owner of {organizationName}? You'll stay in {organizationName} as a
						person, without the owner's access.
					</p>
				{/if}

				<div>
					<Label for="{ids}-password">Your password</Label>
					<Input
						id="{ids}-password"
						type="password"
						autocomplete="current-password"
						bind:value={password}
						disabled={busy}
						class="mt-2 {inputClass}"
						aria-invalid={passwordError ? true : undefined}
						aria-describedby={passwordError ? `${ids}-password-error` : undefined}
						oninput={() => (passwordError = undefined)}
					/>
					{#if passwordError}<p id="{ids}-password-error" class={errorClass}>
							{passwordError}
						</p>{/if}
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
					<Button type="submit" class="{mobileButton} [overflow-wrap:anywhere]" aria-busy={busy}
						><BusyLabel {busy} idle="Hand over ownership" working="Handing over…" /></Button
					>
				</DialogFooter>
			{/if}
		</form>
	</DialogContent>
</Dialog>
