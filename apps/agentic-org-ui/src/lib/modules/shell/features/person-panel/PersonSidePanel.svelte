<!--
  @component
  Lazily-loaded global Person Side Panel (Prototype Release c773480f1).
  - Non-modal docked right panel at >= 960px so the page stays usable beside it.
  - Modal Sheet (< 960px) with backdrop, focus trap, `registerModalBack`, and WebKit focus restore.
  - Strictly read-only profile facts; built-in agents (HR & Tess) never expose illegal mutations.
-->
<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { innerWidth } from 'svelte/reactivity/window';
	import { captureOverlayOpener, returnFocus } from '$lib/components/focus';
	import * as Sheet from '$lib/components/ui/sheet';
	import { registerModalBack } from '$lib/platform/workspace/modal-back';
	import type { OrgSnapshot } from '$lib/proto';
	import PersonPanelContent from './PersonPanelContent.svelte';
	import { PersonPanelLoader } from './person-panel-loader.svelte';
	import type { PersonPanelDataSource } from './types';

	let {
		open = false,
		memberId = null,
		openSeq = 0,
		opener = null,
		restoreFocusOnClose = true,
		organizationId,
		snapshot,
		source,
		onClose,
		onMessageMember
	}: {
		open: boolean;
		memberId: string | null;
		openSeq: number;
		opener?: HTMLElement | null;
		restoreFocusOnClose?: boolean;
		organizationId: string;
		snapshot?: OrgSnapshot;
		source: PersonPanelDataSource;
		onClose: (restoreFocus?: boolean) => void;
		onMessageMember?: (memberId: string) => void;
	} = $props();

	const titleId = 'person-panel-title';
	const loader = new PersonPanelLoader({
		loadMember: (orgId, id, signal) => source.loadMember(orgId, id, signal)
	});
	onDestroy(() => loader.dispose());

	const isNarrow = $derived((innerWidth.current ?? 1280) < 960);
	const isOpenNarrow = $derived(Boolean(open && memberId && isNarrow));
	const isOpenDesktop = $derived(Boolean(open && memberId && !isNarrow));

	let desktopPanelEl: HTMLElement | null = null;
	let lastHandledSeq = 0;
	let lastOpener = $state.raw<HTMLElement | null>(null);

	function trackDesktopPanel(node: HTMLElement) {
		desktopPanelEl = node;
		document.addEventListener('keydown', handleWindowKeydown);
		return () => {
			document.removeEventListener('keydown', handleWindowKeydown);
			if (desktopPanelEl === node) desktopPanelEl = null;
		};
	}

	const snapshotMember = $derived(
		memberId ? snapshot?.members.find((m) => m.id === memberId) : undefined
	);
	const displayName = $derived(loader.profile?.name ?? snapshotMember?.name ?? 'Member');
	const displayOwner = $derived(Boolean(loader.profile?.owner ?? snapshotMember?.owner));
	const canMessage = $derived(
		Boolean(
			memberId &&
			onMessageMember &&
			!displayOwner &&
			!loader.profile?.isSelf &&
			loader.profile?.status !== 'inactive'
		)
	);

	function fallbackHeading(): HTMLElement | null {
		return document.querySelector<HTMLElement>('#shell-page-title, main h1, main h2');
	}

	function focusHeading(root?: HTMLElement | null) {
		const heading =
			root?.querySelector<HTMLElement>(`#${titleId}`) ?? document.getElementById(titleId);
		heading?.focus({ preventScroll: true });
	}

	function closePanel(restoreFocus = true) {
		const target = opener ?? lastOpener;
		const within = desktopPanelEl;
		const wasDesktop = isOpenDesktop;
		loader.cancel();
		onClose(restoreFocus);
		if (wasDesktop && restoreFocus) {
			void returnFocus(target, { within, fallback: fallbackHeading });
		}
	}

	function hasVisibleOverlay(): boolean {
		const nodes = document.querySelectorAll<HTMLElement>(
			'[role="tooltip"], [role="menu"], [role="listbox"], [role="alertdialog"], [role="dialog"]'
		);
		for (const el of nodes) {
			if (el.getClientRects().length > 0) return true;
		}
		return false;
	}

	function handleWindowKeydown(event: KeyboardEvent) {
		if (!isOpenDesktop || event.defaultPrevented) return;
		if (event.key !== 'Escape' || event.isComposing) return;
		if (hasVisibleOverlay()) return;
		event.preventDefault();
		event.stopPropagation();
		closePanel(true);
	}

	function syncMemberLoad(node: HTMLElement) {
		const orgId = organizationId;
		const id = memberId;
		const seq = openSeq;
		const incomingOpener = opener;
		if (seq !== lastHandledSeq) {
			lastHandledSeq = seq;
			lastOpener = incomingOpener ?? captureOverlayOpener(node);
		}
		if (!id) return;
		node.scrollTop = 0;
		loader.load(orgId, id);
		void tick().then(() => {
			if (!loader.disposed && open && memberId === id) {
				focusHeading(node);
			}
		});
		return () => {
			loader.cancel();
		};
	}

	function attachModalBack() {
		return registerModalBack(() => {
			closePanel(true);
		});
	}
