<!--
  @component
  Compact document attachment chip (PDF, DOCX, XLSX, MD, TXT, CSV, JSON) with MIME/size/pages/status
  metadata and authenticated download via the session-bound binary transport.
-->
<script lang="ts">
	import Download from '@lucide/svelte/icons/download';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import HeroIcon from '$lib/components/HeroIcon.svelte';
	import { ModalTooltip } from '$lib/components/ui/tooltip';
	import type { AttachmentMeta } from '$lib/platform/http/attachments';
	import { toAppError } from '$lib/proto';
	import { useAttachmentOrgId } from './context';
	import {
		artifactStatusLabel,
		formatBytes,
		mimeBadgeLabel,
		triggerAttachmentDownload
	} from './model';

	let {
		attachment,
		organizationId,
		compact = false
	}: {
		attachment: AttachmentMeta;
		organizationId?: string;
		compact?: boolean;
	} = $props();

	const getAmbientOrgId = useAttachmentOrgId();
	const orgId = $derived(organizationId ?? getAmbientOrgId?.() ?? '');
	const label = $derived(attachment.title || attachment.filename);
	const mimeLabel = $derived(mimeBadgeLabel(attachment.mime_type, attachment.filename));
	const sizeLabel = $derived(formatBytes(attachment.byte_count));
	const pagesLabel = $derived(
		attachment.page_count && attachment.page_count > 0 ? `${attachment.page_count} pp.` : undefined
	);
	const statusBadge = $derived(
		attachment.artifact_status !== 'final'
			? artifactStatusLabel(attachment.artifact_status)
			: undefined
	);

	let downloading = $state(false);
	let downloadError = $state<string | undefined>();

	async function handleDownload() {
		if (!orgId || downloading) return;
		downloading = true;
		downloadError = undefined;
		try {
			await triggerAttachmentDownload(orgId, attachment.id, attachment.filename);
		} catch (err) {
			downloadError = toAppError(err).message;
		} finally {
			downloading = false;
		}
	}
</script>

<li
	class="inline-flex max-w-full items-start gap-x-1.5 rounded bg-surface-overlay px-2 py-0.5 text-xs/5 text-fg-body"
	data-testid="document-attachment-chip"
	data-attachment-id={attachment.id}
>
	<HeroIcon class="mt-0.5 shrink-0 text-fg-muted" variant="micro" name="paper-clip" />
	<span class="min-w-0 [overflow-wrap:anywhere]">{label}</span>
	{#if !compact}
		<small class="shrink-0 font-mono text-[11px]/5 text-fg-muted">{mimeLabel}</small>
	{/if}
	<small class="shrink-0 text-[11px]/5 text-fg-muted">
		{sizeLabel}{#if pagesLabel}{' · ' + pagesLabel}{/if}
	</small>
	{#if statusBadge}
		<small
			class="shrink-0 rounded bg-surface-chip px-1 text-[11px]/5 font-medium text-fg-secondary"
			data-testid="artifact-status"
		>
			{statusBadge}
		</small>
	{/if}
	<ModalTooltip label="Download {label}">
		<button
			type="button"
			class="mt-0.5 inline-flex shrink-0 items-center justify-center rounded text-fg-muted transition-colors hover:text-fg focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-focus disabled:opacity-50 motion-reduce:transition-none [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
			aria-label="Download {label}"
			disabled={downloading || !orgId}
			onclick={() => void handleDownload()}
			data-testid="download-attachment"
		>
			{#if downloading}
				<LoaderCircle class="size-3.5 animate-spin motion-reduce:animate-none" aria-hidden="true" />
			{:else}
				<Download class="size-3.5" aria-hidden="true" />
			{/if}
		</button>
	</ModalTooltip>
	{#if downloadError}
		<small class="w-full text-[11px]/4 text-danger" role="alert">{downloadError}</small>
	{/if}
</li>
