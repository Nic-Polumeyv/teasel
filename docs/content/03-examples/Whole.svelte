<script lang="ts">
	let { label, files }: { label: string; files: Record<string, string> } = $props();

	// the sheet's lines as the server highlighted them, each led by its number
	const code = /<code>([\s\S]*)<\/code>/.exec(files['tpl.js'])![1].split('\n');
	const plain = (html: string) =>
		html
			.replace(/^<span[^>]*>\d+<\/span>/, '')
			.replace(/<[^>]+>/g, '')
			.replace(/&lt;/g, '<')
			.replace(/&gt;/g, '>')
			.replace(/&amp;/g, '&');
	const text = code.map(plain);

	type Part = { id: string; name: string; step: string; href: string; tint: string };
	const parts: Part[] = [
		{ id: 'file', name: 'The file and its text', step: 'step 1', href: '#1-start-with-the-html', tint: 'bg-[#dbbc7f]/25 border-[#dbbc7f]' },
		{ id: 'elements', name: 'Elements and components', step: 'step 1', href: '#1-start-with-the-html', tint: 'bg-[#7fbbb3]/25 border-[#7fbbb3]' },
		{ id: 'expressions', name: 'Expressions', step: 'step 2', href: '#2-read-the-expressions', tint: 'bg-[#a7c080]/25 border-[#a7c080]' },
		{ id: 'repeat', name: 'The repeat block', step: 'steps 3 to 5', href: '#3-describe-the-repeat-block', tint: 'bg-[#e69875]/25 border-[#e69875]' },
		{ id: 'branch', name: 'The empty branch', step: 'step 6', href: '#6-add-the-empty-branch', tint: 'bg-[#e67e80]/25 border-[#e67e80]' },
	];

	// each line's part, by where it stands among the grammar's keys
	const at = (start: string, from = 0) => text.findIndex((line, i) => i >= from && line.startsWith(start));
	const of: (string | undefined)[] = new Array(text.length).fill(undefined);
	const mark = (id: string, from: number, to: number) => of.fill(id, from, to + 1);
	const constructs = at('\tconstructs: {');
	const repeat = at('\t\trepeat: {');
	const branch = at('\t\t\tbranches:');
	mark('file', at('\tdocument:'), at('\tcomment:'));
	mark('elements', at('\telements: {'), constructs - 1);
	mark('expressions', constructs, repeat - 1);
	mark('repeat', repeat, branch - 1);
	mark('branch', branch, branch);
	mark('repeat', branch + 1, at('\t\t},', branch));

	let active = $state<string | null>(null);
	let copied = $state(false);
	const tint = (id: string | undefined) => parts.find((part) => part.id === id)?.tint ?? 'border-transparent';
	const copy = async () => {
		await navigator.clipboard.writeText(text.join('\n'));
		copied = true;
		setTimeout(() => (copied = false), 1500);
	};
</script>

<figure aria-label={label} class="not-prose my-8 overflow-hidden rounded-sm bg-[#2d353b] text-[#d3c6aa] shadow-lg">
	<figcaption class="flex h-10 items-center gap-2 border-b border-dashed border-[#d3c6aa]/25 px-4 font-serif text-[15px] text-[#d3c6aa]/85 italic">
		tpl.js
		<button type="button" class="ml-auto font-sans text-xs text-white/50 not-italic hover:text-white/80" onclick={copy}>{copied ? 'copied' : 'copy'}</button>
	</figcaption>
	<nav aria-label="the parts of the grammar" class="flex flex-wrap gap-2 border-b border-dashed border-[#d3c6aa]/25 px-4 py-3 font-sans text-xs" onmouseleave={() => (active = null)}>
		{#each parts as part (part.id)}
			<a
				href={part.href}
				class="flex items-center gap-2 rounded-sm border-l-4 px-2 py-1 text-[#d3c6aa]! no-underline! transition-opacity {part.tint} {active && active !== part.id ? 'opacity-40' : ''}"
				onmouseenter={() => (active = part.id)}
				onfocus={() => (active = part.id)}
				>{part.name}<span class="text-white/45">{part.step}</span></a
			>
		{/each}
	</nav>
	<pre class="py-3 pr-4 font-mono text-xs leading-6 whitespace-pre-wrap [overflow-wrap:anywhere] [tab-size:2] sm:text-[13px]"><code
			>{#each code as line, i (i)}<!-- svelte-ignore a11y_no_static_element_interactions --><span
					class="block border-l-4 transition-opacity {tint(of[i])} {active && of[i] !== active ? 'opacity-35' : ''}"
					onmouseenter={() => (active = of[i] ?? null)}
					onmouseleave={() => (active = null)}>{@html line}</span
				>{/each}</code
		></pre>
</figure>
