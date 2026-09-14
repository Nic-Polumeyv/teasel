<script lang="ts">
	import { goto } from "$app/navigation";
	import * as Command from "sheer-ui/components/command";
	import * as Dialog from "sheer-ui/components/dialog";
	import { Button } from "sheer-ui/components/button";
	import { Kbd } from "sheer-ui/components/kbd";
	import type { Page } from "#lib/content.ts";

	let { pages }: { pages: Page[] } = $props();
	let open = $state(false);

	function shortcut(event: KeyboardEvent) {
		if ((event.metaKey || event.ctrlKey) && event.key === "k") {
			event.preventDefault();
			open = !open;
		}
	}

	function go(href: string) {
		open = false;
		goto(href);
	}
</script>

<svelte:window onkeydown={shortcut} />

<Button variant="outline" size="sm" class="w-56 justify-between text-muted-foreground font-normal max-md:hidden" onclick={() => (open = true)}>
	Search
	<Kbd>⌘K</Kbd>
</Button>
<Button variant="ghost" size="icon" class="md:hidden" aria-label="Search" onclick={() => (open = true)}>
	<svg viewBox="0 0 24 24" class="size-5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8" /><path d="m21 21-4.3-4.3" /></svg>
</Button>

<Dialog.Root {open} onOpenChangeComplete={(value) => (open = value)}>
	<Dialog.Content class="overflow-hidden p-0!">
		<div class="sr-only">
			<Dialog.Title>Search</Dialog.Title>
			<Dialog.Description>Search the documentation</Dialog.Description>
		</div>
		<Command.Root>
			<Command.Input placeholder="Search the docs" />
			<Command.List>
				<Command.Empty>Nothing found.</Command.Empty>
				{#each pages as page (page.href)}
					<Command.Group heading={page.title}>
						<Command.Item value={page.title} onSelect={() => go(page.href)}>{page.title}</Command.Item>
						{#each page.headings as heading (heading.id)}
							<Command.Item value={`${page.title} ${heading.text}`} onSelect={() => go(`${page.href}#${heading.id}`)}>{heading.text}</Command.Item>
						{/each}
					</Command.Group>
				{/each}
			</Command.List>
		</Command.Root>
	</Dialog.Content>
</Dialog.Root>
