<script lang="ts">
	import Diagram from '#lib/diagram/Diagram.svelte';
	import Box from '#lib/diagram/Box.svelte';
	import Arrow from '#lib/diagram/Arrow.svelte';
	import { anchor } from '#lib/diagram/geometry.ts';

	let { label }: { label: string } = $props();

	const total = { x: 520, y: 32, w: 130, h: 24 };
	const n = { x: 210, y: 90, w: 110, h: 24 };
	const twice = { x: 230, y: 122, w: 130, h: 24 };
	const nRef = { x: 380, y: 122, w: 100, h: 24 };
	const twiceRef = { x: 380, y: 204, w: 130, h: 24 };
	const totalRef = { x: 520, y: 204, w: 130, h: 24 };
</script>

<Diagram width={720} height={312} {label}>
	<Box rect={{ x: 8, y: 8, w: 704, h: 276 }} tint="sky" />
	<text x="24" y="30" font-size="12" class="text-muted-foreground">scope: script</text>
	<text x="24" y="49" class="font-mono">let total = 0</text>
	<Box rect={total} dashed size={12}>total · binding</Box>
	<Box rect={{ x: 24, y: 66, w: 672, h: 200 }} tint="violet" />
	<text x="40" y="88" font-size="12" class="text-muted-foreground">scope: function add</text>
	<text x="40" y="107" class="font-mono">function add(n) {'{'}</text>
	<Box rect={n} dashed size={12}>n · binding</Box>
	<text x="40" y="139" class="font-mono">  let twice = n * 2</text>
	<Box rect={twice} dashed size={12}>twice · binding</Box>
	<Box rect={nRef} dashed size={12}>n · read</Box>
	<Box rect={{ x: 40, y: 170, w: 640, h: 80 }} tint="emerald" />
	<text x="56" y="192" font-size="12" class="text-muted-foreground">scope: arrow</text>
	<text x="56" y="221" class="font-mono">  () => {'{'} total += twice {'}'}</text>
	<Box rect={twiceRef} dashed size={12}>twice · read</Box>
	<Box rect={totalRef} dashed size={12}>total · write</Box>
	<Arrow from={anchor(nRef, 'left')} to={anchor(n, 'right')} class="text-muted-foreground" />
	<Arrow from={anchor(twiceRef, 'top')} to={anchor(twice, 'bottom', 0.8)} />
	<Arrow from={anchor(totalRef, 'top')} to={anchor(total, 'bottom')} />
	<text x="360" y="304" font-size="12" text-anchor="middle" class="text-muted-foreground">an arrow that leaves the green box is a capture</text>
</Diagram>
