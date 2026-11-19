// When a one-time link stops working (offices-and-teams-ux §7.4): 24-hour time, and today, tomorrow or the date.

/** "14:02 tomorrow": the moment on its own, for "the link works until 14:02 tomorrow". */
export function expiryUntil(expiresAt: string, now = new Date()): string {
	const at = new Date(expiresAt);
	const time = at.toLocaleTimeString(undefined, {
		hour: '2-digit',
		minute: '2-digit',
		hourCycle: 'h23'
	});
	const day = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
	const days = Math.round((day(at) - day(now)) / 86_400_000);
	if (days === 0) return `${time} today`;
	if (days === 1) return `${time} tomorrow`;
	return `${time} on ${at.toLocaleDateString(undefined, { day: 'numeric', month: 'short' })}`;
}

/** "at 14:02 tomorrow": for "It works once and expires in 24 hours (at 14:02 tomorrow)". */
export const expiryText = (expiresAt: string, now = new Date()): string =>
	`at ${expiryUntil(expiresAt, now)}`;
