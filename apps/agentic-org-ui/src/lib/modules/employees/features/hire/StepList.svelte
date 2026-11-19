<!--
  @component
  Where the hire stands (our-org-in-the-app §7: Describe → Suggestion → Review → Hiring): an ordered list with the
  current step marked `aria-current="step"`; once hired every step is done. Below 640 px only "Step 2 of 4 · Suggestion" (or "Done") shows.
-->
<script lang="ts">
	const STEPS = ['Describe', 'Suggestion', 'Review', 'Hiring'] as const;

	/** The current step; 4 once the hire is done, when every step is done and none is current. */
	let { current }: { current: 0 | 1 | 2 | 3 | 4 } = $props();
</script>

<nav
	aria-label="Hire steps"
	class="border-b border-line-subtle px-4 py-3 min-[640px]:px-6 min-[1024px]:px-8"
>
	<p class="text-xs/5 text-fg-muted min-[640px]:hidden">
		{#if current === 4}
			<span class="font-semibold text-fg">Done</span> · every step finished
		{:else}
			Step {current + 1} of {STEPS.length} ·
			<span class="font-semibold text-fg">{STEPS[current]}</span>
		{/if}
	</p>
	<ol role="list" class="hidden flex-wrap items-center gap-x-2 gap-y-1 min-[640px]:flex">
		{#each STEPS as label, i (label)}
			<li class="flex items-center gap-x-2 text-sm/6">
				{#if i > 0}<span aria-hidden="true" class="h-px w-6 bg-surface-overlay"></span>{/if}
				<span
					aria-current={i === current ? 'step' : undefined}
					class={[
						'inline-flex items-center gap-x-2',
						i === current
							? 'font-semibold text-fg'
							: i < current
								? 'text-fg-secondary'
								: 'text-fg-muted'
					]}
				>
					<span
						aria-hidden="true"
						class={[
							'flex size-6 shrink-0 items-center justify-center rounded-full text-xs/5 tabular-nums',
							i === current
								? 'bg-primary text-on-emphasis'
								: i < current
									? 'bg-surface-overlay text-fg-secondary'
									: 'outline-1 -outline-offset-1 outline-line-strong'
						]}>{i + 1}</span
					>
					{label}{#if i < current}<span class="sr-only">, done</span>{/if}
				</span>
			</li>
		{/each}
	</ol>
</nav>
