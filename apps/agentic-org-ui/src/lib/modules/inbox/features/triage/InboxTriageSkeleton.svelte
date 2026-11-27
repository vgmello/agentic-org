<div class="triage-split flex-1" aria-busy="true" aria-label="Loading inbox">
	<div class="triage-skeleton-col flex flex-col gap-3 p-4">
		{#each [1, 2, 3, 4] as n (n)}
			<div class="triage-skel-row h-20"></div>
		{/each}
	</div>
	<div class="hidden flex-col gap-4 p-8 min-[960px]:flex">
		<div class="triage-skel-row h-10 w-64"></div>
		<div class="triage-skel-row h-24 w-full max-w-xl"></div>
		<div class="triage-skel-row h-32 w-full max-w-xl"></div>
	</div>
</div>

<style>
	.triage-split {
		display: grid;
		grid-template-columns: clamp(19rem, 34%, 31rem) minmax(0, 1fr);
		grid-template-rows: minmax(0, 1fr);
		min-height: 0;
	}
	.triage-skeleton-col {
		border-inline-end: 1px solid var(--exec-border);
	}
	.triage-skel-row {
		border-radius: var(--exec-inset-radius);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	@media (max-width: 959px) {
		.triage-split {
			display: block;
		}
		.triage-skeleton-col {
			border-inline-end: 0;
		}
	}
</style>
