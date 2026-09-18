<script lang="ts">
	import { hydrate, unmount, type Component } from "svelte";
	import { Button } from "sheer-ui/components/button";

	let { data } = $props();

	// an interactive diagram was rendered into the page's html; hydrate it in place with the props it was rendered with
	const islands = import.meta.glob("/content/**/*.svelte") as Record<string, () => Promise<{ default: Component<any> }>>;
	let article: HTMLElement;
	$effect(() => {
		data.html;
		const mounted: Record<string, any>[] = [];
		let live = true;
		for (const target of article.querySelectorAll<HTMLElement>("[data-island]")) {
			islands[target.dataset.island!]().then((module) => {
				if (live) mounted.push(hydrate(module.default, { target, props: JSON.parse(target.dataset.props!) }));
			});
		}
		return () => {
			live = false;
			for (const island of mounted) unmount(island);
		};
	});

	let note = $state<{ html: string; left: number; top: number }>();
	let about: HTMLElement | undefined;
	let pinned = false;
	let leaving: ReturnType<typeof setTimeout> | undefined;
	const noted = (event: Event) => (event.target as HTMLElement).closest<HTMLElement>('[data-note]');

	function place() {
		if (!about) return;
		const box = about.getBoundingClientRect();
		note = { html: about.dataset.note!, left: Math.max(16, Math.min(box.left, innerWidth - 400)), top: box.bottom + 8 };
	}

	function show(target: HTMLElement, pin: boolean) {
		clearTimeout(leaving);
		about = target;
		pinned = pin;
		place();
	}

	function hide() {
		clearTimeout(leaving);
		about = note = undefined;
		pinned = false;
	}

	function over(event: Event) {
		const target = noted(event);
		if (target && !pinned) show(target, false);
	}

	function out(event: Event) {
		if (noted(event) && !pinned) leaving = setTimeout(hide, 250);
	}

	function press(event: MouseEvent) {
		const target = noted(event);
		if (target) show(target, !(pinned && about === target));
		else if (!(event.target as HTMLElement).closest('[data-note-panel]')) hide();
		if (target && !pinned) hide();
	}

	function key(event: KeyboardEvent) {
		if (event.key === 'Escape') hide();
		const target = noted(event);
		if (target && (event.key === 'Enter' || event.key === ' ')) {
			event.preventDefault();
			show(target, !(pinned && about === target));
			if (!pinned) hide();
		}
	}

</script>

<svelte:head>
	<title>{data.title} · teasel</title>
</svelte:head>

<h1>{data.title}</h1>
<svelte:document onclick={press} onpointerover={over} onpointerout={out} onfocusin={over} onfocusout={out} onkeydown={key} />
<svelte:window onscroll={place} onresize={place} />
<div bind:this={article}>{@html data.html}</div>
{#if note}
	<div
		data-note-panel
		role="note"
		class="fixed z-50 w-96 max-w-[calc(100vw-2rem)] rounded-md border bg-popover p-3 text-sm leading-6 text-popover-foreground shadow-lg [&_code]:rounded-sm [&_code]:bg-accent [&_code]:px-1 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-[0.9em]"
		style:left="{note.left}px"
		style:top="{note.top}px"
		onpointerenter={() => clearTimeout(leaving)}
		onpointerleave={() => !pinned && (leaving = setTimeout(hide, 250))}>
		{@html note.html}
	</div>
{/if}

<div class="mt-16 flex flex-col gap-6 border-t pt-6">
	<a href={data.edit} class="label w-fit text-sm text-muted-foreground no-underline hover:text-foreground">Edit this page on GitHub</a>
	<div class="flex justify-between gap-4">
		{#if data.prev}
			<Button variant="ghost" href={data.prev.href} class="no-underline">← {data.prev.title}</Button>
		{:else}<span></span>{/if}
		{#if data.next}
			<Button variant="ghost" href={data.next.href} class="no-underline">{data.next.title} →</Button>
		{/if}
	</div>
</div>
