<script lang="ts">
	import Diagram from '#lib/diagram/Diagram.svelte';
	import Box from '#lib/diagram/Box.svelte';
	import Arrow from '#lib/diagram/Arrow.svelte';
	import { anchor } from '#lib/diagram/geometry.ts';

	let { label }: { label: string } = $props();

	const outer = { x: 430, y: 30, w: 140, h: 24 };
	const tick = { x: 430, y: 90, w: 140, h: 24 };
	const inner = { x: 430, y: 150, w: 140, h: 24 };
	const reset = { x: 590, y: 150, w: 120, h: 24 };
</script>

<Diagram width={720} height={230} {label}>
	<g class="font-mono">
		<text x="24" y="47">let count = 0;</text>
		<text x="24" y="107">function tick() {'{'} count += 1; {'}'}</text>
		<text x="24" y="167">function reset() {'{'} let count = 0; count = 1; {'}'}</text>
	</g>
	<Box rect={outer} tint="rose" size={12}>count · binding</Box>
	<Box rect={tick} tint="rose" size={12}>count · reference</Box>
	<Box rect={inner} dashed size={12}>count · binding</Box>
	<Box rect={reset} dashed size={12}>count · reference</Box>
	<Arrow from={tick} to={outer} />
	<Arrow from={anchor(reset, 'left')} to={anchor(inner, 'right')} class="text-muted-foreground" />
	<text x="584" y="47" font-size="12" class="text-muted-foreground">renamed</text>
	<text x="584" y="107" font-size="12" class="text-muted-foreground">renamed</text>
	<text x="24" y="212" font-size="12" class="text-muted-foreground">the inner count resolves to its own let and is left alone</text>
</Diagram>
