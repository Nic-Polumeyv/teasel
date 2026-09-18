<script lang="ts">
	import "./layout.css";
	import favicon from "#lib/assets/favicon.svg";
	import { page } from "$app/state";
	import { initTheme, ThemeToggle } from "sheer-ui/components/theme-toggle";
	import { Button } from "sheer-ui/components/button";
	import { MobileNav } from "sheer-ui/blocks";
	import Search from "#lib/Search.svelte";
	import * as Tooltip from "sheer-ui/components/tooltip";

	let { data, children } = $props();
	initTheme();

	function copy(event: MouseEvent) {
		const button = (event.target as HTMLElement).closest<HTMLButtonElement>('[data-copy]');
		if (!button) return;
		navigator.clipboard.writeText(button.dataset.copy!);
		button.dataset.copied = '';
		setTimeout(() => delete button.dataset.copied, 1500);
	}

</script>

<svelte:document onclick={copy} />

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

<Tooltip.Provider>
	<div class="flex min-h-screen flex-col">
		<div class="plaid h-2.5"></div>
		<header class="sticky top-0 z-50 bg-background">
			<div class="mx-auto flex h-16 w-full max-w-6xl items-center gap-6 px-8">
				<a href="/" class="font-serif text-2xl font-medium tracking-tight">teasel</a>
				<nav class="hidden gap-1 md:flex">
					{#each data.sections as section (section.label)}
						<Button variant="ghost" size="sm" href={section.pages[0].href}>{section.label}</Button>
					{/each}
				</nav>
				<div class="ml-auto flex items-center gap-1">
					<Search pages={data.pages} />
					<Button variant="ghost" size="icon" href="https://github.com/Nic-Polumeyv/teasel" aria-label="GitHub">
						<svg viewBox="0 0 24 24" class="size-5" fill="currentColor"><path d="M12 .3a12 12 0 0 0-3.8 23.4c.6.1.8-.3.8-.6v-2c-3.3.7-4-1.6-4-1.6-.6-1.4-1.4-1.8-1.4-1.8-1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1.1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.8-1.6-2.7-.3-5.5-1.3-5.5-5.9 0-1.3.5-2.4 1.2-3.2-.1-.3-.5-1.5.1-3.2 0 0 1-.3 3.3 1.2a11.5 11.5 0 0 1 6 0c2.3-1.5 3.3-1.2 3.3-1.2.6 1.7.2 2.9.1 3.2.8.8 1.2 1.9 1.2 3.2 0 4.6-2.8 5.6-5.5 5.9.4.4.8 1.1.8 2.2v3.3c0 .3.2.7.8.6A12 12 0 0 0 12 .3" /></svg>
					</Button>
					<ThemeToggle />
					<MobileNav
						navLinks={data.sections.map((section) => ({
							href: section.pages[0].href,
							label: section.label,
							children: section.pages.map((link) => ({ href: link.href, label: link.title })),
						}))}
						actions={[{ href: "https://github.com/Nic-Polumeyv/teasel", label: "GitHub", variant: "outline" }]} />
				</div>
			</div>
		</header>
		<main class="flex-1">
			{@render children()}
		</main>
		<div class="plaid h-2.5"></div>
	</div>
</Tooltip.Provider>
