<!--
  @component
  Authenticated raster image attachment rendered exclusively via the bounded LRU blob: cache.
  Uses Svelte 5 {@attach} (zero $effect) to acquire/release cache refs and abort in-flight loads.
-->
<script lang="ts">
	import ImageOff from '@lucide/svelte/icons/image-off';
	import RotateCw from '@lucide/svelte/icons/rotate-cw';
	import { attachmentBlobCache } from '$lib/platform/http/attachment-blob-cache';
	import { hasAttachmentSessionPair, type AttachmentMeta } from '$lib/platform/http/attachments';
	import { toAppError } from '$lib/proto';
	import { cn } from '$lib/utils.js';
	import { useAttachmentOrgId } from './context';

	let {
		attachment,
		organizationId,
		class: className,
		imgClass,
		alt
	}: {
		attachment: AttachmentMeta;
		organizationId?: string;
		class?: string;
		imgClass?: string;
		alt?: string;
	} = $props();

	const getAmbientOrgId = useAttachmentOrgId();
	const orgId = $derived(organizationId ?? getAmbientOrgId?.() ?? '');
	const altText = $derived(alt ?? (attachment.title || attachment.filename));
	const aspectStyle = $derived(
		attachment.width && attachment.height
			? `aspect-ratio: ${attachment.width} / ${attachment.height};`
			: undefined
	);

	type LoadState =
		| { kind: 'loading'; key: string }
		| { kind: 'ready'; key: string; url: string }
		| { kind: 'authless'; key: string; message: string }
		| { kind: 'error'; key: string; message: string };

	let loadState = $state.raw<LoadState>({ kind: 'loading', key: '' });
	let retryCount = $state(0);

	function loadAttachmentBlob(targetOrgId: string, targetId: string, attempt: number) {
		return () => {
			void attempt;
			const key = `${targetOrgId}:${targetId}`;
			if (!targetOrgId || !targetId) {
				loadState = { kind: 'error', key, message: 'Image unavailable.' };
				return;
			}
			if (!hasAttachmentSessionPair()) {
				loadState = {
					kind: 'authless',
					key,
					message: 'Sign in with a browser session to view images.'
				};
				return;
			}
			const controller = new AbortController();
			let acquiredKey: string | undefined;
			let acquiredUrl: string | undefined;
			let active = true;
			loadState = { kind: 'loading', key };

			void attachmentBlobCache.acquire(targetOrgId, targetId, { signal: controller.signal }).then(
				(handle) => {
					if (!active) {
						attachmentBlobCache.release(handle.key, handle.objectUrl);
						return;
					}
					acquiredKey = handle.key;
					acquiredUrl = handle.objectUrl;
					loadState = { kind: 'ready', key, url: handle.objectUrl };
				},
				(err: unknown) => {
					if (!active || controller.signal.aborted) return;
					const appErr = toAppError(err);
					if (appErr.code === 'not_authorized' && !hasAttachmentSessionPair()) {
						loadState = { kind: 'authless', key, message: appErr.message };
						return;
					}
					loadState = { kind: 'error', key, message: appErr.message || 'Could not load image.' };
				}
			);

			return () => {
				active = false;
				controller.abort();
				if (acquiredKey) {
					attachmentBlobCache.release(acquiredKey, acquiredUrl);
				}
			};
		};
	}
</script>

<div
	{@attach loadAttachmentBlob(orgId, attachment.id, retryCount)}
	class={cn('relative overflow-hidden rounded-md bg-surface-raised', className)}
	style={loadState.kind === 'loading' ? aspectStyle : undefined}
	data-testid="attachment-image"
	data-attachment-id={attachment.id}
>
	{#if loadState.kind === 'ready'}
		<img
			src={loadState.url}
			alt={altText}
			width={attachment.width}
			height={attachment.height}
			loading="lazy"
			decoding="async"
			class={cn('block max-w-full object-contain', imgClass)}
		/>
	{:else if loadState.kind === 'loading'}
		<div
			class="flex min-h-24 w-full items-center justify-center bg-surface-overlay animate-pulse motion-reduce:animate-none"
			role="status"
			aria-label="Loading {altText}"
		>
			<span class="sr-only">Loading {altText}…</span>
		</div>
	{:else if loadState.kind === 'authless'}
		<div
			class="flex min-h-20 flex-col items-center justify-center gap-1 border border-line px-3 py-3 text-center text-xs/5 text-fg-muted"
			data-testid="attachment-image-authless"
		>
			<ImageOff class="size-4 text-fg-subtle" aria-hidden="true" />
			<span class="font-medium text-fg-body">{altText}</span>
			<span>{loadState.message}</span>
		</div>
	{:else}
		<div
			class="flex min-h-20 flex-col items-center justify-center gap-1.5 border border-line px-3 py-3 text-center text-xs/5 text-fg-muted"
			data-testid="attachment-image-error"
		>
			<ImageOff class="size-4 text-danger" aria-hidden="true" />
			<span>{loadState.message}</span>
			<button
				type="button"
				class="inline-flex items-center gap-1 rounded px-2 py-0.5 font-medium text-brand-text hover:text-brand-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
				onclick={() => (retryCount += 1)}
			>
				<RotateCw class="size-3" aria-hidden="true" />
				Retry
			</button>
		</div>
	{/if}
</div>
