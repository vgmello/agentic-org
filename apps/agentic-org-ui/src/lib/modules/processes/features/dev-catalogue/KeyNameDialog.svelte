<!--
  @component
  A key and a name for a development process: New development process… and Copy… (document 18). The key is unique in
  the organization; a refusal shows on its field and focuses it, and the dialog stays.
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
	import { toAppError } from '$lib/proto';
	import { registerModalBack } from '$lib/platform/workspace/modal-back';
	import { KEY_HELP, validKey } from '../../present';

	let {
		title,
		description,
		confirmLabel,
		busyLabel,
		onsubmit,
		onCloseAutoFocus
	}: {
		title: string;
		description: string;
		confirmLabel: string;
		busyLabel: string;
		/** Resolve to close; reject to show the reason (on `key` or `name` when the refusal names it). */
		onsubmit: (key: string, name: string) => Promise<void>;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	let open = $state(false);
	let key = $state('');
	let name = $state('');
	let errors = $state<{ key?: string; name?: string; other?: string }>({});
	let busy = $state(false);
	let keyField = $state<HTMLInputElement | null>(null);
	let nameField = $state<HTMLInputElement | null>(null);

	export function start(initial: { key?: string; name?: string } = {}) {
		key = initial.key ?? '';
		name = initial.name ?? '';
		errors = {};
		busy = false;
		open = true;
	}

	$effect(() => {
		if (open) return registerModalBack(() => (open = false));
	});

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		const next = { key: key.trim(), name: name.trim() };
		errors = {};
		if (!next.name) errors.name = 'Give it a name.';
		if (!validKey(next.key))
			errors.key = `Use ${KEY_HELP.charAt(0).toLowerCase()}${KEY_HELP.slice(1)}`;
		if (errors.name) return void setTimeout(() => nameField?.focus(), 0);
		if (errors.key) return void setTimeout(() => keyField?.focus(), 0);
		busy = true;
		try {
			await onsubmit(next.key, next.name);
			open = false;
		} catch (e) {
			busy = false;
			const failure = toAppError(e);
			const field = (e as { field?: unknown }).field;
			if (field === 'key' || field === 'name') {
				errors = { [field]: failure.message };
				setTimeout(() => (field === 'key' ? keyField : nameField)?.focus(), 0);
			} else errors = { other: failure.message };
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
			nameField?.focus();
		}}
		class="max-h-[calc(100dvh-2rem)] overflow-y-auto"
	>
		<form onsubmit={submit} novalidate>
			<DialogHeader>
				<DialogTitle text={title} />
				<DialogDescription class="[overflow-wrap:anywhere]">{description}</DialogDescription>
			</DialogHeader>
			<div class="mt-4 space-y-6">
				<div>
					<Label for="dev-name">Name</Label>
					<Input
						bind:ref={nameField}
						id="dev-name"
						bind:value={name}
						class="mt-2 {inputClass}"
						aria-invalid={errors.name ? true : undefined}
						aria-describedby={errors.name ? 'dev-name-error' : undefined}
					/>
					{#if errors.name}<p id="dev-name-error" class={errorClass}>{errors.name}</p>{/if}
				</div>
				<div>
					<Label for="dev-key">Key</Label>
					<Input
						bind:ref={keyField}
						id="dev-key"
						bind:value={key}
						autocomplete="off"
						spellcheck={false}
						class="mt-2 font-mono {inputClass}"
						aria-invalid={errors.key ? true : undefined}
						aria-describedby={errors.key ? 'dev-key-error' : 'dev-key-help'}
					/>
					{#if errors.key}<p id="dev-key-error" class={errorClass}>{errors.key}</p>
					{:else}<p id="dev-key-help" class={helpClass}>{KEY_HELP}</p>{/if}
				</div>
				{#if errors.other}<p class="text-sm/6 text-danger-text" role="alert">{errors.other}</p>{/if}
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
					>{busy ? busyLabel : confirmLabel}</Button
				>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
