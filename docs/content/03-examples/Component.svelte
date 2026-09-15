<script lang="ts">
	import Diagram from '#lib/diagram/Diagram.svelte';
	import Box from '#lib/diagram/Box.svelte';
	import Arrow from '#lib/diagram/Arrow.svelte';
	import { anchor } from '#lib/diagram/geometry.ts';

	let { label }: { label: string } = $props();

	type Rect = { x: number; y: number; w: number; h: number };
	const script = { x: 8, y: 8, w: 704, h: 64 };
	const names = ['items', 'query', 'shown', 'selected', 'pick'];
	const pill = (i: number) => ({ x: 110 + i * 118, y: 28, w: 100, h: 24 });
	const each = { x: 8, y: 150, w: 704, h: 146 };
	const item = { x: 140, y: 172, w: 80, h: 24 };
	const index = { x: 236, y: 172, w: 40, h: 24 };
	const size = (text: string, x: number, y: number) => ({ x, y, w: text.length * 7.2 + 20, h: 24 });
	const refs: { text: string; rect: Rect; to: Rect[] }[] = [
		{ text: '{query}', rect: size('{query}', 24, 104), to: [pill(1)] },
		{ text: 'shown.length', rect: size('shown.length', 110, 104), to: [pill(2)] },
		{ text: 'shown as', rect: size('shown as', 230, 104), to: [pill(2)] },
		{ text: 'item.id', rect: size('item.id', 24, 250), to: [item] },
		{ text: 'i + 1', rect: size('i + 1', 106, 250), to: [index] },
		{ text: 'item.name', rect: size('item.name', 170, 250), to: [item] },
		{ text: 'item === selected', rect: size('item === selected', 280, 250), to: [item, pill(3)] },
		{ text: '() => pick(item)', rect: size('() => pick(item)', 440, 250), to: [item, pill(4)] },
	];
</script>

<Diagram width={720} height={310} {label}>
	<Box rect={script} tint="sky" />
	<text x="24" y="44" font-size="12" class="text-muted-foreground">script</text>
	{#each names as name, i (name)}
		<Box rect={pill(i)} dashed size={12}>{name}</Box>
	{/each}
	<text x="24" y="92" font-size="12" class="text-muted-foreground">template</text>
	<Box rect={each} tint="violet" />
	<text x="24" y="190" font-size="12" class="text-muted-foreground">each block</text>
	<Box rect={item} dashed size={12}>item</Box>
	<Box rect={index} dashed size={12}>i</Box>
	{#each refs as r (r.text)}
		<Box rect={r.rect} tint="amber" size={12}>{r.text}</Box>
		{#each r.to as target, j (j)}
			<Arrow from={anchor(r.rect, 'top', r.to.length === 1 ? 0.5 : 0.3 + j * 0.4)} to={anchor(target, 'bottom', target === item || target === index ? 0.5 : 0.5)} class={j === 0 ? '' : 'text-muted-foreground'} bend={40} />
		{/each}
	{/each}
</Diagram>
