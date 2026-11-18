<!--
  @component
  The office's name with Rename, in the details sheet. Renaming is a form in place: Save reads Saving… while it works, and a name
  that is already used says so beside the field.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { toAppError } from '$lib/office/api';
	import { validateName } from '$lib/name';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { errorClass, inputClass } from '$lib/modules/settings/components/classes';

	let { name, onsave }: { name: string; onsave: (name: string) => Promise<void> } = $props();
	let editing = $state(false);
	let value = $state('');
	let busy = $state(false);
	let error = $state<string | undefined>();
	let input = $state<HTMLInputElement | null>(null);
	let renameButton = $state<HTMLButtonElement | null>(null);

	async function start() {
		value = name;
		error = undefined;
		editing = true;
		await tick();
		input?.select();
	}
	async function stop() {
		editing = false;
		await tick();
		renameButton?.focus();
	}
	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		const check = validateName(value, 'Enter a name for the office.');
		error = check.ok ? undefined : check.message;
		if (!check.ok) return input?.focus();
		const next = check.name;
		if (next === name) return stop();
		busy = true;
		try {
			await onsave(next);
			await stop();
		} catch (e) {
			error = toAppError(e).message;
			input?.focus();
		} finally {
			busy = false;
		}
	}
</script>

{#if editing}
	<form class="space-y-2" onsubmit={save} novalidate>
		<Label for="rename-office" class="sr-only">Office name</Label>
		<Input
			bind:ref={input}
			id="rename-office"
			bind:value
			autocomplete="off"
			aria-invalid={error ? true : undefined}
			aria-describedby={error ? 'rename-office-error' : undefined}
			class={inputClass}
		/>
		{#if error}<p id="rename-office-error" class={errorClass} role="alert">{error}</p>{/if}
		<div class="flex flex-wrap gap-2">
			<Button
				type="submit"
				size="sm"
				class="max-[639px]:min-h-11"
				aria-busy={busy}
				aria-disabled={busy}>{busy ? 'Saving…' : 'Save'}</Button
			>
			<Button
				type="button"
				variant="outline"
				size="sm"
				class="max-[639px]:min-h-11"
				disabled={busy}
				onclick={stop}>Cancel</Button
			>
		</div>
	</form>
{:else}
	<div class="flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
		<span class="min-w-0 text-white [overflow-wrap:anywhere]">{name}</span>
		<Button
			bind:ref={renameButton}
			variant="outline"
			size="sm"
			class="max-[639px]:min-h-11"
			aria-label="Rename {name}"
			onclick={start}>Rename</Button
		>
	</div>
{/if}
