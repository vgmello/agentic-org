<!--
  @component
  Reusable attachment staging strip + upload controller supporting file picker, drag-and-drop,
  clipboard paste, <100ms local blob: previews, per-item retry/remove, and zero $effect hooks.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import RotateCw from '@lucide/svelte/icons/rotate-cw';
	import HeroIcon from '$lib/components/HeroIcon.svelte';
	import { ModalTooltip } from '$lib/components/ui/tooltip';
	import {
		ATTACHMENT_ACCEPT,
		hasAttachmentSessionPair,
		MESSAGE_ATTACHMENTS_MAX,
		uploadAttachment,
		validateAttachmentFile
	} from '$lib/platform/http/attachments';
	import { toAppError } from '$lib/proto';
	import { useAttachmentOrgId } from './context';
	import {
		extractClipboardFiles,
		formatBytes,
		isRasterImage,
		markInterruptedUploads,
		revokeStagedPreviews,
		type StagedAttachment
	} from './model';

	let {
		items = $bindable<StagedAttachment[]>([]),
		organizationId,
		disabled = false,
		maxItems = MESSAGE_ATTACHMENTS_MAX,
		resetKey = '',
		attachButton,
		retainPreviewsOnUnmount = false,
		showVisionNote = true,
		onItemsChange
	}: {
		items?: StagedAttachment[];
		organizationId?: string;
		disabled?: boolean;
		maxItems?: number;
		resetKey?: string;
		attachButton?: HTMLButtonElement;
		/** Keep preview blob: URLs alive on unmount when a parent DraftStore owns their lifecycle. */
		retainPreviewsOnUnmount?: boolean;
		showVisionNote?: boolean;
		onItemsChange?: (next: StagedAttachment[]) => void;
	} = $props();

	const getAmbientOrgId = useAttachmentOrgId();
	const orgId = $derived(organizationId ?? getAmbientOrgId?.() ?? '');
	const focusClass =
		'focus-visible:outline-solid focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus';

	let fileInput: HTMLInputElement | undefined;
	let listEl: HTMLUListElement | undefined;
	let notice = $state<string | undefined>();
	let lastScopeKey: string | undefined;
	let seq = 0;

	const controllers: Record<string, AbortController> = Object.create(null);

	function bindFileInput(node: HTMLInputElement) {
		fileInput = node;
		return () => {
			if (fileInput === node) fileInput = undefined;
		};
	}

	function bindListEl(node: HTMLUListElement) {
		listEl = node;
		return () => {
			if (listEl === node) listEl = undefined;
		};
	}

	const hasImages = $derived(items.some((item) => isRasterImage(item.mimeType)));
	const authlessBlocked = $derived(!hasAttachmentSessionPair() && items.length === 0 && !!notice);

	function commit(next: StagedAttachment[]) {
		items = next;
		onItemsChange?.(next);
	}

	function patchItem(localId: string, updater: (item: StagedAttachment) => StagedAttachment) {
		const index = items.findIndex((entry) => entry.localId === localId);
		if (index < 0) return;
		const next = [...items];
		next[index] = updater(next[index]);
		commit(next);
	}

	function abortAllUploads() {
		for (const id of Object.keys(controllers)) {
			controllers[id].abort();
			delete controllers[id];
		}
	}

	function trackScope(scopeKey: string) {
		return () => {
			if (lastScopeKey !== undefined && lastScopeKey !== scopeKey && notice !== undefined) {
				notice = undefined;
			}
			lastScopeKey = scopeKey;
			return () => {
				abortAllUploads();
				if (retainPreviewsOnUnmount) {
					if (items.some((entry) => entry.status === 'uploading')) {
						onItemsChange?.(markInterruptedUploads(items));
					}
				} else {
					revokeStagedPreviews(items);
				}
			};
		};
	}

	async function startUpload(
		localId: string,
		file: File,
		filename: string,
		targetOrgId: string,
		uploadScope: string
	) {
		const controller = new AbortController();
		controllers[localId] = controller;
		try {
			const uploaded = await uploadAttachment(targetOrgId, file, {
				filename,
				signal: controller.signal
			});
			if (controller.signal.aborted || uploadScope !== resetKey) return;
			patchItem(localId, (entry) => ({
				...entry,
				status: 'ready',
				uploadId: uploaded.upload_id,
				uploaded,
				width: uploaded.width,
				height: uploaded.height,
				error: undefined
			}));
		} catch (err) {
			if (controller.signal.aborted || uploadScope !== resetKey) return;
			const message = toAppError(err).message || 'Upload failed.';
			patchItem(localId, (entry) => ({ ...entry, status: 'error', error: message }));
		} finally {
			if (controllers[localId] === controller) delete controllers[localId];
		}
	}

	export function interruptUploads(): StagedAttachment[] {
		abortAllUploads();
		if (notice !== undefined) notice = undefined;
		return markInterruptedUploads(items);
	}

	export function openPicker(): void {
		if (disabled) return;
		fileInput?.click();
	}

	export function addFiles(incoming: FileList | readonly File[]): void {
		if (disabled) return;
		const list = Array.from(incoming);
		if (!list.length) return;
		notice = undefined;
		if (!orgId) {
			notice = 'Select an organization before attaching files.';
			return;
		}
		if (!hasAttachmentSessionPair()) {
			notice =
				'Attachments require a signed-in browser session and are unavailable in local authless mode.';
			return;
		}
		let current = [...items];
		const errors: string[] = [];
		const queued: { localId: string; file: File; filename: string }[] = [];

		for (const file of list) {
			if (current.length >= maxItems) {
				errors.push(`Up to ${maxItems} attachments are allowed per message.`);
				break;
			}
			const check = validateAttachmentFile(file);
			if (!check.ok) {
				errors.push(`${file.name || 'File'}: ${check.message}`);
				continue;
			}
			const localId = `att-${Date.now()}-${++seq}`;
			const previewUrl = isRasterImage(check.mimeType) ? URL.createObjectURL(file) : undefined;
			current.push({
				localId,
				file,
				filename: check.filename,
				mimeType: check.mimeType,
				byteCount: file.size,
				previewUrl,
				status: 'uploading'
			});
			queued.push({ localId, file, filename: check.filename });
		}
		if (errors.length) notice = errors[0];
		if (!queued.length) return;
		commit(current);
		const targetOrgId = orgId;
		const uploadScope = resetKey;
		for (const job of queued) {
			void startUpload(job.localId, job.file, job.filename, targetOrgId, uploadScope);
		}
	}

	export function handlePaste(event: ClipboardEvent): boolean {
		if (disabled) return false;
		const pasted = extractClipboardFiles(event);
		if (!pasted.length) return false;
		event.preventDefault();
		addFiles(pasted);
		return true;
	}

	export function retry(localId: string): void {
		const target = items.find((entry) => entry.localId === localId);
		if (!target || !target.file || !orgId) return;
		patchItem(localId, (entry) => ({ ...entry, status: 'uploading', error: undefined }));
		void startUpload(localId, target.file, target.filename, orgId, resetKey);
	}

	export async function remove(localId: string): Promise<void> {
		const index = items.findIndex((entry) => entry.localId === localId);
		if (index < 0) return;
		const key = resetKey;
		const removed = items[index];
		const nextId = items[index + 1]?.localId;
		const prevId = items[index - 1]?.localId;
		controllers[localId]?.abort();
		delete controllers[localId];
		if (removed.previewUrl) URL.revokeObjectURL(removed.previewUrl);
		commit(items.filter((entry) => entry.localId !== localId));
		await tick();
		if (key !== resetKey) return;
		const buttons = [
			...(listEl?.querySelectorAll<HTMLButtonElement>('button[data-attachment-id]') ?? [])
		];
		const nextBtn = nextId ? buttons.find((btn) => btn.dataset.attachmentId === nextId) : undefined;
		const prevBtn = prevId ? buttons.find((btn) => btn.dataset.attachmentId === prevId) : undefined;
		(nextBtn ?? prevBtn ?? attachButton)?.focus();
	}

	export function clear(revoke = true): void {
		abortAllUploads();
		if (revoke) revokeStagedPreviews(items);
		notice = undefined;
		commit([]);
	}
