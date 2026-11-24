<!--
  @component
  Import a process file (document 18): the office validates it like any write, gives it a new ID and adds it as a
  draft. A key that is taken is refused on the key field, where the person chooses another; a file that isn't a
  development process, or is too large, is refused on the file field.
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
	import { registerModalBack } from '$lib/platform/shell/modal-back';
	import { KEY_HELP, validKey } from '../../present';

	let {
		onimport,
		onCloseAutoFocus
	}: {
		/** Resolve to close; reject to show the reason. */
		onimport: (file: File, key?: string) => Promise<void>;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	let open = $state(false);
	let files = $state<FileList | undefined>();
	let key = $state('');
	let errors = $state<{ file?: string; key?: string; other?: string }>({});
	let busy = $state(false);
	let fileField = $state<HTMLInputElement | null>(null);
	let keyField = $state<HTMLInputElement | null>(null);

	export function start() {
		files = undefined;
		key = '';
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
		const file = files?.[0];
		errors = {};
		if (!file) {
			errors.file = 'Choose a process file.';
			return void setTimeout(() => fileField?.focus(), 0);
		}
		const chosen = key.trim();
		if (chosen && !validKey(chosen)) {
			errors.key = KEY_HELP;
			return void setTimeout(() => keyField?.focus(), 0);
		}
		busy = true;
		try {
			await onimport(file, chosen || undefined);
			open = false;
		} catch (e) {
			busy = false;
			const failure = toAppError(e);
			const field = (e as { field?: unknown }).field;
			if (field === 'key') {
				errors = { key: failure.message };
				setTimeout(() => keyField?.focus(), 0);
			} else if (field === 'file' || failure.code === 'too_large') {
				errors = { file: failure.message };
				setTimeout(() => fileField?.focus(), 0);
			} else errors = { other: failure.message };
		}
	}
</script>

<Dialog
	bind:open
	onOpenChange={(next) => !next && busy && (open = true)}
	onOpenChangeComplete={(next) => !next && (busy = false)}
>
	<DialogContent {onCloseAutoFocus} class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
		<form onsubmit={submit} novalidate>
			<DialogHeader>
				<DialogTitle text="Import a development process" />
				<DialogDescription
					>It arrives as a draft with a new ID. Nothing links it to the organization it came from.</DialogDescription
				>
			</DialogHeader>
			<div class="mt-4 space-y-6">
				<div>
					<Label for="import-file">Process file</Label>
					<input
						bind:this={fileField}
						bind:files
						id="import-file"
						type="file"
						class="mt-2 block w-full text-sm/6 text-fg-secondary file:mr-3 file:rounded-md file:border-0 file:bg-surface-overlay file:px-3 file:py-1.5 file:text-sm/6 file:font-semibold file:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:file:min-h-11"
						aria-invalid={errors.file ? true : undefined}
						aria-describedby={errors.file ? 'import-file-error' : 'import-file-help'}
					/>
					{#if errors.file}<p id="import-file-error" class={errorClass}>{errors.file}</p>
					{:else}<p id="import-file-help" class={helpClass}>
							A file downloaded from a development process, up to 5 MiB.
						</p>{/if}
				</div>
				<div>
					<Label for="import-key"
						>Key <span class="font-normal text-fg-muted">(optional)</span></Label
					>
					<Input
						bind:ref={keyField}
						id="import-key"
						bind:value={key}
						autocomplete="off"
						spellcheck={false}
						class="mt-2 font-mono {inputClass}"
						aria-invalid={errors.key ? true : undefined}
						aria-describedby={errors.key ? 'import-key-error' : 'import-key-help'}
					/>
					{#if errors.key}<p id="import-key-error" class={errorClass}>{errors.key}</p>
					{:else}<p id="import-key-help" class={helpClass}>
							Leave empty to keep the file's key. Choose another if it's taken here.
						</p>{/if}
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
					>{busy ? 'Importing…' : 'Import'}</Button
				>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
