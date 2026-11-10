/** Where the menu bar may send the window. Anything else is ignored. */
const APP_PATHS = ['/', '/welcome', '/create', '/agents', '/offices'] as const;

export const isAppPath = (value: unknown): value is (typeof APP_PATHS)[number] =>
	typeof value === 'string' && (APP_PATHS as readonly string[]).includes(value);
