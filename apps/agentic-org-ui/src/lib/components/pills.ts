// Status pill tones for the settings screens: every pill also carries words (app-shell-ux §6a). Amber means waiting on the owner.
export const tone = {
	ok: 'bg-success-emphasis/10 text-success-text ring-success-emphasis/20',
	owner: 'bg-warning/10 text-warning-text-strong ring-warning/30',
	neutral: '',
	info: 'bg-info/10 text-info-text ring-info/20',
	bad: 'bg-danger-emphasis/10 text-danger-text ring-danger-emphasis/20'
} as const;

/** Compact status-ring tone shared by WorkReadingPanel and the work module. */
export type WorkStatusRingTone = 'active' | 'done' | 'blocked' | 'review' | 'neutral';
