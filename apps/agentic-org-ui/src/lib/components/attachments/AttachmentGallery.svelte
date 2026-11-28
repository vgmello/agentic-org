<!--
  @component
  Splits AttachmentMeta[] into a responsive raster-image thumbnail grid (click -> ImageLightbox)
  and a compact list of DocumentAttachmentChip items.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import RotateCw from '@lucide/svelte/icons/rotate-cw';
	import type { AttachmentMeta } from '$lib/platform/http/attachments';
	import AttachmentImage, { type AttachmentImageStatus } from './AttachmentImage.svelte';
	import DocumentAttachmentChip from './DocumentAttachmentChip.svelte';
	import ImageLightbox from './ImageLightbox.svelte';
	import { isRasterImage } from './model';

	let {
		attachments,
		organizationId,
		compact = false
	}: {
		attachments: readonly AttachmentMeta[];
		organizationId?: string;
		compact?: boolean;
	} = $props();

	const images = $derived(attachments.filter((item) => isRasterImage(item.mime_type)));
	const documents = $derived(attachments.filter((item) => !isRasterImage(item.mime_type)));

	const gridClass = $derived(
		images.length === 1
			? 'grid max-w-sm grid-cols-1 gap-2'
			: images.length === 2
				? 'grid max-w-lg grid-cols-2 gap-2'
				: 'grid max-w-2xl grid-cols-2 gap-2 sm:grid-cols-3'
	);

	let lightboxOpen = $state(false);
	let lightboxIndex = $state(0);
	let lightboxOpener = $state<HTMLElement | null>(null);
	let imageStatuses = $state<Record<string, AttachmentImageStatus>>({});
	let retryTokens = $state<Record<string, number>>({});

	function openLightbox(index: number, button: HTMLButtonElement) {
		lightboxIndex = index;
		lightboxOpener = button;
		lightboxOpen = true;
	}

	function setImageStatus(id: string, status: AttachmentImageStatus) {
		if (imageStatuses[id] === status) return;
		imageStatuses = { ...imageStatuses, [id]: status };
	}

	async function retryImage(id: string, button: HTMLButtonElement) {
		const tile = button.closest('[data-testid="attachment-image-tile"]');
		retryTokens = { ...retryTokens, [id]: (retryTokens[id] ?? 0) + 1 };
		await tick();
		tile?.querySelector<HTMLButtonElement>('[data-testid="attachment-image-trigger"]')?.focus();
	}
</script>

{#if images.length > 0}
	<div class="mt-1.5 {gridClass}" data-testid="attachment-image-grid">
		{#each images as img, idx (img.id)}
			{@const label = img.title || img.filename}
			{@const status = imageStatuses[img.id]}
			<div
				class="group relative rounded-md border border-line bg-surface-raised transition-colors hover:border-line-strong motion-reduce:transition-none"
				data-testid="attachment-image-tile"
			>
				<AttachmentImage
					attachment={img}
					{organizationId}
					retryable={false}
					retryToken={retryTokens[img.id] ?? 0}
					onStatusChange={(next) => setImageStatus(img.id, next)}
					class={status === 'error' ? 'w-full rounded-b-none' : 'w-full'}
					imgClass={images.length === 1
						? compact
							? 'max-h-40 w-full object-contain'
							: 'max-h-64 w-full object-contain'
						: 'h-32 w-full object-cover'}
				/>
				{#if status === 'error'}
					<div
						class="flex items-center justify-center rounded-b-md border-t border-line bg-surface-sunken px-2 py-1"
					>
						<button
							type="button"
							class="inline-flex items-center gap-1 rounded px-2 py-0.5 text-xs/5 font-medium text-brand-text hover:text-brand-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
							aria-label="Retry {label}"
							data-testid="attachment-image-retry"
							onclick={(event) => void retryImage(img.id, event.currentTarget)}
						>
							<RotateCw class="size-3" aria-hidden="true" />
							Retry
						</button>
					</div>
				{:else if status !== 'authless'}
					<button
						type="button"
						class="absolute inset-0 cursor-pointer rounded-md focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-focus"
						aria-label="View {label}"
						data-testid="attachment-image-trigger"
						onclick={(event) => openLightbox(idx, event.currentTarget)}
					></button>
				{/if}
			</div>
		{/each}
	</div>
	<ImageLightbox
		bind:open={lightboxOpen}
		bind:index={lightboxIndex}
		{images}
		{organizationId}
		opener={lightboxOpener}
	/>
{/if}

{#if documents.length > 0}
	<ul class="mt-1 flex flex-wrap gap-1.5" aria-label="Attachments">
		{#each documents as doc (doc.id)}
			<DocumentAttachmentChip attachment={doc} {organizationId} {compact} />
		{/each}
	</ul>
{/if}
