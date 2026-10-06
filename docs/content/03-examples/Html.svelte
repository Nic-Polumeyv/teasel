<script lang="ts">
	import Diagram from '#lib/diagram/Diagram.svelte';
	import Box from '#lib/diagram/Box.svelte';
	import Arrow from '#lib/diagram/Arrow.svelte';
	import { anchor, type Rect } from '#lib/diagram/geometry.ts';

	let { label, start = 'document' }: { label: string; start?: string } = $props();

	const lines = [
		'<ul>',
		'  {{#repeat item, i in items by item.id}}',
		'    <Card title={{ item.name }} index={{ i }} />',
		'  {{:empty}}',
		'    <li>No items</li>',
		'  {{/repeat}}',
		'</ul>',
	];
	const CHAR = 7.8;
	const LINE = 22;
	const panel: Rect = { x: 8, y: 8, w: 704, h: 202 };
	const baseline = (line: number) => 60 + line * LINE;

	// every place a text occurs on a line, as the rect around it
	const find = (text: string, only?: number[]): Rect[] =>
		lines.flatMap((line, row) => {
			const out: Rect[] = [];
			if (only && !only.includes(row)) return out;
			for (let at = line.indexOf(text); at !== -1; at = line.indexOf(text, at + 1)) {
				out.push({ x: 24 + at * CHAR - 2, y: baseline(row) - 15, w: text.length * CHAR + 4, h: 21 });
			}
			return out;
		});

	type Piece = { id: string; code: string[]; says: string[]; reads: Rect[] };
	const pieces: Piece[] = [
		{ id: 'document', code: ["document: { node: 'Template', form: [{ children: g.content }] },"], says: ['The whole file: a Template,', 'its content in children.'], reads: [panel] },
		{ id: 'other', code: ["other: { node: 'Element' },"], says: ['Every tag no other rule', 'names: <ul> and <li>.'], reads: [...find('<ul>'), ...find('</ul>'), ...find('<li>'), ...find('</li>')] },
		{ id: 'component', code: ["component: { node: 'Component' },"], says: ['A tag that starts with a', 'capital letter: <Card>.'], reads: find('<Card title={{ item.name }} index={{ i }} />') },
		{
			id: 'fields',
			code: ['fields: {', '  name: g.element.tag, attributes: g.element.attributes,', '  children: g.content },'],
			says: ['What every element has:', 'its tag name, its attributes,', 'and its content.'],
			reads: [...find('ul', [0]), ...find('title={{'), ...find('item.name', [2]), ...find('}}', [2]), ...find('index={{'), ...find('i }}', [2]), ...find('No items')],
		},
		{ id: 'text', code: ["text: { node: 'Text', form: [{ data: g.text.data }] },"], says: ['Text between tags, as read:', 'No items, and every {{ … }}', 'until a construct claims it.'], reads: [...find('No items'), ...find('{{#repeat item, i in items by item.id}}'), ...find('{{:empty}}'), ...find('{{/repeat}}')] },
		{ id: 'comment', code: ["comment: { node: 'Comment', form: [{ data: g.text.data }] },"], says: ['A <!-- comment -->. list.tpl', 'has none; every grammar', 'names one.'], reads: [] },
	];
	const rects = (() => {
		let y = 244;
		return pieces.map((piece) => {
			const h = 22 + Math.max(piece.code.length, piece.says.length) * 18;
			const rect = { x: 8, y, w: 704, h };
			y += h + 10;
			return rect;
		});
	})();
	const height = rects.at(-1)!.y + rects.at(-1)!.h + 8;

	let active = $state(start);
	const current = $derived(pieces.find((piece) => piece.id === active)!);
	const at = $derived(pieces.indexOf(current));
	const select = (id: string) => () => (active = id);
	const press = (id: string) => (event: KeyboardEvent) => {
		if (event.key === 'Enter' || event.key === ' ') {
			event.preventDefault();
			active = id;
		}
	};
</script>

<Diagram width={720} {height} {label}>
	<Box rect={panel} tint={active === 'document' ? 'sky' : undefined} />
	<text x="24" y="32" font-size="13" class="font-serif text-muted-foreground italic">list.tpl</text>
	{#each pieces as piece (piece.id)}
		{#each piece.reads.filter((read) => read !== panel) as read, i (i)}
			<rect
				x={read.x}
				y={read.y}
				width={read.w}
				height={read.h}
				rx="4"
				role="presentation"
				class={active === piece.id ? 'fill-sky-400/25 stroke-sky-400/70' : 'fill-transparent stroke-none'}
				onmouseenter={select(piece.id)}
			/>
		{/each}
	{/each}
	{#each lines as line, row (row)}
		<text x="24" y={baseline(row)} font-size="13" class="pointer-events-none font-mono" xml:space="preserve">{line}</text>
	{/each}

	{#each pieces as piece, i (piece.id)}
		{@const rect = rects[i]}
		<g
			role="button"
			tabindex="0"
			aria-label="{piece.id}: {piece.says.join(' ')}"
			aria-pressed={active === piece.id}
			class="cursor-pointer focus:outline-none"
			onmouseenter={select(piece.id)}
			onfocusin={select(piece.id)}
			onkeydown={press(piece.id)}>
			<rect x={rect.x} y={rect.y} width={rect.w} height={rect.h} class="fill-transparent" />
			<Box {rect} tint={active === piece.id ? 'sky' : undefined} />
			{#each piece.code as code, row (row)}
				<text x="24" y={rect.y + 26 + row * 18} font-size="12" class="font-mono" xml:space="preserve">{code}</text>
			{/each}
			{#each piece.says as says, row (row)}
				<text x="472" y={rect.y + 26 + row * 18} font-size="13" class="text-muted-foreground">{says}</text>
			{/each}
		</g>
	{/each}

	{#if current.reads.length > 0}
		{@const read = current.reads[0]}
		<Arrow from={anchor(rects[at], 'top', 0.012)} to={read === panel ? anchor(panel, 'bottom', 0.012) : anchor(read, 'bottom')} class="pointer-events-none text-sky-400" />
	{/if}
</Diagram>
