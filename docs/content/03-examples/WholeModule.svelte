<script lang="ts">
	import Diagram from '#lib/diagram/Diagram.svelte';
	import Box from '#lib/diagram/Box.svelte';
	import Arrow from '#lib/diagram/Arrow.svelte';

	let { label }: { label: string } = $props();

	const parse = { x: 8, y: 20, w: 100, h: 36 };
	const answer = { x: 176, y: 8, w: 536, h: 60 };
	const tables = ['tree', 'scopes', 'bindings', 'references'];
	const column = (i: number) => 250 + i * 130;
	const table = (i: number) => ({ x: column(i) - 50, y: 20, w: 100, h: 36 });
	const analyses: [string, number[]][] = [
		['imports', [2, 3]],
		['module state', [3, 1]],
		['exports', [0, 2]],
		['reaches', [1, 3]],
		['calls', [3, 0]],
	];
	const row = (i: number) => 104 + i * 40;
</script>

<Diagram width={720} height={300} {label}>
	<Box rect={parse} tint="violet" size={13}>parse</Box>
	<Box rect={answer} dashed />
	<Arrow from={parse} to={answer} />
	{#each tables as name, i (name)}
		<Box rect={table(i)} tint={i === 0 ? 'sky' : 'rose'} size={13}>{name}</Box>
		<path d="M{column(i)} 68 V 272" stroke="currentColor" stroke-width="1" stroke-dasharray="2 4" fill="none" class="text-muted-foreground/60" />
	{/each}
	{#each analyses as [name, reads], i (name)}
		<text x="24" y={row(i) + 5} font-size="13">{name}</text>
		{#each reads as t (t)}
			<circle cx={column(t)} cy={row(i)} r="6" class="fill-emerald-400/80" />
		{/each}
	{/each}
	<text x="24" y="294" font-size="12" class="text-muted-foreground">what each analysis reads</text>
</Diagram>
