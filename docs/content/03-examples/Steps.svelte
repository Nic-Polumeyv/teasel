<script lang="ts">
	import run from './writing-a-grammar.json';

	type Node = { type: string; start: number; end: number; refers?: [number, number]; [field: string]: unknown };
	type Answer = { tree?: Node; error?: { code: string; message: string; pos: number; end: number } };

	let { label, step }: { label: string; step: string } = $props();

	const text: string = run.text;
	const chars = [...text];
	const answer = $derived(run.steps[Number(step) - 1] as Answer);
	let active = $state<Node | null>(null);

	const isNode = (value: unknown): value is Node => typeof value === 'object' && value !== null && typeof (value as Node).type === 'string';
	const where = (at: number) => {
		const lines = text.slice(0, at).split('\n');
		return `line ${lines.length}, column ${lines.at(-1)!.length + 1}`;
	};
	const spanned = $derived.by(() => {
		const all: Node[] = [];
		const walk = (value: unknown) => {
			if (Array.isArray(value)) value.forEach(walk);
			else if (isNode(value)) {
				all.push(value);
				for (const field in value) walk(value[field]);
			}
		};
		walk(answer.tree);
		return all;
	});
	// the smallest node over a character of the text
	const under = (at: number) => spanned.filter((n) => n.start <= at && at < n.end).sort((a, b) => a.end - a.start - (b.end - b.start))[0] ?? null;
	const inside = (at: number, range?: [number, number] | null) => range != null && range[0] <= at && at < range[1];
	const tone = (at: number) => {
		const error = answer.error;
		if (error) return at < error.pos ? colours[at] : at < Math.max(error.end, error.pos + 1) ? 'rounded-xs bg-[#e67e80]/25 text-[#e67e80]' : `${colours[at]} opacity-35`;
		if (active && inside(at, [active.start, active.end])) return `${colours[at]} bg-[#7fbbb3]/25`;
		if (inside(at, active?.refers)) return `${colours[at]} bg-[#dbbc7f]/30`;
		return colours[at];
	};
	const fields = (node: Node) => Object.entries(node).filter(([field]) => !['type', 'start', 'end', 'refers'].includes(field));
	const leaf = (value: unknown) => !isNode(value) && !Array.isArray(value);
	const ink = { keyword: 'text-[#e67e80]', type: 'text-[#dbbc7f]', property: 'text-[#7fbbb3]', string: 'text-[#a7c080]', operator: 'text-[#e69875]', punctuation: 'text-[#9da9a0]' };
	const colours = (() => {
		const out: string[] = new Array(text.length).fill('');
		const paint = (from: number, to: number, kind: keyof typeof ink) => out.fill(ink[kind], from, to);
		let tag = false;
		for (const match of text.matchAll(/\{\{[\s\S]*?\}\}|<\/?[A-Za-z][\w.:-]*|\/?>|"[^"]*"|=|\s+|[^\s<>{}="\/]+|[\s\S]/g)) {
			const [token] = match;
			const at = match.index;
			if (token.startsWith('{{')) {
				const head = /^\{\{[#:/@][\w-]*/.exec(token)?.[0].length ?? 2;
				paint(at, at + head, head > 2 ? 'keyword' : 'punctuation');
				paint(at + token.length - 2, at + token.length, head > 2 ? 'keyword' : 'punctuation');
				for (const word of token.slice(head, -2).matchAll(/[A-Za-z_$][\w$]*/g)) {
					const from = at + head + word.index;
					const kind = word[0] === 'in' ? 'keyword' : text[from - 1] === '.' ? 'property' : undefined;
					if (kind) paint(from, from + word[0].length, kind);
				}
			} else if (/^<\/?[A-Za-z]/.test(token)) {
				const name = token.replace(/^<\/?/, '');
				paint(at, at + token.length - name.length, 'punctuation');
				paint(at + token.length - name.length, at + token.length, /^[A-Z]|\./.test(name) ? 'type' : 'keyword');
				tag = true;
			} else if (tag && (token === '>' || token === '/>')) (paint(at, at + token.length, 'punctuation'), (tag = false));
			else if (tag && token === '=') paint(at, at + 1, 'operator');
			else if (tag && token[0] === '"') paint(at, at + token.length, 'string');
			else if (tag && !/^\s/.test(token)) paint(at, at + token.length, 'property');
		}
		return out;
	})();
	const point = (event: MouseEvent) => {
		const at = (event.target as HTMLElement).dataset.at;
		if (at !== undefined && answer.tree) active = under(Number(at));
	};
</script>

<figure aria-label={label} class="not-prose my-8 overflow-hidden rounded-sm bg-[#2d353b] text-[#d3c6aa] shadow-lg">
	<figcaption class="flex h-10 items-center border-b border-dashed border-[#d3c6aa]/25 px-4 font-serif text-[15px] text-[#d3c6aa]/85 italic">list.tpl</figcaption>
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
	<pre role="presentation" class="px-4 py-4 font-mono text-xs leading-6 whitespace-pre-wrap [tab-size:2] sm:text-[13px]" onmousemove={point} onmouseleave={() => (active = null)}><code
			>{#each chars as char, at (at)}<span data-at={at} class={tone(at)}>{char}</span>{/each}</code
		></pre>
	{#if answer.error}
		<p class="border-t border-dashed border-[#d3c6aa]/25 px-4 py-3 font-mono text-xs text-[#e67e80] sm:text-[13px]">
			{answer.error.code}: {answer.error.message}, {where(answer.error.pos)}
		</p>
	{:else if answer.tree}
		<div class="max-h-[30rem] overflow-y-auto border-t border-dashed border-[#d3c6aa]/25 py-3 font-mono text-xs leading-6 sm:text-[13px]" onmouseleave={() => (active = null)} role="tree" tabindex="-1">
			{@render row(answer.tree, '', 0)}
		</div>
	{/if}
</figure>

{#snippet row(node: Node, field: string, depth: number)}
	<button
		type="button"
		role="treeitem"
		aria-selected={active === node}
		class="block w-full pr-4 text-left whitespace-pre-wrap {active === node ? 'bg-[#7fbbb3]/25' : 'hover:bg-white/5'}"
		style:padding-left="{1 + depth * 1.25}rem"
		onmouseenter={() => (active = node)}
		onfocus={() => (active = node)}
		>{#if field}<span class="text-[#7fbbb3]">{field}</span>{': '}{/if}<span class="text-[#dbbc7f]">{node.type}</span
		>{#each fields(node).filter(([, value]) => leaf(value)) as [name, value] (name)}{'  '}<span class="text-[#859289]">{name}</span
			>{' '}<span class="text-[#a7c080]">{typeof value === 'string' ? JSON.stringify(value) : String(value)}</span
			>{/each}{#if node.refers}<span class="text-[#859289]">{'  '}declared at {where(node.refers[0])}</span>{/if}</button
	>
	{#each fields(node) as [name, value] (name)}
		{#if isNode(value)}
			{@render row(value, name, depth + 1)}
		{:else if Array.isArray(value) && value.length === 0}
			<div class="pr-4 whitespace-pre-wrap" style:padding-left="{1 + (depth + 1) * 1.25}rem"><span class="text-[#7fbbb3]">{name}</span>{': []'}</div>
		{:else if Array.isArray(value)}
			{#each value as child, i (i)}
				{#if isNode(child)}{@render row(child, `${name}[${i}]`, depth + 1)}{/if}
			{/each}
		{/if}
	{/each}
{/snippet}
