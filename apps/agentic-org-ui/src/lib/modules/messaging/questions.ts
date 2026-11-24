import {
	ASK_QUESTIONS_MAX,
	CHAT_LIMITS,
	type AskOption,
	type AskQuestion,
	type ChatMessage,
	type ChatRefusal,
	type QuestionAnswer
} from './types';

/** Product notices can't be answered (chat-ux §4.2). */
export const SYSTEM_NO_REPLY = "System messages can't be replied to.";

const YES_NO: AskOption[] = [
	{ id: 'yes', label: 'Yes' },
	{ id: 'no', label: 'No' }
];

const refuse = (code: ChatRefusal['code'], message: string): never => {
	throw { source: 'api', code, message } satisfies ChatRefusal;
};

/** An ask's questions (REQ-04): a stored list, or the one choice question older asks carry in `body` and `options`. */
export function questionsOf(
	m: Pick<ChatMessage, 'id' | 'body' | 'options' | 'questions' | 'default_option'>
): AskQuestion[] {
	if (m.questions?.length) return m.questions;
	if (m.options?.length) {
		return [
			{
				id: 'q1',
				question_type: 'choice',
				prompt: m.body,
				options: m.options,
				...(m.default_option ? { default_option: m.default_option } : {})
			}
		];
	}
	return [{ id: 'q1', question_type: 'free_text', prompt: m.body }];
}

/** The options a question offers: its own, or Yes and No. */
export const optionsOf = (q: AskQuestion): AskOption[] =>
	q.question_type === 'yes_no' ? YES_NO : q.question_type === 'choice' ? (q.options ?? []) : [];

/** One answer in words: "Staging", "Staging, Production", or the text. */
export function answerText(q: AskQuestion, a: QuestionAnswer | undefined): string {
	if (!a) return 'No answer';
	const labels = (a.option_ids ?? []).map(
		(id) =>
			optionsOf(q)
				.find((o) => o.id === id)
				?.label.replace(/…$/, '') ?? id
	);
	return [labels.join(', '), a.text].filter(Boolean).join(': ');
}

function validateChoiceOptions(q: AskQuestion): AskOption[] {
	const opts = q.options ?? [];
	if (!opts.length || opts.length > CHAT_LIMITS.askOptionsMax) {
		return refuse(
			'invalid_request',
			`Question ${q.id}: a choice has one to ${CHAT_LIMITS.askOptionsMax} options.`
		);
	}
	if (
		new Set(opts.map((o) => o.id)).size !== opts.length ||
		opts.some((o) => !o.id || !o.label.trim())
	) {
		return refuse('invalid_request', `Question ${q.id}: each option needs its own id and a label.`);
	}
	return opts.map((o) => ({
		id: o.id,
		label: o.label.trim(),
		...(o.description?.trim() ? { description: o.description.trim() } : {})
	}));
}

function validateOneQuestion(q: AskQuestion): AskQuestion {
	if (!q.prompt?.trim()) return refuse('invalid_request', `Question ${q.id} needs a prompt.`);
	if (!['choice', 'yes_no', 'free_text'].includes(q.question_type)) {
		return refuse('invalid_request', `Question ${q.id}: the type is choice, yes_no or free_text.`);
	}
	const out: AskQuestion = {
		id: q.id,
		question_type: q.question_type,
		prompt: q.prompt.trim(),
		...(q.optional ? { optional: true } : {})
	};
	if (q.question_type === 'choice') {
		out.options = validateChoiceOptions(q);
		if (q.allow_multiple) out.allow_multiple = true;
	} else if (q.options?.length) {
		return refuse(
			'invalid_request',
			`Question ${q.id}: ${q.question_type === 'yes_no' ? 'yes or no' : 'free text'} takes no options.`
		);
	}
	if (q.default_option !== undefined) {
		if (q.question_type === 'free_text' || !optionsOf(out).some((o) => o.id === q.default_option)) {
			return refuse(
				'invalid_request',
				`Question ${q.id}: the default must be ${q.question_type === 'yes_no' ? 'yes or no' : 'one of its options'}.`
			);
		}
		out.default_option = q.default_option;
	}
	return out;
}

/** REQ-04 validation, with a reason the asker can act on. */
export function validQuestions(questions: AskQuestion[]): AskQuestion[] {
	if (questions.length > ASK_QUESTIONS_MAX) {
		return refuse('invalid_request', `An ask has one to ${ASK_QUESTIONS_MAX} questions.`);
	}
	const ids = new Set(questions.map((q) => q.id));
	if (ids.size !== questions.length || questions.some((q) => !q.id)) {
		return refuse('invalid_request', 'Each question needs its own id.');
	}
	return questions.map(validateOneQuestion);
}
