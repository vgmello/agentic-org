<script lang="ts">
	let { class: klass = 'h-8 w-auto' }: { class?: string } = $props();
</script>

<!-- Agency mark: three agents linked into one org. -->
<svg viewBox="0 0 32 32" class={klass} aria-hidden="true">
	<rect width="32" height="32" rx="8" class="fill-indigo-500" />
	<path d="M16 9.5 9.5 22M16 9.5 22.5 22M9.5 22h13" stroke="white" stroke-opacity=".55" stroke-width="1.75" fill="none" stroke-linecap="round" />
	<circle cx="16" cy="9.5" r="3.25" fill="white" />
	<circle cx="9.5" cy="22" r="2.75" fill="white" />
	<circle cx="22.5" cy="22" r="2.75" fill="white" />
</svg>
