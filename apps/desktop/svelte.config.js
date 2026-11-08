// Tauri doesn't have a Node.js server to do proper SSR, so the app is a
// single-page app: adapter-static with an index.html fallback.
// See https://v2.tauri.app/start/frontend/sveltekit/
import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
const config = {
	preprocess: vitePreprocess(),
	kit: {
		adapter: adapter({
			fallback: 'index.html'
		})
	}
};

export default config;
