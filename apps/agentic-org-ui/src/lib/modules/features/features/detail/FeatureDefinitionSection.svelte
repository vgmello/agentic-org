<!--
  @component
  Capability definition reader and inline editor (with conflict diff) for FeaturePage.
-->
<script lang="ts">
	import CopyButton from '$lib/components/CopyButton.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { inputClass, mobileButton, selectClass, textareaClass } from '$lib/components/classes';
	import { DocDiffView } from '$lib/modules/docs/ui';
	import type { FeatureDetail, ProjectFeaturesOverview } from '../../types';

	let {
		uid,
		feature,
		owners,
		editing,
		editBusy,
		editError,
		conflictFeature,
		editTitle = $bindable(''),
		editSlug = $bindable(''),
		editOwnerId = $bindable(''),
		editBody = $bindable(''),
		onSave,
		onLoadLatest,
		onCancel
	}: {
		uid: string;
		feature: FeatureDetail;
		owners: ProjectFeaturesOverview['owners'];
		editing: boolean;
		editBusy: boolean;
		editError: string | undefined;
		conflictFeature: FeatureDetail | null;
		editTitle: string;
		editSlug: string;
		editOwnerId: string;
		editBody: string;
		onSave: (expectedRevision?: number) => void;
		onLoadLatest: (latest: FeatureDetail) => void;
		onCancel: () => void;
	} = $props();

	type BodyBlock =
		| { kind: 'heading'; text: string }
		| { kind: 'list'; items: string[] }
		| { kind: 'p'; text: string };

	function parseBodyBlocks(markdown: string): BodyBlock[] {
		const out: BodyBlock[] = [];
		let para: string[] = [];
		const flush = () => {
			if (para.length) out.push({ kind: 'p', text: para.join(' ') });
			para = [];
		};
		for (const raw of markdown.split(/\r?\n/)) {
			const line = raw.trim();
			const h = /^#{1,6}\s+(.*)$/.exec(line);
			const li = /^[-*+]\s+(.*)$/.exec(line);
			if (!line) flush();
			else if (h) {
				flush();
				out.push({ kind: 'heading', text: h[1] });
			} else if (li) {
				flush();
				const last = out.at(-1);
				if (last?.kind === 'list') last.items.push(li[1]);
				else out.push({ kind: 'list', items: [li[1]] });
			} else {
				para.push(line);
			}
		}
		flush();
		return out;
	}
</script>

<section aria-labelledby="{uid}-def-h" data-testid="feature-definition">
	<h4 id="{uid}-def-h" class="text-sm/6 font-semibold text-fg">Capability definition</h4>

	{#if editing}
		<div class="mt-3 flex flex-col gap-4 rounded-xl bg-surface-raised p-4 ring-1 ring-line">
			{#if conflictFeature}
				<div
					class="flex flex-col gap-2 rounded-lg bg-warning/10 p-3 text-xs/5 text-warning-text-strong ring-1 ring-warning/30"
					role="alert"
					data-testid="feature-conflict"
				>
					<p class="font-semibold">
						Changed to rev {conflictFeature.revision} while you were editing. Your draft is kept below.
					</p>
					<DocDiffView
						before={conflictFeature.bodyMarkdown}
						after={editBody}
						label="Latest saved (rev {conflictFeature.revision}) vs your draft"
					/>
					<div class="flex flex-wrap gap-2">
						<Button
							type="button"
							size="sm"
							class={mobileButton}
							disabled={editBusy}
							onclick={() => onSave(conflictFeature.revision)}
						>
							Save draft at rev {conflictFeature.revision}
						</Button>
						<Button
							type="button"
							variant="outline"
							size="sm"
							class={mobileButton}
							onclick={() => onLoadLatest(conflictFeature)}
						>
							Load latest
						</Button>
					</div>
				</div>
			{:else if editError}
				<div
					class="flex flex-col gap-2 rounded-lg bg-danger-emphasis/10 p-3 text-xs/5 text-danger-text ring-1 ring-danger-emphasis/20"
					role="alert"
				>
					<p>{editError}</p>
					<div class="flex flex-wrap gap-2">
						<Button
							type="button"
							size="sm"
							class={mobileButton}
							disabled={editBusy}
							onclick={() => onSave()}
						>
							Retry
						</Button>
						<CopyButton text={editBody} label="Copy definition" size="sm" />
					</div>
				</div>
			{/if}

			<div class="grid grid-cols-1 gap-3 min-[640px]:grid-cols-2">
				<div class="min-[640px]:col-span-2">
					<Label for="{uid}-ed-title" class="mb-1 block text-xs/5 font-medium text-fg">Title</Label>
					<Input id="{uid}-ed-title" bind:value={editTitle} class={inputClass} />
				</div>
				<div>
					<Label for="{uid}-ed-slug" class="mb-1 block text-xs/5 font-medium text-fg">
						Local slug
					</Label>
					<Input id="{uid}-ed-slug" bind:value={editSlug} class="font-mono {inputClass}" />
				</div>
				<div>
					<Label for="{uid}-ed-owner" class="mb-1 block text-xs/5 font-medium text-fg">
						Accountable owner
					</Label>
					<select id="{uid}-ed-owner" bind:value={editOwnerId} class={selectClass}>
						<option value="">Unassigned</option>
						{#each owners as emp (emp.id)}
							<option value={emp.id}>{emp.name} ({emp.kind})</option>
						{/each}
					</select>
				</div>
			</div>

			<div>
				<Label for="{uid}-ed-body" class="mb-1 block text-xs/5 font-medium text-fg">
					Definition (Markdown)
				</Label>
				<textarea
					id="{uid}-ed-body"
					rows={12}
					bind:value={editBody}
					class="font-mono {textareaClass}"></textarea>
			</div>

			<div class="flex flex-wrap gap-2">
				<Button
					type="button"
					size="sm"
					class={mobileButton}
					disabled={editBusy}
					onclick={() => onSave()}
				>
					{editBusy ? 'Saving…' : 'Save definition'}
				</Button>
				<Button
					type="button"
					variant="outline"
					size="sm"
					class={mobileButton}
					disabled={editBusy}
					onclick={onCancel}
				>
					Cancel
				</Button>
			</div>
		</div>
	{:else}
		<div
			class="mt-3 flex max-w-prose flex-col gap-3 text-sm/6 text-fg-body [overflow-wrap:anywhere]"
		>
			{#each parseBodyBlocks(feature.bodyMarkdown) as blk, i (`${i}-${blk.kind}`)}
				{#if blk.kind === 'heading'}
					<h5 class="pt-2 text-sm/6 font-semibold text-fg first:pt-0">{blk.text}</h5>
				{:else if blk.kind === 'list'}
					<ul role="list" class="flex list-disc flex-col gap-1 pl-5 marker:text-fg-muted">
						{#each blk.items as li, j (`${i}-${j}`)}
							<li>{li}</li>
						{/each}
					</ul>
				{:else}
					<p>{blk.text}</p>
				{/if}
			{/each}
		</div>
	{/if}
</section>
