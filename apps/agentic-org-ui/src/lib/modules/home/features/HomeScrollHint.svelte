<script lang="ts">
	import type { Attachment } from 'svelte/attachments';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';

	interface Props {
		scrollContainer: HTMLElement | null;
	}

	let { scrollContainer }: Props = $props();

	let visible = $state(false);
	let triggerMeasure: (() => void) | null = null;

	function observeScroll(container: HTMLElement | null): Attachment<HTMLDivElement> {
		return (node) => {
			if (!container) {
				visible = false;
				triggerMeasure = null;
				return;
			}

			let rafId = 0;
			let lastVisible = false;

			const measure = () => {
				const hintHeight = lastVisible ? node.offsetHeight : 0;
				const availableHeight = container.clientHeight + hintHeight;
				const hasOverflow = container.scrollHeight - availableHeight > 24;
				const nextVisible = hasOverflow && container.scrollTop <= 16;
				lastVisible = nextVisible;
				visible = nextVisible;
			};

			const scheduleMeasure = () => {
				if (typeof window === 'undefined') return;
				cancelAnimationFrame(rafId);
				rafId = window.requestAnimationFrame(measure);
			};

			triggerMeasure = scheduleMeasure;

			const resizeObserver = new ResizeObserver(scheduleMeasure);
			const observeChildren = () => {
				resizeObserver.disconnect();
				resizeObserver.observe(container);
				for (const child of container.children) {
					resizeObserver.observe(child);
				}
			};

			observeChildren();
			const mutationObserver = new MutationObserver(() => {
				observeChildren();
				scheduleMeasure();
			});
			mutationObserver.observe(container, { childList: true });

			container.addEventListener('scroll', measure, { passive: true });
			scheduleMeasure();

			return () => {
				triggerMeasure = null;
				cancelAnimationFrame(rafId);
				resizeObserver.disconnect();
				mutationObserver.disconnect();
				container.removeEventListener('scroll', measure);
			};
		};
	}

	function scrollDown() {
		const container = scrollContainer;
		if (!container) return;
		const prefersReduced =
			typeof window !== 'undefined' &&
			window.matchMedia('(prefers-reduced-motion: reduce)').matches;
		const behavior = prefersReduced ? 'auto' : 'smooth';

		const scorecard = container.querySelector<HTMLElement>('#exec-projects-heading');
		if (scorecard) {
			const containerRect = container.getBoundingClientRect();
			const scorecardRect = scorecard.getBoundingClientRect();
			const targetTop = Math.round(
				scorecardRect.top - containerRect.top + container.scrollTop - 16
			);
			if (targetTop > container.scrollTop + 24) {
				container.scrollTo({ top: targetTop, behavior });
				return;
			}
		}

		const step = Math.max(120, Math.round(container.clientHeight * 0.72));
		container.scrollTo({ top: container.scrollTop + step, behavior });
	}
</script>

<svelte:window onresize={() => triggerMeasure?.()} />

<div
	{@attach observeScroll(scrollContainer)}
	hidden={!visible}
	class="home-scroll-hint-bar flex shrink-0 items-center justify-center"
>
	<button
		type="button"
		data-testid="home-scroll-hint"
		onclick={scrollDown}
		class="home-scroll-hint-btn inline-flex min-h-10 items-center justify-center gap-1 rounded-md px-3 text-[13px] font-medium focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus focus-visible:outline-solid"
	>
		<span>Scroll for more</span>
		<ChevronDown class="size-4 shrink-0" aria-hidden="true" />
	</button>
</div>

<style>
	.home-scroll-hint-bar {
		position: relative;
		z-index: 10;
		background: var(--exec-scroll-hint-bg);
		padding-bottom: env(safe-area-inset-bottom);
	}

	.home-scroll-hint-bar[hidden] {
		display: none;
	}

	.home-scroll-hint-btn {
		color: var(--exec-scroll-hint-ink);
		cursor: pointer;
		transition: color 0.14s ease;
	}

	.home-scroll-hint-btn:hover,
	.home-scroll-hint-btn:focus-visible {
		color: var(--exec-scroll-hint-ink-hover);
	}

	@media (max-width: 720px), (pointer: coarse) {
		.home-scroll-hint-btn {
			min-height: 44px;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.home-scroll-hint-btn {
			transition: none;
		}
	}
</style>
