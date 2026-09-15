<script lang="ts">
	import Diagram from "#lib/diagram/Diagram.svelte";
	import Box from "#lib/diagram/Box.svelte";
	import Arrow from "#lib/diagram/Arrow.svelte";
	import { anchor } from "#lib/diagram/geometry.ts";
	import field from "./field.svg?raw";

	let { data } = $props();

	const text = { x: 8, y: 72, w: 116, h: 56 };
	const piece = { x: 150, y: 78, w: 112, h: 44 };
	const engine = { x: 262, y: 48, w: 180, h: 104 };
	const decoder = { x: 512, y: 72, w: 100, h: 56 };
	const tree = { x: 640, y: 24, w: 72, h: 56 };
	const scopes = { x: 640, y: 120, w: 72, h: 56 };
	const knob = 9;
	const neck = 5;
	const mid = engine.y + engine.h / 2;

	const rust = { x: 90, y: 44, w: 110, h: 44 };
	const json = { x: 230, y: 44, w: 140, h: 44 };
	const parse = { x: 230, y: 196, w: 140, h: 44 };
	const again = { x: 90, y: 196, w: 110, h: 44 };
	const kept = { x: 460, y: 44, w: 220, h: 44 };
	const code = { x: 460, y: 196, w: 220, h: 44 };
</script>

<svelte:head>
	<title>teasel · a JavaScript parser in Rust, for JavaScript</title>
</svelte:head>

<div class="text-base leading-7 [&_h2]:font-serif [&_h2]:text-4xl [&_h2]:font-medium [&_h2]:tracking-tight md:[&_h2]:text-5xl [&_p]:mt-5 [&_p]:max-w-prose [&_p_a]:text-primary [&_p_a]:underline [&_p_a]:decoration-primary/40 [&_p_a]:underline-offset-4 [&_p_a:hover]:decoration-primary [&_:not(pre)>code]:rounded-sm [&_:not(pre)>code]:bg-accent [&_:not(pre)>code]:px-1.5 [&_:not(pre)>code]:py-0.5 [&_:not(pre)>code]:font-mono [&_:not(pre)>code]:text-[0.9em] [&_:not(pre)>code]:text-accent-foreground">
	<section class="relative overflow-hidden">
		<div class="plaid absolute inset-x-0 top-0 -z-10 h-full opacity-[0.07] [mask-image:linear-gradient(to_bottom,black,transparent_70%)]"></div>
		<div class="relative mx-auto w-full max-w-6xl px-8 pt-16 md:pt-24">
			<div class="pointer-events-none absolute top-6 right-0 w-full text-foreground opacity-25 [mask-image:linear-gradient(to_bottom,black_75%,transparent)] md:w-3/5 md:opacity-60">{@html field}</div>
			<div class="relative max-w-2xl">
				<h1 class="font-serif text-7xl font-medium tracking-tight md:text-9xl">teasel</h1>
				<p class="mt-2! max-w-none! font-serif text-2xl text-muted-foreground italic md:text-3xl">One pass, one tree, one crossing.</p>
				<p class="text-lg">A JavaScript and TypeScript parser written in Rust, made to be called from JavaScript. You get acorn's tree, the one every tool already knows how to walk, and you get the scopes, bindings and references with it, worked out during the same parse.</p>
			</div>
		</div>
		<div class="mx-auto w-full max-w-6xl px-8 pt-8 pb-16 md:pt-12">
			<Diagram width={720} height={200} label="the source text crosses into the engine once, read through the piece that says what to read, which fits the engine like a puzzle piece; the engine lexes, parses and analyses scopes in one pass and answers with one stream, which the decoder turns into the tree and the scope tables on the JavaScript side">
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
		</div>
	</section>

	<div class="plaid mx-auto h-1 max-w-6xl opacity-60"></div>

	<section class="mx-auto w-full max-w-6xl px-8 py-16 md:py-24">
		<h2>Parsing in Rust is the easy part.</h2>
		<div class="mt-8 grid items-start gap-x-16 gap-y-4 md:grid-cols-[3fr_2fr]">
			<div class="min-w-0 md:sticky md:top-24 [&_.my-8]:my-0">
				<Diagram width={720} height={300} label="on the left, a tree in Rust is written out as JSON text, which crosses to JavaScript, where JSON.parse builds the tree again; on the right, the tree stays in Rust and JavaScript code sends a question across the boundary and gets an answer back for every node it looks at">
					<g font-size="12" class="text-muted-foreground">
						<text x="8" y="70">Rust</text>
						<text x="8" y="222">JavaScript</text>
						<path d="M8 140 H 712" fill="none" stroke="currentColor" stroke-width="1.5" stroke-dasharray="4 4" />
						<text x="230" y="286" text-anchor="middle">the tree is built twice</text>
						<text x="570" y="286" text-anchor="middle">every question crosses</text>
					</g>
					<Box rect={rust} tint="violet">tree</Box>
					<Box rect={json} tint="amber">JSON text</Box>
					<Box rect={parse} tint="amber">JSON.parse</Box>
					<Box rect={again} tint="violet">tree, again</Box>
					<Arrow from={rust} to={json} />
					<Arrow from={json} to={parse} />
					<Arrow from={parse} to={again} />
					<Box rect={kept} tint="violet">tree, kept in Rust</Box>
					<Box rect={code} tint="sky">your code</Box>
					{#each [0.12, 0.5, 0.88] as at (at)}
						<Arrow from={anchor(code, 'top', at)} to={anchor(kept, 'bottom', at)} />
						<Arrow from={anchor(kept, 'bottom', at + 0.1)} to={anchor(code, 'top', at + 0.1)} />
					{/each}
				</Diagram>
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
