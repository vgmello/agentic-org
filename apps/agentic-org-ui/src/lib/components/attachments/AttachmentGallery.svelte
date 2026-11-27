<!--
  @component
  Splits AttachmentMeta[] into a responsive raster-image thumbnail grid (click -> ImageLightbox)
  and a compact list of DocumentAttachmentChip items.
-->
<script lang="ts">
	import type { AttachmentMeta } from '$lib/platform/http/attachments';
	import AttachmentImage from './AttachmentImage.svelte';
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

	function openLightbox(index: number, button: HTMLButtonElement) {
		lightboxIndex = index;
		lightboxOpener = button;
		lightboxOpen = true;
	}
</script>

{#if images.length > 0}
	<div class="mt-1.5 {gridClass}" data-testid="attachment-image-grid">
		{#each images as img, idx (img.id)}
			{@const label = img.title || img.filename}
			<button
				type="button"
				class="group relative block w-full overflow-hidden rounded-md border border-line bg-surface-raised text-left transition-colors hover:border-line-strong focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-focus motion-reduce:transition-none"
				aria-label="View {label}"
				data-testid="attachment-image-trigger"
				onclick={(event) => openLightbox(idx, event.currentTarget)}
			>
				<AttachmentImage
					attachment={img}
					{organizationId}
					class="w-full"
					imgClass={images.length === 1
						? compact
							? 'max-h-40 w-full object-contain'
							: 'max-h-64 w-full object-contain'
						: 'h-32 w-full object-cover'}
				/>
			</button>
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
