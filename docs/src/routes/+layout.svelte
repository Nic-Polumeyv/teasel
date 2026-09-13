<script lang="ts">
	import "./layout.css";
	import favicon from "#lib/assets/favicon.svg";
	import { initTheme } from "sheer-ui/components/theme-toggle";
	import { Header, Footer } from "sheer-ui/blocks";

	let { children } = $props();
	initTheme();
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

<div class="flex min-h-screen flex-col">
	<Header
		brand="teasel"
		actions={[
			{ href: "https://github.com/Nic-Polumeyv/teasel", label: "GitHub", variant: "ghost" },
			{ href: "https://www.npmjs.com/package/@teasel/parser", label: "npm", variant: "outline" },
		]} />
	<main class="flex-1">
		{@render children()}
	</main>
	<Footer
		navLinks={[
			{ href: "https://github.com/Nic-Polumeyv/teasel", label: "GitHub" },
			{ href: "https://www.npmjs.com/package/@teasel/parser", label: "npm" },
		]} />
</div>
