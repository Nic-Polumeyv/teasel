<script lang="ts">
	import { Button } from "sheer-ui/components/button";
	import Boundary from "./Boundary.svelte";
	import Pipeline from "./Pipeline.svelte";

	let { data } = $props();

	const audiences = [
		{ title: "Compilers and bundlers", text: "acorn's tree, faster, with scope analysis included and TypeScript read or erased in the same pass.", href: "/scopes", link: "Scopes" },
		{ title: "Template languages", text: "Read the JavaScript inside a larger text piece by piece, each parse ended by your own tokens.", href: "/inside-a-host", link: "Inside a host" },
		{ title: "Rust programs", text: "The same parser as a crate, with the tree and the scope tables as Rust types.", href: "https://github.com/Nic-Polumeyv/teasel", link: "The crate" },
	];
</script>

<svelte:head>
	<title>teasel · a JavaScript parser in Rust, for JavaScript</title>
</svelte:head>

<div class="mx-auto w-full max-w-6xl px-8 text-base leading-7 [&_h2]:font-serif [&_h2]:text-4xl [&_h2]:font-medium [&_h2]:tracking-tight [&_p]:mt-4 [&_:not(pre)>code]:rounded-sm [&_:not(pre)>code]:bg-accent [&_:not(pre)>code]:px-1.5 [&_:not(pre)>code]:py-0.5 [&_:not(pre)>code]:font-mono [&_:not(pre)>code]:text-[0.9em] [&_:not(pre)>code]:text-accent-foreground">
	<section class="grid items-center gap-12 py-24 md:grid-cols-[3fr_2fr]">
		<div>
			<h1 class="font-serif text-6xl leading-none font-medium tracking-tight md:text-7xl">One pass, one tree, one crossing.</h1>
			<p class="mt-8 max-w-prose text-lg text-muted-foreground">teasel is a JavaScript and TypeScript parser written in Rust for JavaScript to call. It answers in ESTree, the tree acorn produces and every tool built on acorn already reads, with scopes, bindings and references worked out in the same pass.</p>
			<div class="mt-8 flex flex-wrap gap-3">
				<Button href="/getting-started">Get started</Button>
				<Button variant="outline" href="/overview">Read the docs</Button>
			</div>
		</div>
		<div class="[&_.my-6]:my-0">{@html data.install}</div>
	</section>

	<section class="mx-auto max-w-3xl py-16 [&_p_a]:text-primary [&_p_a]:underline [&_p_a]:decoration-primary/40 [&_p_a]:underline-offset-4 [&_p_a:hover]:decoration-primary">
		<p class="label text-sm text-muted-foreground">The boundary</p>
		<h2 class="mt-2">A Rust parser is fast. Getting its tree into JavaScript is what costs.</h2>
		<Boundary label="on the left, a tree in Rust is written out as JSON text, which crosses to JavaScript, where JSON.parse builds the tree again; on the right, the tree stays in Rust and JavaScript code sends a question across the boundary and gets an answer back for every node it looks at" />
		<p>Today that is done one of two ways. Serialize it, as <a href="https://github.com/swc-project/swc/blob/366817647cf0da46cc296a77b2feca6a1882d733/packages/core/src/index.ts#L98">swc</a> and <a href="https://github.com/oxc-project/oxc/blob/827fdbdf52558db556f2a00da44e02864f416c0a/napi/parser/src-js/wrap.js#L35">oxc</a> do by default: Rust writes the tree out as JSON text, JavaScript calls <code>JSON.parse</code>, and the tree is built twice, with a text between the two that grows with the file. Or keep it in Rust, as <a href="https://github.com/tree-sitter/node-tree-sitter/blob/d9c53278dd6d95db507772cef7e62b46888d8744/src/node.cc#L322">tree-sitter's bindings</a> do: JavaScript holds handles, and every question about a node, its type, its children, where it starts, is a call across the boundary and back.</p>
	</section>

	<section class="mx-auto max-w-3xl py-16 [&_p_a]:text-primary [&_p_a]:underline [&_p_a]:decoration-primary/40 [&_p_a]:underline-offset-4 [&_p_a:hover]:decoration-primary">
		<p class="label text-sm text-muted-foreground">One crossing</p>
		<h2 class="mt-2">teasel does neither.</h2>
		<Pipeline label="the source text crosses into the engine once, read through the piece that says what to read, which fits the engine like a puzzle piece; the engine lexes, parses and analyses scopes in one pass and answers with one stream, which the decoder turns into the tree and the scope tables on the JavaScript side" />
		<p>The text crosses once, when the <code>Source</code> is made. What to read, a whole program, an expression at an offset, a piece of a template that ends at the host's own token, is the piece the engine reads it through. The engine lexes, parses and analyses scopes in one pass and answers with a compact stream of words. The decoder builds the ESTree tree from that stream on the JavaScript side, one object per node, and nothing in it points back into the engine: walk it, change it, serialize it. oxc's experimental raw transfer takes the same road, reading its arena from JavaScript.</p>
	</section>

	<section class="mx-auto max-w-3xl py-16 [&_p_a]:text-primary [&_p_a]:underline [&_p_a]:decoration-primary/40 [&_p_a]:underline-offset-4 [&_p_a:hover]:decoration-primary">
		<p class="label text-sm text-muted-foreground">The answer</p>
		<h2 class="mt-2">Plain ESTree, with the facts beside it.</h2>
		<p>Every node is a plain object with <code>start</code> and <code>end</code>, as acorn spells it. Turn <code>scopes</code> on and the same parse answers what an identifier declares or refers to, without a second walk.</p>
		{@html data.facts}
		<p><a href="/the-answer">The answer</a> lists what each option adds; <a href="/how-it-works">How it works</a> follows a parse across the boundary and back.</p>
	</section>

	<section class="py-16">
		<p class="label text-sm text-muted-foreground">Who it is for</p>
		<div class="mt-6 grid gap-6 md:grid-cols-3">
			{#each audiences as audience (audience.href)}
				<div class="flex flex-col rounded-lg border bg-card p-6">
					<h3 class="font-serif text-2xl font-medium tracking-tight">{audience.title}</h3>
					<p class="flex-1 text-muted-foreground">{audience.text}</p>
					<a href={audience.href} class="mt-4 text-sm text-primary underline decoration-primary/40 underline-offset-4 hover:decoration-primary">{audience.link} →</a>
				</div>
			{/each}
		</div>
	</section>
</div>
