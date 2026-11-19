<!--
  @component
  One file of a development process (document 18): read it, or, for the organization's own, write it. The manifest
  is YAML; roles, skills and templates are Markdown (a skill starts with its front matter). A new role, skill or
  template asks for its key first. Every write names the revision the person saw; a refusal names the field and the
  text stays; a conflict says so and names the effect of saving again. Back closes the sheet only.
-->
<script lang="ts" module>
	import type { DevPart } from '../../types';

	export type PartMode =
		| { kind: 'read'; title: string; body: string; meta?: string }
		| { kind: 'edit'; part: DevPart; title: string; content: string; isNew?: boolean };
</script>

<script lang="ts">
	import {
		errorClass,
		helpClass,
		inputClass,
		mobileButton,
		textareaClass
	} from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetFooter,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { toAppError } from '$lib/office/api';
	import { registerModalBack } from '$lib/platform/shell/modal-back';
	import DocumentText from '../../components/DocumentText.svelte';
	import { KEY_HELP, validKey } from '../../present';

	let {
		revision,
		onsave,
		onconflict,
		onCloseAutoFocus
	}: {
		/** The development process's revision the person is looking at. */
		revision: number;
		/** Writes the file; rejects with the refusal. */
		onsave: (part: DevPart, content: string, expectedRevision: number) => Promise<void>;
		/** Someone saved first: the page reads the development process again, and `revision` follows it. */
		onconflict?: () => void;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	let open = $state(false);
	let mode = $state.raw<PartMode | undefined>();
	let key = $state('');
	let content = $state('');
	let expected = $state(0);
	let busy = $state(false);
	let error = $state<string | undefined>();
	let keyError = $state<string | undefined>();
	let conflict = $state(false);
	let keyField = $state<HTMLInputElement | null>(null);
	let textField = $state<HTMLTextAreaElement | null>(null);

	export function start(next: PartMode) {
		mode = next;
		content = next.kind === 'edit' ? next.content : '';
		key = '';
		expected = revision;
		busy = false;
		error = undefined;
		keyError = undefined;
		conflict = false;
		open = true;
	}

	$effect(() => {
		if (open) return registerModalBack(() => (open = false));
	});

	const editing = $derived(mode?.kind === 'edit' ? mode : undefined);
	const isManifest = $derived(editing?.part.kind === 'manifest');

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!editing || busy) return;
		let part = editing.part;
		if (editing.isNew) {
			const k = key.trim();
			if (!validKey(k)) {
				keyError = KEY_HELP;
				return void setTimeout(() => keyField?.focus(), 0);
			}
			part = { ...part, key: k } as DevPart;
		}
		busy = true;
		error = undefined;
		keyError = undefined;
		try {
			// After a conflict the next save goes over the revision the page has read since.
			await onsave(part, content, conflict ? revision : expected);
			open = false;
		} catch (e) {
			busy = false;
			const failure = toAppError(e);
			if (failure.source === 'api' && failure.code === 'conflict') {
				conflict = true;
				onconflict?.();
				error =
					'Someone else changed this development process while you were editing. Your text is kept; saving again replaces their change to this file.';
			} else error = failure.message;
			setTimeout(() => textField?.focus(), 0);
		}
	}
</script>

<Sheet bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<SheetContent
		side="right"
		{onCloseAutoFocus}
		onOpenAutoFocus={(e) => {
			if (!editing) return;
			e.preventDefault();
			(editing.isNew ? keyField : textField)?.focus();
		}}
		class="flex w-full flex-col overflow-y-auto data-[side=right]:w-full data-[side=right]:sm:max-w-2xl max-[639px]:[&>button]:size-11"
	>
		{#if mode}
			<SheetHeader class="pr-14">
				<SheetTitle class="text-base/7 font-semibold text-fg [overflow-wrap:anywhere]"
					>{mode.title}</SheetTitle
				>
				{#if mode.kind === 'read' && mode.meta}
					<SheetDescription class="[overflow-wrap:anywhere]">{mode.meta}</SheetDescription>
				{:else if editing}
					<SheetDescription>
						{isManifest
							? 'process.yaml: the name, summary, lifecycle, roles, kinds, sections, stages and tabs. A field not in the format is refused.'
							: 'Markdown. Agents read it as written.'}
					</SheetDescription>
				{/if}
			</SheetHeader>
			{#if mode.kind === 'read'}
				<div class="px-4 pb-8"><DocumentText body={mode.body} /></div>
			{:else}
				<form class="flex flex-1 flex-col gap-y-6 px-4 pb-4" onsubmit={submit} novalidate>
					{#if mode.isNew}
						<div>
							<Label for="part-key">Key</Label>
							<Input
								bind:ref={keyField}
								id="part-key"
								bind:value={key}
								autocomplete="off"
								spellcheck={false}
								class="mt-2 font-mono {inputClass}"
								aria-invalid={keyError ? true : undefined}
								aria-describedby={keyError ? 'part-key-error' : 'part-key-help'}
							/>
							{#if keyError}<p id="part-key-error" class={errorClass}>{keyError}</p>
							{:else}<p id="part-key-help" class={helpClass}>
									1 to 64 lowercase letters, digits and single hyphens; unique in this development
									process.
								</p>{/if}
						</div>
					{/if}
					<div class="flex flex-1 flex-col">
						<Label for="part-text">{isManifest ? 'process.yaml' : 'Text'}</Label>
						<textarea
							bind:this={textField}
							id="part-text"
							bind:value={content}
							rows="18"
							spellcheck={!isManifest}
							class="mt-2 min-h-64 flex-1 font-mono {textareaClass}"
							aria-invalid={error && !conflict ? true : undefined}
							aria-describedby={error ? 'part-error' : undefined}></textarea>
						{#if error}<p id="part-error" class={errorClass} role="alert" data-testid="part-error">
								{error}
							</p>{/if}
					</div>
					<SheetFooter class="flex-row flex-wrap justify-end gap-3 p-0">
						<Button
							type="button"
							variant="outline"
							class={mobileButton}
							disabled={busy}
							onclick={() => (open = false)}>Cancel</Button
						>
						<Button
							type="submit"
							class="min-w-28 {mobileButton}"
							aria-busy={busy}
							aria-disabled={busy}
							>{busy ? 'Saving…' : conflict ? 'Save over their change' : 'Save'}</Button
						>
					</SheetFooter>
				</form>
			{/if}
		{/if}
	</SheetContent>
</Sheet>
