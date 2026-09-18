<script lang="ts">
	import { page } from "$app/state";
	import * as Sidebar from "sheer-ui/components/sidebar";
	import * as Breadcrumb from "sheer-ui/components/breadcrumb";

	let { data, children } = $props();

	const here = $derived.by(() => {
		for (const section of data.sections) {
			const link = section.pages.find((l) => l.href === page.url.pathname);
			if (link) return { section, link };
		}
	});
</script>

<Sidebar.Provider class="min-h-0 w-full flex-1 [--band:max(17rem,calc((100vw_-_47.5rem)_/_2_-_3rem))]">
	<aside class="hidden w-(--band) shrink-0 border-r bg-background md:block">
	<div class="sticky top-16 ml-auto h-[calc(100svh-4rem)] w-68 overflow-y-auto py-12 pr-6 pl-8">
	<Sidebar.Root collapsible="none" class="h-fit w-full bg-transparent">
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
	</div>
	</aside>
	<div class="min-w-0 flex-1 bg-card px-4 py-12 sm:px-8 lg:px-12 2xl:pr-(--band)">
	<article
		class="min-w-0 max-w-[47.5rem] 2xl:mx-auto text-base leading-7 [&_a]:text-primary [&_a]:underline [&_a]:decoration-primary/40 [&_a]:underline-offset-4 [&_a:hover]:decoration-primary [&_h1]:font-serif [&_h1]:text-5xl [&_h1]:leading-none [&_h1]:font-medium [&_h1]:tracking-tight [&_h2]:mt-12 [&_h2]:font-serif [&_h2]:text-3xl [&_h2]:font-medium [&_h2]:tracking-tight [&_h3]:label [&_h3]:mt-8 [&_h3]:text-lg [&_h3]:font-medium [&_p]:mt-4 [&_ul]:mt-4 [&_ul]:list-disc [&_ul]:pl-6 [&_table]:mt-4 [&_table]:w-full [&_table]:text-left [&_th]:label [&_th]:pb-2 [&_th]:pr-4 [&_th]:font-medium [&_td]:border-t [&_td]:py-2 [&_td]:pr-4 [&_td]:align-top [&_:not(pre)>code]:rounded-sm [&_:not(pre)>code]:bg-accent [&_:not(pre)>code]:px-1.5 [&_:not(pre)>code]:py-0.5 [&_:not(pre)>code]:font-mono [&_:not(pre)>code]:text-[0.9em] [&_:not(pre)>code]:text-accent-foreground">
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
	</div>
</Sidebar.Provider>
