<script lang="ts">
	import Boundary from "./Boundary.svelte";
	import Pipeline from "./Pipeline.svelte";
	import field from "../../../banner.svg";
	import fieldDark from "../../../banner-dark.svg";

	let { data } = $props();
</script>

<svelte:head>
	<title>teasel · a JavaScript parser in Rust, for JavaScript</title>
</svelte:head>

<div class="text-base leading-7 [&_h2]:font-serif [&_h2]:text-4xl [&_h2]:font-medium [&_h2]:tracking-tight md:[&_h2]:text-5xl [&_p]:mt-5 [&_p]:max-w-prose [&_p_a]:text-primary [&_p_a]:underline [&_p_a]:decoration-primary/40 [&_p_a]:underline-offset-4 [&_p_a:hover]:decoration-primary [&_:not(pre)>code]:rounded-sm [&_:not(pre)>code]:bg-accent [&_:not(pre)>code]:px-1.5 [&_:not(pre)>code]:py-0.5 [&_:not(pre)>code]:font-mono [&_:not(pre)>code]:text-[0.9em] [&_:not(pre)>code]:text-accent-foreground">
	<section class="relative overflow-hidden">
		<div class="plaid absolute inset-x-0 top-0 -z-10 h-full opacity-[0.07] [mask-image:linear-gradient(to_bottom,black,transparent_70%)]"></div>
		<div class="relative mx-auto w-full max-w-6xl px-8 pt-16 md:pt-24">
			{#each [{ blur: '', mask: 'md:[mask-image:linear-gradient(to_right,transparent_30%,black_55%),linear-gradient(to_bottom,black_55%,transparent)]' }, { blur: 'blur-[3px]', mask: 'md:[mask-image:linear-gradient(to_right,transparent_5%,black_30%,transparent_55%),linear-gradient(to_bottom,black_55%,transparent)]' }] as layer (layer.blur)}
				<div class="pointer-events-none absolute right-0 bottom-0 h-48 w-full opacity-30 [mask-composite:intersect] [mask-image:linear-gradient(to_bottom,transparent_10%,black_50%,transparent_95%)] md:h-64 md:w-2/3 md:opacity-50 {layer.blur} {layer.mask}">
					<img src={field} alt="" class="h-full w-full object-cover object-left-bottom dark:hidden" />
					<img src={fieldDark} alt="" class="hidden h-full w-full object-cover object-left-bottom dark:block" />
				</div>
			{/each}
			<div class="relative max-w-2xl">
				<h1 class="font-serif text-7xl font-medium tracking-tight md:text-9xl">teasel</h1>
				<p class="mt-2! max-w-none! font-serif text-2xl text-muted-foreground italic md:text-3xl">One pass, one tree, one crossing.</p>
				<p class="text-lg">A JavaScript and TypeScript parser written in Rust, made to be called from JavaScript. You get acorn's tree, the one every tool already knows how to walk, and you get the scopes, bindings and references with it, worked out during the same parse.</p>
			</div>
		</div>
		<div class="mx-auto w-full max-w-6xl px-8 pt-8 pb-16 md:pt-12">
			<Pipeline label="the source text crosses into the engine once, read through the piece that says what to read, which fits the engine like a puzzle piece; the engine lexes, parses and analyses scopes in one pass and answers with one stream, which the decoder turns into the tree and the scope tables on the JavaScript side" />
		</div>
	</section>

	<div class="plaid mx-auto h-1 max-w-6xl opacity-60"></div>

	<section class="mx-auto w-full max-w-6xl px-8 py-16 md:py-24">
		<h2>Parsing in Rust is the easy part.</h2>
		<div class="mt-8 grid items-start gap-x-16 gap-y-4 md:grid-cols-[3fr_2fr]">
			<div class="min-w-0 md:sticky md:top-24 [&_.my-8]:my-0">
				<Boundary label="on the left, a tree in Rust is written out as JSON text, which crosses to JavaScript, where JSON.parse builds the tree again; on the right, the tree stays in Rust and JavaScript code sends a question across the boundary and gets an answer back for every node it looks at" />
			</div>
			<div class="[&_p:first-child]:mt-0">
				<p>The hard part is getting the tree back to JavaScript, and there are two ways people do it. <a href="https://github.com/swc-project/swc/blob/366817647cf0da46cc296a77b2feca6a1882d733/packages/core/src/index.ts#L98">swc</a> and <a href="https://github.com/oxc-project/oxc/blob/827fdbdf52558db556f2a00da44e02864f416c0a/napi/parser/src-js/wrap.js#L35">oxc</a>, by default, write the tree out as JSON and let you <code>JSON.parse</code> it. You paid for one tree and built two, with a string in between that grows with your file. <a href="https://github.com/tree-sitter/node-tree-sitter/blob/d9c53278dd6d95db507772cef7e62b46888d8744/src/node.cc#L322">tree-sitter</a> keeps the tree in native memory and hands you handles, so every question you ask a node, its type, its children, where it starts, is a trip across the boundary and back.</p>
				<p>teasel does neither. Your text goes in once, when you make the <code>Source</code>. What you want read, a whole program, an expression at some offset, a piece of a template that stops at your own token, is the slot the engine reads it through. The engine lexes, parses and resolves scopes in one pass and answers with a compact stream of words. The decoder on the JavaScript side turns that stream into ESTree objects, one per node, and nothing in them points back at the engine. Walk the tree, change it, serialize it, throw it away. (oxc's experimental raw transfer takes the same road, reading its arena straight from JavaScript.)</p>
			</div>
		</div>
	</section>

	<div class="plaid mx-auto h-1 max-w-6xl opacity-60"></div>

	<section class="mx-auto w-full max-w-6xl px-8 py-16 md:py-24">
		<div class="grid items-center gap-x-16 gap-y-4 md:grid-cols-[2fr_3fr]">
			<div>
				<h2>A plain tree, with the facts beside it.</h2>
				<p>Every node is an ordinary object with <code>start</code> and <code>end</code>, exactly as acorn would give it to you. Turn on <code>scopes</code> and the same parse also knows what every identifier declares or refers to. No second walk.</p>
				<p><a href="/the-answer">The answer</a> lists what each option adds. <a href="/how-it-works">How it works</a> follows one parse across the boundary and back.</p>
			</div>
			<div class="min-w-0 [&_.my-6]:my-0">{@html data.facts}</div>
		</div>
	</section>

	<div class="plaid mx-auto h-1 max-w-6xl opacity-60"></div>

	<section class="mx-auto w-full max-w-6xl px-8 py-16 md:py-24">
		<h2>For compilers, for templates, for Rust.</h2>
		<div class="mt-4 grid gap-x-12 md:grid-cols-3">
			<p>If you're writing a compiler or a bundler, you get acorn's tree faster, scopes included, and TypeScript read or stripped on the way through.</p>
			<p>If you're writing a template language, you can pull one expression out of the middle of a document and stop at your own tokens. That's <a href="/inside-a-host">Inside a host</a>.</p>
			<p>If you're writing Rust, it's a crate. Under Node the package is a native addon; in a browser or behind a bundler the same import is a WebAssembly build with the same API.</p>
		</div>
		<div class="mt-10 grid items-center gap-x-16 gap-y-6 md:grid-cols-2">
			<div class="[&_.my-6]:my-0">{@html data.install}</div>
			<p class="mt-0! text-muted-foreground"><a href="/getting-started">Getting started</a> is the first parse, four lines long.</p>
		</div>
	</section>

	<p class="mx-auto max-w-6xl px-8 pb-16 font-serif text-sm text-muted-foreground italic">A teasel is the spiky seed head of a tall thistle-looking plant. For a few hundred years, cloth makers dragged them across woven wool to raise the nap. This one raises trees.</p>
</div>