</script>

{#snippet panelBody()}
	<div class="flex min-h-0 flex-1 flex-col overflow-y-auto" {@attach syncMemberLoad}>
		<PersonPanelContent
			{titleId}
			{memberId}
			{snapshot}
			profile={loader.profile}
			openWork={loader.openWork}
			totalOpenWork={loader.totalOpenWork}
			loading={loader.loading}
			error={loader.error}
			{canMessage}
			onClose={closePanel}
			onRetry={() => {
				if (memberId) loader.load(organizationId, memberId);
			}}
			{onMessageMember}
		/>
	</div>
{/snippet}

{#if isOpenDesktop}
	<aside
		{@attach trackDesktopPanel}
		aria-labelledby={titleId}
		data-testid="person-side-panel"
		data-mode="docked"
		class="side-panel person-panel"
	>
		{@render panelBody()}
	</aside>
{/if}

<Sheet.Root
	open={isOpenNarrow}
	onOpenChange={(next) => {
		if (!next && open && isNarrow) closePanel(true);
	}}
>
	<Sheet.Content
		side="right"
		showCloseButton={false}
		opener={lastOpener}
		fallback={fallbackHeading}
		onOpenAutoFocus={(event) => {
			event.preventDefault();
			void tick().then(() => focusHeading());
		}}
		onCloseAutoFocus={(event) => {
			if (!restoreFocusOnClose || open) event.preventDefault();
		}}
		data-testid="person-side-panel"
		data-mode="modal"
		class="gap-0 border-0 border-s border-[var(--line)] bg-[var(--side-panel-surface,var(--exec-surface))] p-0 text-[var(--exec-ink)] shadow-[var(--side-panel-shadow)] motion-reduce:animate-none motion-reduce:transition-none data-[side=right]:w-full data-[side=right]:max-w-[25rem]"
	>
		<Sheet.Title class="sr-only">{displayName}</Sheet.Title>
		<Sheet.Description class="sr-only">Member profile summary and current work</Sheet.Description>
		<div class="flex min-h-0 flex-1 flex-col" {@attach attachModalBack}>
			{@render panelBody()}
		</div>
	</Sheet.Content>
</Sheet.Root>

<style>
	.side-panel.person-panel {
		--side-panel-width: 25rem;
		position: fixed;
		inset: 0 0 0 auto;
		z-index: 45;
		display: flex;
		flex-direction: column;
		width: min(100%, var(--side-panel-width));
		height: 100%;
		overflow: hidden;
		background: var(--side-panel-surface, var(--exec-surface));
		color: var(--exec-ink);
		border-inline-start: 1px solid var(--line);
		box-shadow: var(--side-panel-shadow);
		animation: pp-enter 200ms cubic-bezier(0.2, 0.7, 0.25, 1);
	}
	@keyframes pp-enter {
		from {
			transform: translateX(100%);
		}
		to {
			transform: translateX(0);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.side-panel.person-panel {
			animation: none;
		}
	}
</style>
