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
	<label for={id} class="block text-sm/6 font-medium text-fg">{label}</label>
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
			class="block w-full rounded-md bg-surface-raised px-3 py-1.5 text-base text-fg outline-1 -outline-offset-1 outline-line placeholder:text-fg-subtle focus:outline-2 focus:-outline-offset-2 focus:outline-ring sm:text-sm/6"
		/>
		<button
			type="button"
			aria-pressed={shown}
			aria-controls={id}
			onclick={() => (shown = !shown)}
			class="rounded-md bg-surface-overlay px-3 py-1.5 text-sm font-semibold text-fg hover:bg-tint/20 focus-visible:outline-2 focus-visible:outline-ring"
			>Show</button
		>
	</div>
	{#if error}
		<p id="{id}-error" class="mt-2 text-sm/6 text-danger">{error}</p>
	{/if}
</div>
