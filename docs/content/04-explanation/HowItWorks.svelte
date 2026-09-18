<script lang="ts">
	import Diagram from '#lib/diagram/Diagram.svelte';
	import Box from '#lib/diagram/Box.svelte';
	import Arrow from '#lib/diagram/Arrow.svelte';

	let { label }: { label: string } = $props();

	const source = { x: 20, y: 44, w: 200, h: 44 };
	const kept = { x: 442, y: 44, w: 240, h: 44 };
	const parse = { x: 20, y: 120, w: 200, h: 44 };
	const pass = { x: 442, y: 108, w: 240, h: 68 };
	const tree = { x: 442, y: 196, w: 240, h: 44 };
	const reader = { x: 20, y: 196, w: 200, h: 44 };
</script>

<Diagram width={720} height={260} {label}>
	<g text-anchor="middle" font-size="12" class="text-muted-foreground">
		<text x="120" y="24">JavaScript</text>
		<text x="560" y="24">Rust</text>
		<path d="M360 8 V 252" fill="none" stroke="currentColor" stroke-width="1.5" stroke-dasharray="4 4" />
	</g>
	<Box rect={source} tint="sky">new Source(text)</Box>
	<Box rect={kept} tint="violet">the source, kept</Box>
	<Box rect={parse} tint="amber">source.parse(…)</Box>
	<Box rect={pass} tint="violet" />
	<text x="562" y="132" text-anchor="middle">lex → parse → scopes</text>
	<text x="562" y="156" text-anchor="middle" font-size="11" class="text-muted-foreground">one tree, pooled between parses</text>
	<Box rect={tree} tint="violet">the tree, in place</Box>
	<Box rect={reader} tint="emerald">reader → ESTree</Box>
	<Arrow from={source} to={kept} />
	<Arrow from={parse} to={pass} />
	<Arrow from={tree} to={reader} />
</Diagram>
