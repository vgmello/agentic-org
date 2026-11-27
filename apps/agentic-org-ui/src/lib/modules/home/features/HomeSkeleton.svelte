<div
	class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-4"
	aria-busy="true"
	aria-label="Loading executive overview"
>
	{#each [1, 2, 3, 4] as n (n)}
		<div class="exec-card flex flex-col gap-3 p-5">
			<div class="bg-bar-track h-3 w-24 rounded"></div>
			<div class="bg-bar-track h-7 w-36 rounded"></div>
			<div class="bg-bar-track h-1.5 w-full rounded-full"></div>
			<div class="bg-bar-track h-3 w-44 rounded"></div>
		</div>
	{/each}
</div>
