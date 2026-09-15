<script lang="ts">
	import Diagram from '#lib/diagram/Diagram.svelte';
	import Box from '#lib/diagram/Box.svelte';
	import Arrow from '#lib/diagram/Arrow.svelte';

	let { label }: { label: string } = $props();

	const xBinding = { x: 200, y: 44, w: 150, h: 24 };
	const yBinding = { x: 420, y: 120, w: 170, h: 24 };
	const xReference = { x: 200, y: 180, w: 190, h: 24 };
	const yReference = { x: 420, y: 180, w: 180, h: 24 };
</script>

<Diagram width={720} height={240} {label}>
	<Box rect={{ x: 8, y: 8, w: 704, h: 224 }} tint="sky" />
	<text x="24" y="32">scope: module</text>
	<text x="24" y="60" class="font-mono">let x = 1</text>
	<Box rect={{ x: 24, y: 84, w: 672, h: 132 }} tint="violet" />
	<text x="40" y="108">scope: function</text>
	<text x="40" y="136" class="font-mono">function f(y) {'{'}</text>
	<text x="40" y="196" class="font-mono">  x = y</text>
	<Box rect={xBinding} dashed size={12}>x  binding  let</Box>
	<Box rect={yBinding} dashed size={12}>y  binding  param</Box>
	<Box rect={xReference} dashed size={12}>x  reference  write</Box>
	<Box rect={yReference} dashed size={12}>y  reference  read</Box>
	<Arrow from={xReference} to={xBinding} class="text-muted-foreground" />
	<Arrow from={yReference} to={yBinding} class="text-muted-foreground" />
</Diagram>
