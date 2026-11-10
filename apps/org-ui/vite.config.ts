import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vitest/config';
import { productionImportGuard } from './src/lib/platform/tooling/production-import-guard';

// Set by `tauri dev` when it targets a device on the network (mobile); unused on desktop.
const host = process.env.TAURI_DEV_HOST;

// https://v2.tauri.app/start/frontend/sveltekit/
export default defineConfig({
	// Report until Avery's module moves and the production-factory cutovers are complete.
	plugins: [tailwindcss(), sveltekit(), productionImportGuard({ mode: 'report' })],
	// Keep Rust errors visible in the terminal.
	clearScreen: false,
	server: {
		port: 1420,
		strictPort: true,
		host: host || false,
		hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
		watch: {
			// Don't let Vite watch the Rust side.
			ignored: ['**/src-tauri/**']
		}
	},
	test: {
		include: ['src/**/*.{test,spec}.{js,ts}'],
		environment: 'node'
	}
});
