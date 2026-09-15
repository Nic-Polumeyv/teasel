<script lang="ts">
	import { Button } from "sheer-ui/components/button";

	let { data } = $props();

	function copy(event: MouseEvent) {
		const button = (event.target as HTMLElement).closest<HTMLButtonElement>('[data-copy]');
		if (!button) return;
		navigator.clipboard.writeText(button.dataset.copy!);
		button.textContent = 'Copied';
		setTimeout(() => (button.textContent = 'Copy'), 1500);
	}
</script>

<svelte:head>
	<title>{data.title} · teasel</title>
</svelte:head>

<h1>{data.title}</h1>
<svelte:document onclick={copy} />
<div>{@html data.html}</div>

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
