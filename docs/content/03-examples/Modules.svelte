<script lang="ts">
	import * as Dialog from 'sheer-ui/components/dialog';
	import Diagram from '#lib/diagram/Diagram.svelte';
	import Arrow from '#lib/diagram/Arrow.svelte';
	import { anchor, type Side } from '#lib/diagram/geometry.ts';

	let { label, files = {} }: { label: string; files?: Record<string, string> } = $props();

	const cards: Record<string, { x: number; y: number; w: number; h: number; tilt: number }> = {
		'world.js': { x: 60, y: 150, w: 150, h: 52, tilt: -2 },
		'player.js': { x: 510, y: 60, w: 150, h: 52, tilt: 2 },
		'game.js': { x: 340, y: 240, w: 150, h: 52, tilt: 1 },
	};
	const edges: { from: string; out: Side; to: string; in: Side }[] = [
		{ from: 'player.js', out: 'left', to: 'world.js', in: 'top' },
		{ from: 'game.js', out: 'left', to: 'world.js', in: 'bottom' },
		{ from: 'game.js', out: 'top', to: 'player.js', in: 'bottom' },
	];
	const center = (r: { x: number; y: number; w: number; h: number }) => ({ x: r.x + r.w / 2, y: r.y + r.h / 2 });
	let open = $state<string | null>(null);
</script>

<Diagram width={720} height={320} {label}>
	<path d="M8 34 v-16 a6 6 0 0 1 6 -6 h108 a6 6 0 0 1 5 3 l10 13 h569 a6 6 0 0 1 6 6 v274 a6 6 0 0 1 -6 6 h-692 a6 6 0 0 1 -6 -6 z" stroke-width="1.5" class="fill-secondary/50 stroke-secondary-foreground/40" />
	<text x="24" y="28" font-size="12" class="font-mono text-secondary-foreground">adventure/</text>
	{#each edges as edge (edge.from + edge.to)}
		<Arrow from={anchor(cards[edge.from], edge.out)} to={anchor(cards[edge.to], edge.in)} class="text-muted-foreground" />
	{/each}
	{#each Object.entries(cards) as [name, card] (name)}
		{@const c = center(card)}
		<g
			role="button"
			tabindex="0"
			aria-label="open {name}"
			transform="rotate({card.tilt} {c.x} {c.y})"
			class="group cursor-pointer focus:outline-none"
			onclick={() => (open = name)}
			onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); open = name; } }}>
			<rect x={card.x + 3} y={card.y + 4} width={card.w} height={card.h} rx="6" class="fill-black/20" stroke="none" />
			<rect x={card.x} y={card.y} width={card.w} height={card.h} rx="6" stroke-width="1.5" class="fill-card stroke-border transition-colors group-hover:fill-accent group-hover:stroke-primary group-focus-visible:stroke-primary" />
			<path d="M{card.x + 18} {card.y + 17} h7l4 4v11h-11z M{card.x + 25} {card.y + 17} v4h4" fill="none" stroke="currentColor" stroke-width="1.2" class="text-muted-foreground" />
			<text x={card.x + 42} y={c.y} dominant-baseline="central" font-size="14" class="font-mono">{name}</text>
		</g>
	{/each}
</Diagram>

<Dialog.Root open={open !== null} onOpenChange={(value) => { if (!value) open = null; }}>
	<Dialog.Content class="p-0 sm:max-w-4xl [&_figure]:my-0 [&_figure]:rounded-none [&_figure]:border-0 [&_figure]:shadow-none">
		<Dialog.Title class="sr-only">{open}</Dialog.Title>
		<Dialog.Description class="sr-only">the file, highlighted</Dialog.Description>
		{#if open !== null}{@html files[open]}{/if}
	</Dialog.Content>
</Dialog.Root>
