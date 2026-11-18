<script lang="ts">
	let {
		id,
		label,
		value = $bindable(''),
		autocomplete,
		error,
		readonly = false,
		input = $bindable<HTMLInputElement | null>(null)
	}: {
		id: string;
		label: string;
		value?: string;
		autocomplete: 'current-password' | 'new-password';
		error?: string;
		readonly?: boolean;
		input?: HTMLInputElement | null;
	} = $props();
	let shown = $state(false);
</script>

<div>
	<label for={id} class="block text-sm/6 font-medium text-white">{label}</label>
	<div class="mt-2 flex gap-x-2">
		<input
			{id}
			bind:this={input}
			bind:value
			type={shown ? 'text' : 'password'}
			{autocomplete}
			{readonly}
			aria-invalid={error ? 'true' : undefined}
			aria-describedby={error ? `${id}-error` : undefined}
			class="block w-full rounded-md bg-white/5 px-3 py-1.5 text-base text-white outline-1 -outline-offset-1 outline-white/10 placeholder:text-gray-500 focus:outline-2 focus:-outline-offset-2 focus:outline-indigo-500 sm:text-sm/6"
		/>
		<button
			type="button"
			aria-pressed={shown}
			aria-controls={id}
			onclick={() => (shown = !shown)}
			class="rounded-md bg-white/10 px-3 py-1.5 text-sm font-semibold text-white hover:bg-white/20 focus-visible:outline-2 focus-visible:outline-indigo-500"
			>Show</button
		>
	</div>
	{#if error}
		<p id="{id}-error" class="mt-2 text-sm/6 text-red-400">{error}</p>
	{/if}
</div>
