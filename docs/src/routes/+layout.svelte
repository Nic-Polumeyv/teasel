<script lang="ts">
	import "./layout.css";
	import favicon from "#lib/assets/favicon.svg";
	import { page } from "$app/state";
	import { initTheme, ThemeToggle } from "sheer-ui/components/theme-toggle";
	import { Button } from "sheer-ui/components/button";
	import { MobileNav } from "sheer-ui/blocks";
	import Search from "#lib/Search.svelte";
	import * as Tooltip from "sheer-ui/components/tooltip";
	import * as Sidebar from "sheer-ui/components/sidebar";
	import * as Breadcrumb from "sheer-ui/components/breadcrumb";

	let { data, children } = $props();
	initTheme();

	const here = $derived.by(() => {
		for (const section of data.sections) {
			const link = section.pages.find((l) => l.href === page.url.pathname);
			if (link) return { section, link };
		}
	});
</script>

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
			<Sidebar.Provider class="mx-auto min-h-0 w-full max-w-6xl gap-12 px-8 py-12">
				<Sidebar.Root collapsible="none" class="sticky top-24 hidden h-fit w-48 bg-transparent md:flex">
					<Sidebar.Content class="gap-6">
						{#each data.sections as section (section.label)}
							<Sidebar.Group class="p-0">
								<Sidebar.GroupLabel class="label h-auto px-2 pb-2 text-sm">{section.label}</Sidebar.GroupLabel>
								<Sidebar.GroupContent>
									<Sidebar.Menu>
										{#each section.pages as link (link.href)}
											<Sidebar.MenuItem>
												<Sidebar.MenuButton isActive={page.url.pathname === link.href}>
													{#snippet child({ props })}
														<a href={link.href} {...props}>{link.title}</a>
													{/snippet}
												</Sidebar.MenuButton>
											</Sidebar.MenuItem>
										{/each}
									</Sidebar.Menu>
								</Sidebar.GroupContent>
							</Sidebar.Group>
						{/each}
					</Sidebar.Content>
				</Sidebar.Root>
				<article
					class="min-w-0 max-w-prose flex-1 text-base leading-7 [&_a]:text-primary [&_a]:underline [&_a]:decoration-primary/40 [&_a]:underline-offset-4 [&_a:hover]:decoration-primary [&_h1]:font-serif [&_h1]:text-5xl [&_h1]:leading-none [&_h1]:font-medium [&_h1]:tracking-tight [&_h2]:mt-12 [&_h2]:font-serif [&_h2]:text-3xl [&_h2]:font-medium [&_h2]:tracking-tight [&_h3]:label [&_h3]:mt-8 [&_h3]:text-lg [&_h3]:font-medium [&_p]:mt-4 [&_ul]:mt-4 [&_ul]:list-disc [&_ul]:pl-6 [&_:not(pre)>code]:rounded-sm [&_:not(pre)>code]:bg-accent [&_:not(pre)>code]:px-1.5 [&_:not(pre)>code]:py-0.5 [&_:not(pre)>code]:font-mono [&_:not(pre)>code]:text-[0.9em] [&_:not(pre)>code]:text-accent-foreground">
					{#if here}
						<Breadcrumb.Root class="mb-4 [&_a]:no-underline">
							<Breadcrumb.List>
								<Breadcrumb.Item>{here.section.label}</Breadcrumb.Item>
								<Breadcrumb.Separator />
								<Breadcrumb.Item><Breadcrumb.Page>{here.link.title}</Breadcrumb.Page></Breadcrumb.Item>
							</Breadcrumb.List>
						</Breadcrumb.Root>
					{/if}
					{@render children()}
				</article>
			</Sidebar.Provider>
		</main>
		<div class="plaid h-2.5"></div>
	</div>
</Tooltip.Provider>
