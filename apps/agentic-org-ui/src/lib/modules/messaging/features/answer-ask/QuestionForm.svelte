<script lang="ts">
	import { untrack } from 'svelte';
	import { RadioGroup } from 'bits-ui';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Label } from '$lib/components/ui/label';
	import { cn } from '$lib/utils.js';
	import { optionsOf } from '$lib/modules/messaging/engine';
	import type { AskQuestion, QuestionAnswer } from '$lib/modules/messaging/types';

	/**
	 * An ask with several questions, or one multi-select or typed question (REQ-04, CHAT-44): a Bits UI radio group for a
	 * single choice, shadcn-svelte checkboxes for a multiple choice, a Yes/No button pair, and an accessible text area for
	 * free text, each default preselected (`default_option` or `default_value`). **Send answers** is enabled only when every
	 * question that isn't optional has an answer; more than three questions show one step at a time.
	 */
	let {
		askId,
		questions,
		disabled = false,
		onsubmit
	}: {
		askId: string;
		questions: AskQuestion[];
		disabled?: boolean;
		onsubmit: (answers: QuestionAnswer[]) => Promise<void> | void;
	} = $props();

	type Value = { option_ids: string[]; text: string };

	const defaultOf = (q: AskQuestion): string | undefined => q.default_option ?? q.default_value;

	function buildInitial(list: AskQuestion[]): Record<string, Value> {
		return Object.fromEntries(
			list.map((q) => {
				const def = defaultOf(q);
				if (q.question_type === 'free_text') {
					return [q.id, { option_ids: [], text: def ?? '' }];
				}
				return [q.id, { option_ids: def ? [def] : [], text: '' }];
			})
		);
	}

	let values = $derived.by<Record<string, Value>>(() => {
		void askId;
		return untrack(() => buildInitial(questions));
	});
	let step = $derived.by(() => {
		void askId;
		return 0;
	});
	let legend: HTMLElement | undefined;

	function trackLegend(node: HTMLElement) {
		legend = node;
		return () => {
			if (legend === node) legend = undefined;
		};
	}

	const stepped = $derived(questions.length > 3);
	const shown = $derived(stepped ? [questions[step]].filter(Boolean) : questions);
	const valueOf = (id: string): Value => values[id] ?? { option_ids: [], text: '' };
	const answered = (q: AskQuestion) =>
		q.question_type === 'free_text'
			? valueOf(q.id).text.trim() !== ''
			: valueOf(q.id).option_ids.length > 0;
	const missing = $derived(questions.filter((q) => !q.optional && !answered(q)));
	const last = $derived(!stepped || step === questions.length - 1);

	function toggle(q: AskQuestion, id: string, on: boolean) {
		const current = valueOf(q.id);
		const nextIds = q.allow_multiple
			? on
				? [...current.option_ids.filter((x) => x !== id), id]
				: current.option_ids.filter((x) => x !== id)
			: [id];
		values = { ...values, [q.id]: { ...current, option_ids: nextIds } };
	}

	function setText(id: string, text: string) {
		const current = valueOf(id);
		values = { ...values, [id]: { ...current, text } };
	}

	function go(to: number) {
		step = to;
		// Keep the keyboard on the new question.
		queueMicrotask(() => legend?.focus());
	}

	function submit(event: SubmitEvent) {
		event.preventDefault();
		if (!last) return go(step + 1);
		if (missing.length || disabled) return;
		const payload: QuestionAnswer[] = questions
			.map((q): QuestionAnswer => {
				const current = valueOf(q.id);
				return {
					question_id: q.id,
					...(q.question_type === 'free_text'
						? { text: current.text.trim() }
						: { option_ids: current.option_ids })
				};
			})
			.filter((a) => (a.option_ids?.length ?? 0) > 0 || a.text);
		void onsubmit(payload);
	}

	const choiceCard = (selected: boolean) =>
		cn(
			'flex items-start gap-x-3 rounded-md px-2.5 py-2 text-sm/6 outline-1 -outline-offset-1 outline-line has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-focus',
			selected && 'bg-primary/10 outline-brand/60'
		);
</script>

<form
	class="mt-3 flex flex-col gap-4"
	onsubmit={submit}
	data-testid="question-form"
	aria-label="Answer the questions"
