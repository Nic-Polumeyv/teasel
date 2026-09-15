<script lang="ts">
	import Diagram from '#lib/diagram/Diagram.svelte';
	import Box from '#lib/diagram/Box.svelte';
	import Arrow from '#lib/diagram/Arrow.svelte';
	import { anchor } from '#lib/diagram/geometry.ts';

	let { label }: { label: string } = $props();

	const tree = { x: 90, y: 44, w: 110, h: 44 };
	const json = { x: 230, y: 44, w: 140, h: 44 };
	const parse = { x: 230, y: 196, w: 140, h: 44 };
	const again = { x: 90, y: 196, w: 110, h: 44 };
	const kept = { x: 460, y: 44, w: 220, h: 44 };
	const code = { x: 460, y: 196, w: 220, h: 44 };
</script>

<Diagram width={720} height={300} {label}>
	<g font-size="12" class="text-muted-foreground">
		<text x="8" y="70">Rust</text>
		<text x="8" y="222">JavaScript</text>
		<path d="M8 140 H 712" fill="none" stroke="currentColor" stroke-width="1.5" stroke-dasharray="4 4" />
		<text x="230" y="286" text-anchor="middle">the tree is built twice</text>
		<text x="570" y="286" text-anchor="middle">every question crosses</text>
	</g>
	<Box rect={tree} tint="violet">tree</Box>
	<Box rect={json} tint="amber">JSON text</Box>
	<Box rect={parse} tint="amber">JSON.parse</Box>
	<Box rect={again} tint="violet">tree, again</Box>
	<Arrow from={tree} to={json} />
	<Arrow from={json} to={parse} />
	<Arrow from={parse} to={again} />
	<Box rect={kept} tint="violet">tree, kept in Rust</Box>
	<Box rect={code} tint="sky">your code</Box>
	{#each [0.12, 0.5, 0.88] as at (at)}
		<Arrow from={anchor(code, 'top', at)} to={anchor(kept, 'bottom', at)} />
		<Arrow from={anchor(kept, 'bottom', at + 0.1)} to={anchor(code, 'top', at + 0.1)} />
	{/each}
</Diagram>
