<script lang="ts">
	import Diagram from '#lib/diagram/Diagram.svelte';
	import Box from '#lib/diagram/Box.svelte';
	import Arrow from '#lib/diagram/Arrow.svelte';
	import { anchor } from '#lib/diagram/geometry.ts';

	let { label }: { label: string } = $props();

	const text = { x: 8, y: 24, w: 160, h: 56 };
	const rules = { x: 8, y: 120, w: 160, h: 56 };
	const engine = { x: 234, y: 60, w: 200, h: 80 };
	const decoder = { x: 486, y: 72, w: 100, h: 56 };
	const tree = { x: 638, y: 24, w: 74, h: 56 };
	const scopes = { x: 638, y: 120, w: 74, h: 56 };
</script>

<Diagram width={720} height={200} {label}>
	<Box rect={text} tint="sky">source text</Box>
	<Box rect={rules} tint="amber">what to read</Box>
	<Box rect={engine} tint="violet">engine</Box>
	<Box rect={decoder} tint="emerald">decoder</Box>
	<Box rect={tree} tint="rose">tree</Box>
	<Box rect={scopes} tint="rose">scopes</Box>
	<Arrow from={text} to={anchor(engine, 'left', 0.35)} />
	<Arrow from={rules} to={anchor(engine, 'left', 0.65)} />
	<Arrow from={engine} to={decoder} />
	<Arrow from={anchor(decoder, 'right', 0.35)} to={tree} />
	<Arrow from={anchor(decoder, 'right', 0.65)} to={scopes} />
</Diagram>
