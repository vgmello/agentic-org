<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { optionsOf } from '$lib/modules/messaging/engine';
	import type { AskQuestion, QuestionAnswer } from '$lib/modules/messaging/types';

	/**
	 * An ask with several questions, or one multi-select or typed question (REQ-04, CHAT-44): a radio group for a single choice,
	 * checkboxes for a multiple choice, a Yes/No pair, a text area for free text, each default preselected. **Send answers** is
	 * enabled only when every question that isn't optional has an answer; more than three questions show one step at a time.
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
	const initial = (): Record<string, Value> =>
		Object.fromEntries(
			questions.map((q) => [
				q.id,
				{ option_ids: q.default_option ? [q.default_option] : [], text: '' }
			])
		);
	let values = $state<Record<string, Value>>(initial());
	let step = $state(0);
	const stepped = $derived(questions.length > 3);
	const shown = $derived(stepped ? [questions[step]] : questions);
	const answered = (q: AskQuestion) =>
		q.question_type === 'free_text'
			? values[q.id].text.trim() !== ''
			: values[q.id].option_ids.length > 0;
	const missing = $derived(questions.filter((q) => !q.optional && !answered(q)));
	const last = $derived(!stepped || step === questions.length - 1);
	let legend = $state<HTMLElement>();

	function toggle(q: AskQuestion, id: string, on: boolean) {
		const v = values[q.id];
		v.option_ids = q.allow_multiple
			? on
				? [...v.option_ids, id]
				: v.option_ids.filter((x) => x !== id)
			: [id];
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
		void onsubmit(
			questions
				.map((q): QuestionAnswer => ({
					question_id: q.id,
					...(q.question_type === 'free_text'
						? { text: values[q.id].text.trim() }
						: { option_ids: values[q.id].option_ids })
				}))
				.filter((a) => (a.option_ids?.length ?? 0) > 0 || a.text)
		);
	}
	const choice =
		'flex items-start gap-x-3 rounded-md px-2.5 py-2 text-sm/6 outline-1 -outline-offset-1 outline-white/10 has-[:checked]:bg-indigo-500/10 has-[:checked]:outline-indigo-400/60 has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-indigo-400';
</script>

<form
	class="mt-3 space-y-4"
	onsubmit={submit}
	data-testid="question-form"
	aria-label="Answer the questions"
>
	{#if stepped}
		<p class="text-xs/5 text-gray-400" aria-live="polite" data-testid="question-step">
			Question {step + 1} of {questions.length}
		</p>
	{/if}
	{#each shown as q (q.id)}
		{@const index = questions.indexOf(q)}
		<fieldset class="min-w-0" data-question={q.id}>
			<legend
				bind:this={legend}
				tabindex="-1"
				class="text-sm/6 font-semibold text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
			>
				{#if !stepped && questions.length > 1}<span class="text-gray-400"
						>{index + 1}.
					</span>{/if}{q.prompt}{#if q.optional}<span class="font-normal text-gray-400">
						(optional)</span
					>{/if}
			</legend>
			{#if q.question_type === 'yes_no'}
				<div class="mt-2 flex flex-wrap gap-2" role="group" aria-label={q.prompt}>
					{#each optionsOf(q) as o (o.id)}
						{@const on = values[q.id].option_ids.includes(o.id)}
						<Button
							type="button"
							size="sm"
							variant="outline"
							class="h-auto min-h-8 px-2.5 py-1 max-[640px]:min-h-11 {on
								? 'bg-indigo-500/15 text-white outline-indigo-400/60'
								: ''}"
							aria-pressed={on}
							{disabled}
							onclick={() => toggle(q, o.id, true)}
						>
							{o.label}{#if q.default_option === o.id}<span
									class="text-xs/5 font-normal text-indigo-200">Default</span
								>{/if}
						</Button>
					{/each}
				</div>
			{:else if q.question_type === 'choice'}
				<div class="mt-2 grid gap-2 min-[640px]:grid-cols-2">
					{#each optionsOf(q) as o (o.id)}
						<label class={choice}>
							<input
								type={q.allow_multiple ? 'checkbox' : 'radio'}
								name="{askId}-{q.id}"
								value={o.id}
								class="mt-1 size-4 shrink-0 accent-indigo-500"
								checked={values[q.id].option_ids.includes(o.id)}
								{disabled}
								onchange={(event) => toggle(q, o.id, event.currentTarget.checked)}
							/>
							<span class="min-w-0">
								<span class="block font-medium text-white"
									>{o.label}{#if q.default_option === o.id}<span
											class="ml-2 text-xs/5 font-normal text-indigo-200">Default</span
										>{/if}</span
								>
								{#if o.description}<span class="block text-xs/5 text-gray-400">{o.description}</span
									>{/if}
							</span>
						</label>
					{/each}
				</div>
				{#if q.allow_multiple}<p class="mt-1 text-xs/5 text-gray-400">
						Choose any that apply.
					</p>{/if}
			{:else}
				<label class="sr-only" for="{askId}-{q.id}-text">{q.prompt}</label>
				<textarea
					id="{askId}-{q.id}-text"
					bind:value={values[q.id].text}
					rows="2"
					maxlength={20000}
					{disabled}
					class="mt-2 block w-full rounded-md bg-gray-800 px-3 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 placeholder:text-gray-500 focus-visible:outline-2 focus-visible:outline-indigo-400 max-[639px]:text-base"
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
			{#if missing.length}<p class="text-xs/5 text-gray-400" aria-live="polite">
					{missing.length === 1
						? '1 question needs an answer'
						: `${missing.length} questions need an answer`}
				</p>{/if}
		{:else}
			<Button type="submit" size="sm" variant="outline">Next</Button>
		{/if}
	</div>
</form>
