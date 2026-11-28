<!--
  @component
  Full-resolution image lightbox built on DialogContent so WebKit focus return & Escape trap work.
  Supports keyboard Left/Right gallery navigation, zoom/fit toggle, and authenticated download.
-->
<script lang="ts">
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Download from '@lucide/svelte/icons/download';
	import ZoomIn from '@lucide/svelte/icons/zoom-in';
	import ZoomOut from '@lucide/svelte/icons/zoom-out';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { ModalTooltip } from '$lib/components/ui/tooltip';
	import type { AttachmentMeta } from '$lib/platform/http/attachments';
	import { toAppError } from '$lib/proto';
	import AttachmentImage from './AttachmentImage.svelte';
	import { useAttachmentOrgId } from './context';
	import { formatBytes, triggerAttachmentDownload } from './model';

	let {
		open = $bindable(false),
		images,
		index = $bindable(0),
		organizationId,
		opener
	}: {
		open?: boolean;
		images: readonly AttachmentMeta[];
		index?: number;
		organizationId?: string;
		opener?: HTMLElement | null;
	} = $props();

	const getAmbientOrgId = useAttachmentOrgId();
	const orgId = $derived(organizationId ?? getAmbientOrgId?.() ?? '');

	let contentEl = $state<HTMLElement | null>(null);
	let zoomed = $state(false);
	let downloading = $state(false);
	let downloadError = $state<string | undefined>();

	const safeIndex = $derived(
		images.length > 0 ? Math.min(Math.max(0, index), images.length - 1) : 0
	);
	const current = $derived(images[safeIndex]);
	const hasMany = $derived(images.length > 1);
	const titleText = $derived(current ? current.title || current.filename : 'Image preview');
	const metaLine = $derived.by(() => {
		if (!current) return '';
		const parts: string[] = [];
		if (current.title && current.title !== current.filename) parts.push(current.filename);
		if (current.width && current.height) parts.push(`${current.width} × ${current.height}`);
		parts.push(formatBytes(current.byte_count));
		if (hasMany) parts.push(`${safeIndex + 1} of ${images.length}`);
		return parts.join(' · ');
	});

	function resetView() {
		zoomed = false;
		downloadError = undefined;
	}

	function step(delta: number) {
		if (!hasMany) return;
		index = (safeIndex + delta + images.length) % images.length;
		resetView();
	}

	function handleKeydown(event: KeyboardEvent) {
		if (!open || !hasMany) return;
		if (document.querySelector('[role="tooltip"]')) return;
		const target = event.target;
		if (
			target instanceof HTMLInputElement ||
			target instanceof HTMLTextAreaElement ||
			(target instanceof HTMLElement && target.isContentEditable)
		) {
			return;
		}
		if (event.key === 'ArrowLeft') {
			event.preventDefault();
			step(-1);
		} else if (event.key === 'ArrowRight') {
			event.preventDefault();
			step(1);
		}
	}

	async function handleDownload() {
		if (!current || !orgId || downloading) return;
		const targetId = current.id;
		downloading = true;
		downloadError = undefined;
		try {
			await triggerAttachmentDownload(orgId, targetId, current.filename);
		} catch (err) {
			if (current?.id === targetId) {
				downloadError = toAppError(err).message;
			}
		} finally {
			downloading = false;
		}
	}
</script>

<svelte:window onkeydown={handleKeydown} />

<Dialog bind:open onOpenChange={() => resetView()}>
	<DialogContent
		bind:ref={contentEl}
		{opener}
		onOpenAutoFocus={(event) => {
			resetView();
			event.preventDefault();
			contentEl?.focus({ preventScroll: true });
		}}
		class="flex max-h-[92dvh] max-w-[min(94vw,64rem)] flex-col gap-3 p-4 sm:max-w-[min(92vw,64rem)]"
		data-testid="image-lightbox"
	>
		<DialogHeader class="pr-10">
			<DialogTitle text={titleText} class="truncate text-base/6 font-semibold text-fg" />
			{#if metaLine}
				<DialogDescription class="text-xs/5 text-fg-muted">{metaLine}</DialogDescription>
			{/if}
		</DialogHeader>

		{#if current}
			<div
				class="relative flex min-h-48 flex-1 overflow-hidden rounded-lg border border-line bg-surface-sunken"
			>
				{#if hasMany}
					<ModalTooltip
						label="Previous image"
						side="bottom-start"
						class="absolute top-1/2 left-2 z-10 -translate-y-1/2"
					>
						<Button
							type="button"
							variant="secondary"
							size="icon-sm"
							class="[@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
							aria-label="Previous image"
							onclick={() => step(-1)}
						>
							<ChevronLeft aria-hidden="true" />
						</Button>
					</ModalTooltip>
				{/if}

				<div class="flex flex-1 overflow-auto p-2" data-testid="lightbox-scroll">
					<AttachmentImage
						attachment={current}
						organizationId={orgId}
						class={zoomed ? 'm-auto shrink-0 bg-transparent' : 'm-auto bg-transparent'}
						imgClass={zoomed
							? 'max-w-none transition-transform motion-reduce:transition-none'
							: 'max-h-[70dvh] max-w-full object-contain transition-transform motion-reduce:transition-none'}
					/>
				</div>

				{#if hasMany}
					<ModalTooltip label="Next image" class="absolute top-1/2 right-2 z-10 -translate-y-1/2">
						<Button
							type="button"
							variant="secondary"
							size="icon-sm"
							class="[@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
							aria-label="Next image"
							onclick={() => step(1)}
						>
							<ChevronRight aria-hidden="true" />
						</Button>
					</ModalTooltip>
				{/if}
			</div>

			<div class="flex flex-wrap items-center justify-between gap-2">
				<div class="min-w-0 text-xs/5 text-fg-muted">
					{#if downloadError}
						<span class="text-danger" role="alert">{downloadError}</span>
					{:else if current.description}
						<span>{current.description}</span>
					{/if}
				</div>
				<div class="flex items-center gap-1.5">
					<ModalTooltip label={zoomed ? 'Fit to screen' : 'Actual size'}>
						<Button
							type="button"
							variant="outline"
							size="icon-sm"
							class="[@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
							aria-label={zoomed ? 'Fit to screen' : 'Actual size'}
							aria-pressed={zoomed}
							onclick={() => (zoomed = !zoomed)}
						>
							{#if zoomed}
								<ZoomOut aria-hidden="true" />
							{:else}
								<ZoomIn aria-hidden="true" />
							{/if}
						</Button>
					</ModalTooltip>
					<ModalTooltip label="Download {current.filename}">
						<Button
							type="button"
							variant="outline"
							size="icon-sm"
							class="[@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
							aria-label="Download {current.filename}"
							disabled={downloading || !orgId}
							onclick={() => void handleDownload()}
						>
							<Download aria-hidden="true" />
						</Button>
					</ModalTooltip>
				</div>
			</div>
		{/if}
	</DialogContent>
</Dialog>
