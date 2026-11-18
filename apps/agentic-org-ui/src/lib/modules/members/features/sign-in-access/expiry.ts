// "at 14:02 tomorrow" for a one-time link (offices-and-teams-ux §7.4): 24-hour time, and today, tomorrow or the date.
export function expiryText(expiresAt: string, now = new Date()): string {
	const at = new Date(expiresAt);
	const time = at.toLocaleTimeString(undefined, {
		hour: '2-digit',
		minute: '2-digit',
		hourCycle: 'h23'
	});
	const day = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
	const days = Math.round((day(at) - day(now)) / 86_400_000);
	if (days === 0) return `at ${time} today`;
	if (days === 1) return `at ${time} tomorrow`;
	return `at ${time} on ${at.toLocaleDateString(undefined, { day: 'numeric', month: 'short' })}`;
}