>
	{#if stepped}
		<p class="text-xs/5 text-fg-muted" aria-live="polite" data-testid="question-step">
			Question {step + 1} of {questions.length}
		</p>
	{/if}
	{#each shown as q (q.id)}
		{@const index = questions.indexOf(q)}
		{@const current = valueOf(q.id)}
		{@const def = defaultOf(q)}
		<fieldset class="min-w-0" data-question={q.id}>
			<legend
				{@attach trackLegend}
				tabindex="-1"
				class="text-sm/6 font-semibold text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
			>
				{#if !stepped && questions.length > 1}<span class="text-fg-muted"
						>{index + 1}.
					</span>{/if}{q.prompt}{#if q.optional}<span class="font-normal text-fg-muted">
						(optional)</span
					>{/if}
			</legend>
			{#if q.question_type === 'yes_no'}
				<div class="mt-2 flex flex-wrap gap-2" role="group" aria-label={q.prompt}>
					{#each optionsOf(q) as o (o.id)}
						{@const on = current.option_ids.includes(o.id)}
						<Button
							type="button"
							size="sm"
							variant="outline"
							class={cn(
								'h-auto min-h-8 px-2.5 py-1 max-[640px]:min-h-11',
								on && 'bg-primary/15 text-fg outline-brand/60'
							)}
							aria-pressed={on}
							{disabled}
							onclick={() => toggle(q, o.id, true)}
						>
							{o.label}{#if def === o.id}<span class="text-xs/5 font-normal text-brand-text-strong"
									>Default</span
								>{/if}
						</Button>
					{/each}
				</div>
			{:else if q.question_type === 'choice' && q.allow_multiple}
				<div class="mt-2 grid gap-2 min-[640px]:grid-cols-2">
					{#each optionsOf(q) as o (o.id)}
						{@const optId = `${askId}-${q.id}-${o.id}`}
						{@const checked = current.option_ids.includes(o.id)}
						<div class={choiceCard(checked)}>
							<Checkbox
								id={optId}
								{checked}
								{disabled}
								class="mt-1"
								onCheckedChange={(on) => toggle(q, o.id, on === true)}
							/>
							<Label for={optId} class="block min-w-0 flex-1 cursor-pointer leading-6 font-normal">
								<span class="block font-medium text-fg"
									>{o.label}{#if def === o.id}<span
											class="ml-2 text-xs/5 font-normal text-brand-text-strong">Default</span
										>{/if}</span
								>
								{#if o.description}<span class="block text-xs/5 text-fg-muted">{o.description}</span
									>{/if}
							</Label>
						</div>
					{/each}
				</div>
				<p class="mt-1 text-xs/5 text-fg-muted">Choose any that apply.</p>
			{:else if q.question_type === 'choice'}
				<RadioGroup.Root
					value={current.option_ids[0] ?? ''}
					onValueChange={(value) => {
						if (value) toggle(q, value, true);
					}}
					name="{askId}-{q.id}"
					{disabled}
					aria-label={q.prompt}
					class="mt-2 grid gap-2 min-[640px]:grid-cols-2"
				>
					{#each optionsOf(q) as o (o.id)}
						{@const optId = `${askId}-${q.id}-${o.id}`}
						{@const checked = current.option_ids.includes(o.id)}
						<div class={choiceCard(checked)}>
							<RadioGroup.Item
								id={optId}
								value={o.id}
								{disabled}
								class="mt-1 flex size-4 shrink-0 items-center justify-center rounded-full border border-input bg-surface shadow-xs outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:border-primary data-[state=checked]:text-primary"
							>
								{#snippet children({ checked: itemChecked })}
									{#if itemChecked}
										<span class="size-2 rounded-full bg-primary"></span>
									{/if}
								{/snippet}
							</RadioGroup.Item>
							<Label for={optId} class="block min-w-0 flex-1 cursor-pointer leading-6 font-normal">
								<span class="block font-medium text-fg"
									>{o.label}{#if def === o.id}<span
											class="ml-2 text-xs/5 font-normal text-brand-text-strong">Default</span
										>{/if}</span
								>
								{#if o.description}<span class="block text-xs/5 text-fg-muted">{o.description}</span
									>{/if}
							</Label>
						</div>
					{/each}
				</RadioGroup.Root>
			{:else}
				<Label class="sr-only" for="{askId}-{q.id}-text">{q.prompt}</Label>
				<textarea
					id="{askId}-{q.id}-text"
					value={current.text}
					oninput={(event) => setText(q.id, event.currentTarget.value)}
					rows="2"
					maxlength={20000}
					{disabled}
					class="mt-2 block w-full rounded-md bg-surface px-3 py-2 text-sm/6 text-fg outline-1 -outline-offset-1 outline-line placeholder:text-fg-subtle focus-visible:outline-2 focus-visible:outline-focus max-[639px]:text-base"
				></textarea>
			{/if}
		</fieldset>
	{/each}
	<div class="flex flex-wrap items-center gap-2">
		{#if stepped && step > 0}<Button
				type="button"
				size="sm"
				variant="outline"
				onclick={() => go(step - 1)}>Back</Button
			>{/if}
		{#if last}
			<Button
				type="submit"
				size="sm"
				disabled={disabled || missing.length > 0}
				data-testid="send-answers">Send answers</Button
			>
			{#if missing.length}<p class="text-xs/5 text-fg-muted" aria-live="polite">
					{missing.length === 1
						? '1 question needs an answer'
						: `${missing.length} questions need an answer`}
				</p>{/if}
		{:else}
			<Button type="submit" size="sm" variant="outline">Next</Button>
		{/if}
	</div>
</form>
