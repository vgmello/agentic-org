<!--
  @component
  Chat composer attachment strip wrapping the shared AttachmentComposer primitive.
  Preserves focus restoration (next -> prev -> Attach) and exposes paste/drop/picker helpers.
-->
<script module lang="ts">
	export interface DropZoneCallbacks {
		canDrop: () => boolean;
		onDragging: (active: boolean) => void;
		onFiles: (files: FileList) => void;
	}

	function hasDraggedFiles(event: DragEvent): boolean {
		return Boolean(
			event.dataTransfer?.types && Array.from(event.dataTransfer.types).includes('Files')
		);
	}

	/** Svelte 5 {@attach} drop-zone handler that avoids static-element a11y warnings. */
	export function fileDropZone(cb: DropZoneCallbacks) {
		return (node: HTMLElement) => {
			let depth = 0;
			const onDragEnter = (event: DragEvent) => {
				if (!cb.canDrop() || !hasDraggedFiles(event)) return;
				event.preventDefault();
				depth += 1;
				cb.onDragging(true);
			};
			const onDragOver = (event: DragEvent) => {
				if (!cb.canDrop() || !hasDraggedFiles(event)) return;
				event.preventDefault();
				if (event.dataTransfer) event.dataTransfer.dropEffect = 'copy';
			};
			const onDragLeave = (event: DragEvent) => {
				if (depth <= 0) return;
				event.preventDefault();
				depth = Math.max(0, depth - 1);
				if (depth === 0) cb.onDragging(false);
			};
			const onDrop = (event: DragEvent) => {
				if (!cb.canDrop()) return;
				depth = 0;
				cb.onDragging(false);
				const dropped = event.dataTransfer?.files;
				if (!dropped?.length) return;
				event.preventDefault();
				cb.onFiles(dropped);
			};
			node.addEventListener('dragenter', onDragEnter);
			node.addEventListener('dragover', onDragOver);
			node.addEventListener('dragleave', onDragLeave);
			node.addEventListener('drop', onDrop);
			return () => {
				node.removeEventListener('dragenter', onDragEnter);
				node.removeEventListener('dragover', onDragOver);
				node.removeEventListener('dragleave', onDragLeave);
				node.removeEventListener('drop', onDrop);
			};
		};
	}
</script>

<script lang="ts">
	import AttachmentComposer from '$lib/components/attachments/AttachmentComposer.svelte';
	import type { StagedAttachment } from '$lib/components/attachments/model';
	import { MESSAGE_ATTACHMENTS_MAX } from '$lib/platform/http/attachments';

	let {
		items = $bindable<StagedAttachment[]>([]),
		disabled = false,
		resetKey = '',
		attachButton,
		onItemsChange
	}: {
		items?: StagedAttachment[];
		disabled?: boolean;
		resetKey?: string;
		attachButton?: HTMLButtonElement;
		onItemsChange?: (next: StagedAttachment[]) => void;
	} = $props();

	let inner = $state<ReturnType<typeof AttachmentComposer> | undefined>();

	export function openPicker(): void {
		inner?.openPicker();
	}

	export function addFiles(incoming: FileList | readonly File[]): void {
		inner?.addFiles(incoming);
	}

	export function handlePaste(event: ClipboardEvent): boolean {
		return inner?.handlePaste(event) ?? false;
	}

	export function clear(revoke = true): void {
		inner?.clear(revoke);
	}
</script>

<AttachmentComposer
	bind:this={inner}
	bind:items
	{disabled}
	maxItems={MESSAGE_ATTACHMENTS_MAX}
	{resetKey}
	{attachButton}
	retainPreviewsOnUnmount={true}
	showVisionNote={true}
	{onItemsChange}
/>
