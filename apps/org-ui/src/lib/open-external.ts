import { invoke } from '@tauri-apps/api/core';

/**
 * Open a link from a chat message in the system browser. The Tauri webview cannot open a new window, so a plain link would do
 * nothing there. The Rust shell checks the address (http or https only) and hands it to the browser. In a plain browser the
 * link just works, so nothing calls this.
 */
export const openExternal = (url: string): Promise<void> => invoke<void>('open_external_url', { url });
