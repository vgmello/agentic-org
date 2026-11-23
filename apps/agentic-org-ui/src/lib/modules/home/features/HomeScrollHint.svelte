<script lang="ts">
	import type { Attachment } from 'svelte/attachments';
	import { afterNavigate } from '$app/navigation';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';

	interface Props {
		scrollContainer: HTMLElement | null;
	}

	let { scrollContainer }: Props = $props();

	let visible = $state(false);
	let triggerMeasure: (() => void) | null = null;

	afterNavigate(() => {
		triggerMeasure?.();
	});

	function observeScroll(container: HTMLElement | null): Attachment<HTMLDivElement> {
		return () => {
			if (!container) {
				visible = false;
				triggerMeasure = null;
				return;
			}

			let rafId = 0;

			const measure = () => {
				const hasOverflow = container.scrollHeight - container.clientHeight > 24;
				visible = hasOverflow && container.scrollTop <= 16;
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

	function focusDestination(target: HTMLElement) {
		if (!target.hasAttribute('tabindex')) {
			target.setAttribute('tabindex', '-1');
		}
		target.focus({ preventScroll: true });
	}

	function scrollDown() {
		const container = scrollContainer;
		if (!container) return;
		const prefersReduced =
			typeof window !== 'undefined' &&
			window.matchMedia('(prefers-reduced-motion: reduce)').matches;
		const behavior = prefersReduced ? 'auto' : 'smooth';

		const containerRect = container.getBoundingClientRect();
		const scorecard = container.querySelector<HTMLElement>('#exec-projects-heading');
		if (scorecard) {
			const scorecardRect = scorecard.getBoundingClientRect();
			const targetTop = Math.round(
				scorecardRect.top - containerRect.top + container.scrollTop - 16
			);
			if (targetTop > container.scrollTop + 24) {
				container.scrollTo({ top: targetTop, behavior });
				focusDestination(scorecard);
				return;
			}
		}

		const viewport = container.clientHeight;
		const maxScroll = Math.max(0, container.scrollHeight - viewport);
		const targetTop = Math.min(
			maxScroll,
			container.scrollTop + Math.max(120, Math.round(viewport * 0.72))
		);
		container.scrollTo({ top: targetTop, behavior });

		const candidates = container.querySelectorAll<HTMLElement>('h2, h3, section[aria-labelledby]');
		let fallbackTarget: HTMLElement | null = null;
		for (const candidate of candidates) {
			const headingOffsetTop =
				candidate.getBoundingClientRect().top - containerRect.top + container.scrollTop;
			if (headingOffsetTop >= targetTop && headingOffsetTop <= targetTop + viewport - 24) {
				fallbackTarget = candidate;
				break;
			}
		}
		focusDestination(fallbackTarget ?? container);
	}
</script>

<svelte:window onresize={() => triggerMeasure?.()} />

<div
	{@attach observeScroll(scrollContainer)}
	hidden={!visible}
	class="home-scroll-hint-bar flex items-center justify-center"
>
	<button
		type="button"
		data-testid="home-scroll-hint"
		onclick={scrollDown}
		class="home-scroll-hint-btn inline-flex min-h-10 items-center justify-center gap-1.5 rounded-full px-3.5 text-[13px] font-medium focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus focus-visible:outline-solid"
	>
		<span>Scroll for more</span>
		<ChevronDown class="size-4 shrink-0" aria-hidden="true" />
	</button>
</div>

<style>
	.home-scroll-hint-bar {
		position: absolute;
		inset-inline: 0;
		bottom: 0;
		z-index: 20;
		pointer-events: none;
		padding: 12px 16px calc(8px + env(safe-area-inset-bottom));
		background: linear-gradient(to top, var(--exec-scroll-hint-bg) 52%, transparent 100%);
	}

	.home-scroll-hint-bar[hidden] {
		display: none;
	}

	.home-scroll-hint-btn {
		pointer-events: auto;
		background: var(--exec-scroll-hint-pill-bg);
		color: var(--exec-scroll-hint-ink);
		box-shadow:
			inset 0 0 0 1px var(--exec-border-hi),
			0 6px 18px -6px color-mix(in srgb, var(--shade) 45%, transparent);
		cursor: pointer;
		transition:
			color 0.14s ease,
			box-shadow 0.14s ease;
	}

	.home-scroll-hint-btn:hover,
	.home-scroll-hint-btn:focus-visible {
		color: var(--exec-scroll-hint-ink-hover);
		box-shadow:
			inset 0 0 0 1px var(--tone-accent-ring),
			0 6px 18px -6px color-mix(in srgb, var(--shade) 55%, transparent);
	}

	@media (width < 640px), (pointer: coarse) {
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
