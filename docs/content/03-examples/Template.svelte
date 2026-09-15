<script lang="ts">
	import Diagram from '#lib/diagram/Diagram.svelte';
	import Box from '#lib/diagram/Box.svelte';
	import Arrow from '#lib/diagram/Arrow.svelte';
	import { anchor } from '#lib/diagram/geometry.ts';

	let { label }: { label: string } = $props();

	const text = 'Hello {{ user.name }}, you have {{ count }} new {{ count === 1 ? "message" : "messages" }}.';
	const left = 24;
	const step = 7.2;
	const at = (offset: number) => left + offset * step;
	const pieces = [
		{ start: 9, end: 18, type: 'MemberExpression', box: { x: 40, y: 110, w: 190, h: 68 } },
		{ start: 35, end: 40, type: 'Identifier', box: { x: 250, y: 110, w: 190, h: 68 } },
		{ start: 51, end: 87, type: 'ConditionalExpression', box: { x: 480, y: 110, w: 220, h: 68 } },
	];
</script>

<Diagram width={720} height={220} {label}>
	<text x="24" y="28" font-size="12" class="text-muted-foreground">one Source, {text.length} characters</text>
	<rect x="16" y="40" width="688" height="28" rx="6" fill="none" stroke="currentColor" stroke-width="1.5" class="stroke-sky-400/60 fill-sky-400/10" />
	{#each pieces as piece (piece.start)}
		<rect x={at(piece.start)} y="42" width={(piece.end - piece.start) * step} height="24" rx="4" class="fill-amber-400/25" />
	{/each}
	<text x={left} y="58" font-size="12" class="font-mono" xml:space="preserve">{text}</text>
	{#each pieces as piece (piece.start)}
		<Box rect={piece.box} tint="amber" />
		<text x={piece.box.x + piece.box.w / 2} y={piece.box.y + 22} text-anchor="middle" font-size="12" class="font-mono">parse('expression', {piece.start})</text>
		<text x={piece.box.x + piece.box.w / 2} y={piece.box.y + 42} text-anchor="middle" font-size="12">{piece.type}</text>
		<text x={piece.box.x + piece.box.w / 2} y={piece.box.y + 58} text-anchor="middle" font-size="11" class="text-muted-foreground">start {piece.start} · end {piece.end}</text>
		<Arrow from={{ x: at((piece.start + piece.end) / 2), y: 68, dx: 0, dy: 1 }} to={anchor(piece.box, 'top')} />
	{/each}
	<text x="360" y="208" font-size="12" text-anchor="middle" class="text-muted-foreground">every offset is an offset into the whole document</text>
</Diagram>
