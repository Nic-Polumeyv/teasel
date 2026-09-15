---
title: A component, piece by piece
---

A template language owns the file. It has a script block, which is a whole program, and a template, which is its own syntax with JavaScript expressions inside it. Here is one, and a reader for it: one `Source` for the whole file, the script read as a program between its tags, every expression and every `each` head read in place, and the template's names stitched to the script's bindings.

```svelte
<script>
	import { onMount } from './lifecycle.js';
	let { items = [], selected = null } = $props();
	let query = $state('');
	const shown = $derived(items.filter((item) => item.name.includes(query)));
	function pick(item) { selected = item; }
</script>

<input bind:value={query} placeholder="filter" />
{#if shown.length === 0}
	<p>nothing matches "{query}"</p>
{:else}
	<ul>
		{#each shown as item, i (item.id)}
			<li class:active={item === selected} onclick={() => pick(item)}>{i + 1}. {item.name}</li>
		{/each}
	</ul>
{/if}
```

## The source, and the script

The `Source` is the whole file. Everything parsed out of it, the script and each expression, reports offsets into this text.

```js
import { Source } from '@teasel/parser';

const source = new Source(text, { sourceType: 'module', scopes: true });

const open = text.indexOf('<script>') + '<script>'.length;
const close = text.indexOf('</script>');
const script = source.parse('program', open, { end: close });
```

`program` with `end` reads exactly the text between the tags as a module. The answer's `bindings` are the script's declarations, and they are what the template's names will resolve to.

```text
script bindings: onMount:import  items:let  selected:let  query:let  shown:const  pick:function  item:param  item:param
```

## The template

A scanner finds `{`. What follows decides what to ask for. An expression ends at `}`; an `each` head is three parses, the list ended by `as`, the item as a `pattern` ended by `,` or `(`, the index as another pattern, and the key as an expression ended by `)`.

```js
const skip = (i) => { while (text[i] === ' ' || text[i] === '\t') i += 1; return i; };
const inScope = [script.bindings];   // a stack: the script's bindings, then each block's own

function expression(from, stopAt) {
	const answer = source.parse('expression', from, { stopAt });
	for (const r of answer.references) {
		if (r.binding !== null) continue;   // a piece declares nothing, so every name in it is the host's to resolve
		const found = inScope.flat().findLast((b) => b.name === r.node.name);
		resolved.push([r.node.name, r.node.start, found ? `${found.kind} at ${found.node.start}` : 'unresolved']);
	}
	return answer;
}

let at = close;
while ((at = text.indexOf('{', at)) !== -1) {
	if (text.startsWith('{#each ', at)) {
		const list = expression(at + 7, ['as']);
		const item = source.parse('pattern', list.end + 4, { stopAt: [',', '(', '}'] });
		const scope = [...item.bindings];
		let next = skip(item.end);
		if (text[next] === ',') {
			const index = source.parse('pattern', skip(next + 1), { stopAt: ['(', '}'] });
			scope.push(...index.bindings);
			next = skip(index.end);
		}
		inScope.push(scope);
		if (text[next] === '(') next = expression(next + 1, [')']).end + 1;
		at = text.indexOf('}', next) + 1;
	} else if (text.startsWith('{/each}', at)) { inScope.pop(); at += 7; }
	else if (text.startsWith('{#if ', at)) at = expression(at + 5, ['}']).end + 1;
	else if (text.startsWith('{:else}', at) || text.startsWith('{/if}', at)) at += 1;
	else at = expression(at + 1, ['}']).end + 1;
}
```

The parses, in the order they were asked for, with the offset each started at:

```text
program      8  Program
expression 274  Identifier                 bind:value={query}
expression 310  BinaryExpression           {#if shown.length === 0}
expression 352  Identifier                 "{query}"
expression 387  Identifier                 {#each shown as …
pattern    396  Identifier   declares item
pattern    402  Identifier   declares i
expression 405  MemberExpression           (item.id)
expression 436  BinaryExpression           class:active={item === selected}
expression 464  ArrowFunctionExpression    onclick={() => pick(item)}
expression 483  BinaryExpression           {i + 1}
expression 492  MemberExpression           {item.name}
```

## Stitching the scopes

Each expression is parsed on its own, so from the parser's side every name in it is a global: `binding` is `null`. That's the contract. The host knows the template's scoping rules, so the host resolves them: against the script's bindings, and against whatever the enclosing `each` declared, which the `pattern` parses handed back as bindings of kind `pattern`.

```text
query      at 274 -> let at 106
shown      at 310 -> const at 133
query      at 352 -> let at 106
shown      at 387 -> const at 133
item       at 405 -> pattern at 396
item       at 436 -> pattern at 396
selected   at 445 -> let at 71
pick       at 470 -> function at 212
item       at 475 -> pattern at 396
i          at 483 -> pattern at 402
item       at 492 -> pattern at 396
```

![the script block at the top with its bindings; the each block below declaring item and i; every expression in the template with an arrow from each name to the binding it resolves to, in the script or in the each block](Component.svelte)

Note `pick(item)` inside the arrow function: the arrow's own scope is the parser's, and `item` inside it still comes back as a global for the host to resolve, because nothing in that expression declared it. Had the arrow been `(item) => pick(item)`, the parser would have resolved `item` itself, to the parameter, and the host would never see it.

## What the host keeps

- The `Source`, for the life of the file. Every piece is parsed from it, and it can be parsed again.
- The script's answer: its tree and its bindings.
- One stack of bindings for the template's own scopes.
- The resolutions, which are its scope graph: the reactivity, the unused-variable warnings, the rename support all read that.
