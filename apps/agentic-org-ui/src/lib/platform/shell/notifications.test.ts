import { describe, expect, it } from 'vitest';
import { ShellNotificationsState, formatRelativeNotificationTime } from './notifications.svelte';

describe('ShellNotificationsState (TASK-145)', () => {
	it('starts empty with zero fake items and tracks live background jobs to completion', () => {
		const store = new ShellNotificationsState();
		expect(store.runningJobs).toEqual([]);
		expect(store.items).toEqual([]);
		expect(store.unreadCount).toBe(0);

		store.startJob({ id: 'job-1', title: 'Refining: Ship search', source: 'tess' });
		expect(store.runningJobs).toHaveLength(1);
		expect(store.activeTessJobs).toHaveLength(1);

		store.finishJob('job-1', {
			title: 'Tess filed T-901',
			summary: 'Ship search · Assigned to Ava',
			href: '/work/T-901'
		});
		expect(store.runningJobs).toHaveLength(0);
		expect(store.items).toHaveLength(1);
		expect(store.unreadCount).toBe(1);

		store.markAllRead();
		expect(store.unreadCount).toBe(0);

		store.clearRead();
		expect(store.items).toHaveLength(0);
	});

	it('formats relative notification timestamps cleanly', () => {
		const now = 1_700_000_000_000;
		expect(formatRelativeNotificationTime(now - 10_000, now)).toBe('Just now');
		expect(formatRelativeNotificationTime(now - 5 * 60_000, now)).toBe('5m ago');
		expect(formatRelativeNotificationTime(now - 3 * 3_600_000, now)).toBe('3h ago');
		expect(formatRelativeNotificationTime(now - 2 * 86_400_000, now)).toBe('2d ago');
	});
});
