<script lang="ts">
	import Diagram from '#lib/diagram/Diagram.svelte';
	import Box from '#lib/diagram/Box.svelte';
	import Arrow from '#lib/diagram/Arrow.svelte';
	import { anchor } from '#lib/diagram/geometry.ts';

	let { label }: { label: string } = $props();

	const text = { x: 8, y: 72, w: 116, h: 56 };
	const piece = { x: 150, y: 78, w: 112, h: 44 };
	const engine = { x: 262, y: 48, w: 180, h: 104 };
	const decoder = { x: 512, y: 72, w: 100, h: 56 };
	const tree = { x: 640, y: 24, w: 72, h: 56 };
	const scopes = { x: 640, y: 120, w: 72, h: 56 };
	const knob = 9;
	const neck = 5;
	const mid = engine.y + engine.h / 2;
</script>

<Diagram width={720} height={200} {label}>
	<g font-size="12" class="text-muted-foreground">
		<rect x="138" y="8" width="318" height="184" rx="10" fill="none" stroke="currentColor" stroke-width="1.5" stroke-dasharray="4 4" />
		<text x="150" y="30">Rust</text>
	</g>
	<Box rect={text} tint="sky">source text</Box>
	<path
		d="M{engine.x + 8} {engine.y} H{engine.x + engine.w - 8} a8 8 0 0 1 8 8 V{engine.y + engine.h - 8} a8 8 0 0 1 -8 8 H{engine.x + 8} a8 8 0 0 1 -8 -8 V{mid + neck} h4 a{knob} {knob} 0 1 0 0 -{2 * neck} h-4 V{engine.y + 8} a8 8 0 0 1 8 -8 Z"
		stroke-width="1.5"
		class="fill-violet-400/15 stroke-violet-400/60" />
	<text x={engine.x + engine.w / 2 + 6} y={mid - 4} text-anchor="middle">engine</text>
	<text x={engine.x + engine.w / 2 + 6} y={mid + 18} text-anchor="middle" font-size="11" class="text-muted-foreground">lex · parse · scopes</text>
	<path
		d="M{piece.x + 6} {piece.y} H{engine.x} V{mid - neck} h4 a{knob} {knob} 0 1 1 0 {2 * neck} h-4 V{piece.y + piece.h} H{piece.x + 6} a6 6 0 0 1 -6 -6 V{piece.y + 6} a6 6 0 0 1 6 -6 Z"
		stroke-width="1.5"
		class="fill-amber-400/15 stroke-amber-400/60" />
	<text x={piece.x + piece.w / 2 - 4} y={mid} text-anchor="middle" dominant-baseline="central" font-size="12">what to read</text>
	<Box rect={decoder} tint="emerald">decoder</Box>
	<Box rect={tree} tint="rose">tree</Box>
	<Box rect={scopes} tint="rose">scopes</Box>
	<Arrow from={text} to={piece} />
	<Arrow from={engine} to={decoder} />
	<text x={(engine.x + engine.w + decoder.x) / 2} y={mid - 12} text-anchor="middle" font-size="11" class="text-muted-foreground">one stream</text>
	<Arrow from={anchor(decoder, 'right', 0.35)} to={tree} />
	<Arrow from={anchor(decoder, 'right', 0.65)} to={scopes} />
</Diagram>
