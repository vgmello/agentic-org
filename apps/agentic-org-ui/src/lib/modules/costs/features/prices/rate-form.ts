// Add a rate's fields, checked before sending (costs-and-insights-ux §5). The server is the authority; this only
// catches what a person can fix before waiting on it. Rates are dollars per million tokens, converted exactly.
import { isDayKey } from '../../dates';
import { parseRate } from '../../money';
import type { PriceDraft, PriceField } from '../../types';

export interface RateForm {
	provider: string;
	model: string;
	input: string;
	output: string;
	cacheRead: string;
	cacheWrite: string;
	/** `YYYY-MM-DD`. */
	effectiveFrom: string;
}

/** The form's fields in reading order: the first one in error takes focus. */
export const RATE_FIELDS = [
	'provider',
	'model',
	'input',
	'output',
	'cacheRead',
	'cacheWrite',
	'effectiveFrom'
] as const;
export type RateFormField = (typeof RATE_FIELDS)[number];

export type RateErrors = Partial<Record<RateFormField, string>>;

const MAX_NAME = 120;

/** Checks the form; on success returns the draft's values (without its operation ID and zone). */
export function checkRateForm(
	form: RateForm,
	today: string
):
	| { ok: true; values: Omit<PriceDraft, 'operationId' | 'timeZone'> }
	| { ok: false; errors: RateErrors } {
	const errors: RateErrors = {};
	const provider = form.provider.trim();
	const model = form.model.trim();
	if (!provider) errors.provider = 'Name the provider, such as anthropic.';
	else if (provider.length > MAX_NAME) errors.provider = `Use at most ${MAX_NAME} characters.`;
	if (!model) errors.model = 'Name the model exactly as the runtime reports it.';
	else if (model.length > MAX_NAME) errors.model = `Use at most ${MAX_NAME} characters.`;

	const rates = {} as Record<'input' | 'output' | 'cacheRead' | 'cacheWrite', number | null>;
	for (const field of ['input', 'output', 'cacheRead', 'cacheWrite'] as const) {
		const parsed = parseRate(form[field]);
		if (parsed.ok) rates[field] = parsed.value;
		else errors[field] = parsed.error;
	}
	if (
		!errors.input &&
		!errors.output &&
		!errors.cacheRead &&
		!errors.cacheWrite &&
		Object.values(rates).every((r) => r === null)
	)
		errors.input = 'Enter at least one rate. Leave a rate blank only when it isn’t known.';

	if (!isDayKey(form.effectiveFrom)) errors.effectiveFrom = 'Choose the day the rate takes effect.';
	else if (form.effectiveFrom < today)
		errors.effectiveFrom = 'A new rate takes effect from today or a later date.';

	if (Object.keys(errors).length) return { ok: false, errors };
	return {
		ok: true,
		values: {
			provider,
			model,
			inputMicroUsdPerMillion: rates.input,
			outputMicroUsdPerMillion: rates.output,
			cacheReadMicroUsdPerMillion: rates.cacheRead,
			cacheWriteMicroUsdPerMillion: rates.cacheWrite,
			effectiveFrom: form.effectiveFrom
		}
	};
}

/** The form field a server refusal points at. */
export function fieldOf(field: PriceField | undefined): RateFormField | undefined {
	if (field === 'rates') return 'input';
	return field;
}

/** The first field in error, in reading order. */
export const firstError = (errors: RateErrors): RateFormField | undefined =>
	RATE_FIELDS.find((f) => errors[f]);
