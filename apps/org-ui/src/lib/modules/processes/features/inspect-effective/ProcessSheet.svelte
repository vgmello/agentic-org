<script lang="ts">
	import CircleCheck from '@lucide/svelte/icons/circle-check';
	import CircleMinus from '@lucide/svelte/icons/circle-minus';
	import CircleX from '@lucide/svelte/icons/circle-x';
	import PowerOff from '@lucide/svelte/icons/power-off';
	import Lock from '@lucide/svelte/icons/lock';
	import X from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import ProcessBadges from '$lib/modules/processes/components/ProcessBadges.svelte';
	import { Sheet, SheetClose, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '$lib/components/ui/sheet';
	import type { Entry, Layer, ProcessDoc, TurnedOff } from '$lib/modules/processes/types';

	/** One process: its text, its facts and "why": the layers that had its key and what happened to each (PROC-8). */
	let { open = $bindable(false), entry, off }: { open: boolean; entry?: Entry; off?: TurnedOff } = $props();
	const doc = $derived<ProcessDoc | undefined>(entry?.doc ?? off?.doc);
	const chain = $derived<Layer[]>(entry?.chain ?? off?.chain ?? []);
	const OUTCOME: Record<Layer['outcome'], string> = {
		applies: 'Applies',
		overridden: 'Overridden',
		refused: 'Refused',
		left_out: 'Left out',
		turned_off: 'Turned off',
		not_applied: 'Not applied',
		conflict: 'In conflict'
	};
</script>

<Sheet bind:open>
	<SheetContent side="right" showCloseButton={false} class="w-full max-w-[440px] overflow-y-auto data-[side=right]:w-full data-[side=right]:sm:max-w-[440px]">
		{#if doc}
			<SheetHeader>
				<SheetTitle class="pr-10 [overflow-wrap:anywhere]">{doc.name}</SheetTitle>
				<SheetDescription class="flex flex-wrap items-center gap-2">
					<ProcessBadges level={doc.location.level} scopeName={entry?.scopeName ?? off?.scopeName} version={doc.version} />
					{#if entry?.sealed}<span class="inline-flex items-center gap-x-1 text-xs/5 text-amber-200"><Lock class="size-3.5" aria-hidden="true" /> Sealed</span>{/if}
					{#if off}<span class="inline-flex min-w-0 max-w-full items-start gap-x-1 text-xs/5 text-gray-300"><PowerOff class="mt-0.5 size-3.5 shrink-0" aria-hidden="true" /><span class="min-w-0 [overflow-wrap:anywhere]">Turned off{off.scopeName ? ` for ${off.scopeName}` : ''}: {off.reason}</span></span>{/if}
				</SheetDescription>
			</SheetHeader>
			<div class="space-y-5 px-4 pb-6 text-sm/6">
				{#if doc.summary}<p class="text-gray-200 [overflow-wrap:anywhere]">{doc.summary}</p>{/if}
				<dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 text-gray-300">
					<dt class="text-gray-400">Key</dt><dd class="font-mono [overflow-wrap:anywhere]">{doc.key}</dd>
					<dt class="text-gray-400">Owner</dt><dd class="[overflow-wrap:anywhere]">{doc.owner}</dd>
					{#if doc.appliesTo}<dt class="text-gray-400">Applies to</dt><dd class="[overflow-wrap:anywhere]">{doc.appliesTo}</dd>{/if}
					<dt class="text-gray-400">Can be overridden</dt><dd>{doc.overridable ? 'Yes' : 'No: sealed'}</dd>
					{#if doc.overrides}<dt class="text-gray-400">Overrides</dt><dd class="font-mono">{doc.overrides.key}@{doc.overrides.version}</dd>{/if}
				</dl>

				{#if off}
					<p class="rounded-md bg-white/5 p-2 text-gray-200 outline-1 -outline-offset-1 outline-white/10 [overflow-wrap:anywhere]" data-testid="off-why">Turned off{off.scopeName ? ` for ${off.scopeName}` : ''}: {off.reason}. The process it turns off stays as written at the level above.</p>
				{/if}
				{#if chain.length > 0}
					<section aria-labelledby="why-title" data-testid="why">
						<h3 id="why-title" class="font-semibold text-white">Why it reads this way</h3>
						<ol class="mt-2 space-y-2">
							{#each chain as layer, i (i)}
								<li class="flex gap-x-2 rounded-md bg-white/5 p-2 outline-1 -outline-offset-1 outline-white/10" data-testid="why-layer" data-outcome={layer.outcome}>
									{#if layer.outcome === 'applies'}<CircleCheck class="mt-0.5 size-4 shrink-0 text-emerald-300" aria-hidden="true" />
									{:else if layer.outcome === 'refused' || layer.outcome === 'left_out'}<CircleX class="mt-0.5 size-4 shrink-0 text-red-300" aria-hidden="true" />
									{:else}<CircleMinus class="mt-0.5 size-4 shrink-0 text-gray-300" aria-hidden="true" />{/if}
									<div class="min-w-0">
										<p class="flex flex-wrap items-center gap-2 text-white">
											<ProcessBadges level={layer.level} scopeName={layer.scopeName} version={layer.doc.version} />
											<span class="font-medium">{OUTCOME[layer.outcome]}</span>
										</p>
										{#if layer.note}<p class="text-gray-300 [overflow-wrap:anywhere]">{layer.note}</p>{/if}
									</div>
								</li>
							{/each}
						</ol>
					</section>
				{/if}

				{#if doc.moments && Object.values(doc.moments).some(Boolean)}
					<section aria-labelledby="moments-title">
						<h3 id="moments-title" class="font-semibold text-white">Prompts at guidance moments</h3>
						<dl class="mt-1 space-y-1 text-gray-300">
							{#if doc.moments.starting_task}<div><dt class="inline text-gray-400">Starting a task:</dt>{' '}<dd class="inline [overflow-wrap:anywhere]">{doc.moments.starting_task}</dd></div>{/if}
							{#if doc.moments.waking_up}<div><dt class="inline text-gray-400">Waking up:</dt>{' '}<dd class="inline [overflow-wrap:anywhere]">{doc.moments.waking_up}</dd></div>{/if}
							{#if doc.moments.finishing_task}<div><dt class="inline text-gray-400">Finishing a task:</dt>{' '}<dd class="inline [overflow-wrap:anywhere]">{doc.moments.finishing_task}</dd></div>{/if}
						</dl>
					</section>
				{/if}
				{#if doc.checklist && doc.checklist.length > 0}
					<section aria-labelledby="checklist-title">
						<h3 id="checklist-title" class="font-semibold text-white">Checklist steps it adds to a task</h3>
						<ul class="mt-1 list-disc pl-5 text-gray-300">{#each doc.checklist as step, i (i)}<li class="[overflow-wrap:anywhere]">{step.text}{step.required ? ' (required)' : ''}</li>{/each}</ul>
					</section>
				{/if}
				<section aria-labelledby="text-title">
					<h3 id="text-title" class="font-semibold text-white">The process</h3>
					<!-- The file's text, shown as text: never as HTML. -->
					<pre class="mt-1 max-h-96 overflow-auto rounded-md bg-gray-900/60 p-3 whitespace-pre-wrap text-gray-200 [overflow-wrap:anywhere]">{doc.body || '(no text)'}</pre>
				</section>
			</div>
		{/if}
		<SheetClose>
			{#snippet child({ props })}
				<Button variant="ghost" size="icon-sm" class="absolute top-4 right-4 focus-visible:ring-0 focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring" {...props}>
					<X aria-hidden="true" />
					<span class="sr-only">Close</span>
				</Button>
			{/snippet}
		</SheetClose>
	</SheetContent>
</Sheet>
