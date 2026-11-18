// Status pill tones for the settings screens: every pill also carries words (app-shell-ux §6a). Amber means waiting on the owner.
export const tone = {
	ok: 'bg-green-500/10 text-green-300 ring-green-500/20',
	owner: 'bg-amber-400/10 text-amber-200 ring-amber-400/30',
	neutral: '',
	info: 'bg-sky-400/10 text-sky-300 ring-sky-400/20',
	bad: 'bg-red-500/10 text-red-300 ring-red-500/20'
} as const;
