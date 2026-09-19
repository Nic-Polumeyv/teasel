import adapter from '@sveltejs/adapter-static';
import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	// TODO: remove once lightningcss parses `@container anchored()`: https://github.com/parcel-bundler/lightningcss/pull/1218
	css: { lightningcss: { errorRecovery: true } },
	// the interactive diagrams under content/ are imported by the browser
	server: { fs: { allow: ['content'] } },
	plugins: [
		tailwindcss(),
		sveltekit({
			compilerOptions: {
				runes: ({ filename }) => filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			adapter: adapter(),
			// 404.html is served at any depth, where relative asset urls miss
			paths: { relative: false }
		})
	]
});
