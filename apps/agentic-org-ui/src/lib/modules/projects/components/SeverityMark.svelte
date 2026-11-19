<!--
  @component
  A bug-severity count as icon and word (work-items-ux §2.1): Critical in danger text with exclamation-triangle; Major
  exclamation-circle, Minor minus-circle, Trivial information-circle and Not set question-mark-circle, all neutral.
  The word always carries the meaning; the icon is decorative. Heroicons v2.2.0 (MIT, Tailwind Labs) solid paths at
  16 px; the shared icon set (`$lib/components/heroicons.ts`) has only the question mark, so the other four are kept
  here until it gains them.
-->
<script lang="ts" module>
	import type { BugCounts } from '../types';

	const PATHS: Record<Exclude<keyof BugCounts, 'unset'>, string> = {
		critical:
			'M8.485 2.495c.673-1.167 2.357-1.167 3.03 0l6.28 10.875c.673 1.167-.17 2.625-1.516 2.625H3.72c-1.347 0-2.189-1.458-1.515-2.625L8.485 2.495ZM10 5a.75.75 0 0 1 .75.75v3.5a.75.75 0 0 1-1.5 0v-3.5A.75.75 0 0 1 10 5Zm0 9a1 1 0 1 0 0-2 1 1 0 0 0 0 2Z',
		major:
			'M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0Zm-8-5a.75.75 0 0 1 .75.75v4.5a.75.75 0 0 1-1.5 0v-4.5A.75.75 0 0 1 10 5Zm0 10a1 1 0 1 0 0-2 1 1 0 0 0 0 2Z',
		minor:
			'M10 18a8 8 0 1 0 0-16 8 8 0 0 0 0 16ZM6.75 9.25a.75.75 0 0 0 0 1.5h6.5a.75.75 0 0 0 0-1.5h-6.5Z',
		trivial:
			'M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0Zm-7-4a1 1 0 1 1-2 0 1 1 0 0 1 2 0ZM9 9a.75.75 0 0 0 0 1.5h.253a.25.25 0 0 1 .244.304l-.459 2.066A1.75 1.75 0 0 0 10.747 15H11a.75.75 0 0 0 0-1.5h-.253a.25.25 0 0 1-.244-.304l.459-2.066A1.75 1.75 0 0 0 9.253 9H9Z'
	};
</script>

<script lang="ts">
	import HeroIcon from '$lib/components/HeroIcon.svelte';

	let { severity, text }: { severity: keyof BugCounts; text: string } = $props();
</script>

<span
	class="inline-flex items-center gap-x-1 whitespace-nowrap {severity === 'critical'
		? 'text-danger-text'
		: ''}"
	data-severity={severity}
>
	{#if severity === 'unset'}
		<HeroIcon variant="micro" name="question-mark-circle" class="size-4 text-fg-muted" />
	{:else}
		<svg
			viewBox="0 0 20 20"
			fill="currentColor"
			aria-hidden="true"
			focusable="false"
			class="size-4 shrink-0 {severity === 'critical' ? '' : 'text-fg-muted'}"
		>
			<path fill-rule="evenodd" clip-rule="evenodd" d={PATHS[severity]} />
		</svg>
	{/if}
	{text}
</span>
