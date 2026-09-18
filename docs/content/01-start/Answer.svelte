<script lang="ts">
	import Diagram from '#lib/diagram/Diagram.svelte';
	import Box from '#lib/diagram/Box.svelte';

	let { label }: { label: string } = $props();

	const node = { x: 24, y: 52, w: 200, h: 124 };
	const end = { x: 248, y: 52, w: 120, h: 52 };
	const column = (x: number, row: number) => ({ x, y: 52 + row * 44, w: 140, h: 36 });
</script>

<Diagram width={720} height={244} {label}>
	<Box rect={{ x: 8, y: 8, w: 704, h: 228 }} dashed />
	<text x="24" y="32" class="font-mono">{'{ node, end, …tables }'}</text>
	<Box rect={node} tint="sky" />
	<text x="124" y="80" text-anchor="middle">node</text>
	<g text-anchor="middle" font-size="12" class="text-muted-foreground">
		<text x="124" y="104">an ESTree tree</text>
		<text x="124" y="124">start, end on every node</text>
		<text x="124" y="144">loc with locations</text>
	</g>
	<Box rect={end} tint="amber">end</Box>
	{#each ['comments', 'errors', 'typescript'] as table, row (table)}
		<Box rect={column(392, row)} tint="emerald" size={13}>{table}</Box>
	{/each}
	{#each ['scopes', 'bindings', 'references', 'roots'] as table, row (table)}
		<Box rect={column(556, row)} tint="rose" size={13}>{table}</Box>
	{/each}
</Diagram>