</script>

<div {@attach trackScope(resetKey)} class="min-w-0">
	<input
		{@attach bindFileInput}
		type="file"
		multiple
		accept={ATTACHMENT_ACCEPT}
		class="hidden"
		aria-label="Attach files"
		onchange={(event) => {
			if (event.currentTarget.files?.length) addFiles(event.currentTarget.files);
			event.currentTarget.value = '';
		}}
	/>

	{#if notice}
		<p
			class="mt-2 text-xs/5 {authlessBlocked ? 'text-fg-secondary' : 'text-danger'}"
			role="alert"
			data-testid="attachment-notice"
		>
			{notice}
		</p>
	{/if}

	{#if items.length > 0}
		<ul
			{@attach bindListEl}
			class="mt-2 flex flex-wrap gap-1.5 max-[640px]:max-h-14 max-[640px]:overflow-y-auto [@media(max-height:32rem)]:max-h-14 [@media(max-height:32rem)]:overflow-y-auto"
			aria-label="Attached"
		>
			{#each items as item (item.localId)}
				{@const image = isRasterImage(item.mimeType)}
				<li
					class="flex min-w-0 max-w-full items-center gap-x-1.5 rounded bg-surface-overlay px-2 py-0.5 text-xs/5 text-fg-body {item.status ===
					'error'
						? 'ring-1 ring-danger'
						: ''}"
					data-testid="staged-attachment"
					data-status={item.status}
				>
					{#if image && item.previewUrl}
						<img
							src={item.previewUrl}
							alt={item.filename}
							class="my-0.5 size-7 shrink-0 rounded object-cover"
						/>
					{:else}
						<HeroIcon variant="micro" name="paper-clip" />
					{/if}
					<span class="min-w-0 truncate" title={item.filename}>{item.filename}</span>
					{#if image}
						<span class="shrink-0 text-[11px]/5 text-fg-muted">{formatBytes(item.byteCount)}</span>
					{/if}
					{#if item.status === 'uploading'}
						<LoaderCircle
							class="size-3.5 shrink-0 animate-spin text-fg-muted motion-reduce:animate-none"
							aria-label="Uploading {item.filename}"
						/>
					{:else if item.status === 'error'}
						<span class="shrink-0 text-[11px]/5 text-danger" title={item.error}>Failed</span>
						{#if item.file}
							<ModalTooltip label="Retry {item.filename}">
								<button
									type="button"
									class="inline-flex shrink-0 items-center justify-center text-fg-muted hover:text-fg [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11 {focusClass}"
									aria-label="Retry {item.filename}"
									onclick={() => retry(item.localId)}
								>
									<RotateCw class="size-3.5" aria-hidden="true" />
								</button>
							</ModalTooltip>
						{/if}
					{/if}
					<ModalTooltip label="Remove {item.filename}">
						<button
							type="button"
							data-attachment={item.filename}
							data-attachment-id={item.localId}
							class="inline-flex shrink-0 items-center justify-center text-fg-muted hover:text-fg [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11 {focusClass}"
							aria-label="Remove {item.filename}"
							onclick={() => void remove(item.localId)}
						>
							<HeroIcon class="text-current" variant="mini" name="x-mark" />
						</button>
					</ModalTooltip>
				</li>
			{/each}
		</ul>
		{#if showVisionNote && hasImages}
			<p class="mt-1 text-[11px]/4 text-fg-muted" data-testid="vision-capability-note">
				Images are stored natively; non-vision agents see filename and metadata unless rendered.
			</p>
		{/if}
	{/if}
</div>
