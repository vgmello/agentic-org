// Static browser SPA with an index.html fallback; the gateway owns server operations.
import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
const config = {
	preprocess: vitePreprocess(),
	vitePlugin: {
		dynamicCompileOptions({ filename }) {
			const norm = filename.replaceAll('\\', '/');
			if (norm.includes('/src/') && !norm.includes('/node_modules/')) {
				return { runes: true };
			}
		}
	},
	kit: {
		adapter: adapter({
			fallback: 'index.html'
		})
	}
};

export default config;
