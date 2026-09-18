---
title: A component, piece by piece
---

This example reads a component file: a `<script>` block, then a template with JavaScript inside `{…}`. The parser reads JavaScript and does not know the template syntax. So your code finds each `{`, and asks the parser to read the JavaScript that starts there.

At the end, every name the template uses is matched to the declaration it refers to, in the script or in an `each` block. That match is what a compiler, a linter or a rename tool for the format is built on.

This page uses three things that [Inside a host](/inside-a-host) explains: parsing at an offset, [plans](/inside-a-host#what-to-ask-for), and [`until`](/inside-a-host#where-to-stop). Read that page first if they are new.

The component:

```svelte List.svelte
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

## 1. One Source for the whole file

```js read-component.js
import { Source, Plan, scopeOf } from '@teasel/parser';

const source = new Source(text, { sourceType: 'module', scopes: true });
```
```notes
Source :: Holds the text of the file. Every parse on this page reads from this one object, so every position in every answer is an offset into the file. See [Getting started](/getting-started#parse-it-again).
sourceType: 'module' :: Lets the script use `import` and `export`. Every option is in the [reference](/reference/parser#options).
scopes: true :: Makes each answer list the names it declares, `bindings`, and the names it uses, `references`. [Scopes](/scopes) explains both.
```

`text` is the component above, as a string.

## 2. Read the script block

Find where the script's text starts and ends, then parse that range as a program.

```js read-component.js
const open = text.indexOf('<script>') + '<script>'.length;
const close = text.indexOf('</script>');
const script = source.parse(Plan.program, [open, close]);

const topLevel = script.bindings.filter(
	(binding) => binding.scope === scopeOf(script.node),
);
```
```notes
Plan.program :: The plan says what to read. `Plan.program` reads statements, as a file would have them. The other plans are `Plan.expression`, `Plan.statement`, `Plan.pattern`, `Plan.params` and `Plan.typeParameters`: see [What to ask for](/inside-a-host#what-to-ask-for).
[open, close] :: A pair of offsets reads only the text between them. A single number, used below, starts there and reads as far as the plan goes.
script.bindings :: Every declaration in the script, one entry each: `{ name, kind, scope, node, declaration }`. [What a binding knows](/reference/parser#binding).
scopeOf(script.node) :: `scopeOf` gives the scope a node opens. `script.node` is the `Program`, which opens the script's outermost scope, so a binding whose `scope` is that one was declared at the top level. See [Three questions](/scopes#three-questions).
```

`script.bindings` has every declaration, including the parameters of the two functions:

```text
onMount:import  items:let  selected:let  query:let  shown:const  item:param  pick:function  item:param
```

The template can only see the top-level ones, so `topLevel` leaves the two `item` parameters out.

## 3. Read one expression, and match its names

Most braces hold one expression: `{query}`, `{i + 1}`, `onclick={() => pick(item)}`. This function reads the expression that starts at `from`, stops at one of your tokens, and looks up every name the expression uses.

```js read-component.js
const inScope = [topLevel];
const resolved = [];

function expression(from, ...stops) {
	const answer = source.parse(Plan.expression.until(...stops), from);
	for (const reference of answer.references) {
		if (reference.binding !== null) continue;
		const name = reference.node.name;
		const found = inScope.flat().findLast((binding) => binding.name === name);
		resolved.push({ name, at: reference.node.start, found });
	}
	return answer;
}
```
```notes
inScope :: The declarations a template expression can see right now, as a stack of lists. It starts with the script's top-level bindings. Step 4 pushes what an `each` block declares, and pops it at `{/each}`.
Plan.expression.until(...stops) :: Reads one expression and ends it where one of `stops` follows, outside any bracket the expression opened. With `'}'` as the stop, `{i + 1}` ends at its brace, while `{ {a: 1}.a }` still reads whole. See [Where to stop](/inside-a-host#where-to-stop).
answer.references :: Every name the expression uses, as `{ node, binding, read, write, … }`. See [What a reference knows](/scopes#what-a-reference-knows).
reference.binding !== null :: The parser resolved this name itself, to something the expression declared, like the parameter in `(x) => x + 1`. Those need nothing from you. See [Names in a piece](/inside-a-host#names-in-a-piece).
findLast :: The innermost declaration wins, as in JavaScript: an `each` block's `item` hides a top-level `item`.
```

The parser reads this expression alone. It has not seen the script, so it cannot know that `query` is declared there: for every such name, `reference.binding` is `null`. Looking those up is your code's job, because only your code knows the template's scoping rules. `found` stays `undefined` for a name that nothing declares, a global like `console`.

## 4. Read an each block's head

`{#each shown as item, i (item.id)}` has up to four pieces of JavaScript in it, and each one ends at a different token:

| piece | what it is | plan | ends at |
| --- | --- | --- | --- |
| `shown` | the list | `Plan.expression` | `as` |
| `item` | declares the item | `Plan.pattern` | `,` or `(` or `}` |
| `i` | declares the index, optional | `Plan.pattern` | `(` or `}` |
| `item.id` | the key, optional | `Plan.expression` | `)` |

Every answer has `end`, the offset where that parse stopped. That is where the next piece starts.

```js read-component.js
const skip = (i) => {
	while (text[i] === ' ' || text[i] === '\t') i += 1;
	return i;
};

function each(from) {
	const list = expression(from, 'as');
	const afterAs = skip(skip(list.end) + 'as'.length);
	const item = source.parse(Plan.pattern.until(',', '(', '}'), afterAs);
	const declared = [...item.bindings];
	let next = skip(item.end);
	if (text[next] === ',') {
		const index = source.parse(Plan.pattern.until('(', '}'), skip(next + 1));
		declared.push(...index.bindings);
		next = skip(index.end);
	}
	inScope.push(declared);
	if (text[next] === '(') next = expression(next + 1, ')').end;
	return next;
}
```
```notes
skip :: A helper of this example, not part of the package. Given an offset, it returns the offset of the next character that is not a space or a tab.
list.end :: Where the list expression stopped: just after `shown`, before the `as`. `end` is on every answer, see [The answer](/the-answer).
Plan.pattern :: Reads what can stand on the left of `=` in a declaration: a name, or a destructuring like `{ id, name }` or `[first, ...rest]`.
item.bindings :: What the pattern declares, one binding per name, with `kind: 'pattern'`. For `{ id, name }` that is two bindings.
inScope.push(declared) :: From here until `{/each}`, template expressions can see `item` and `i`.
expression(next + 1, ')') :: The key is an ordinary expression that ends at the closing parenthesis. It is read after the push, because the key may use `item`.
```

## 5. Walk the template

The loop finds each `{` after the script and picks what to read from the text that follows it.

```js read-component.js
let at = close;
while ((at = text.indexOf('{', at)) !== -1) {
	if (text.startsWith('{#each ', at)) at = each(at + '{#each '.length);
	else if (text.startsWith('{/each}', at)) { inScope.pop(); at += 1; }
	else if (text.startsWith('{#if ', at)) at = expression(at + '{#if '.length, '}').end;
	else if (text.startsWith('{:else}', at) || text.startsWith('{/if}', at)) at += 1;
	else at = expression(at + 1, '}').end;
}
```
```notes
inScope.pop() :: The block is over, so `item` and `i` are no longer visible.
at += 1 :: Nothing to parse in this tag. Moving one character on lets the search find the next `{`.
expression(at + 1, '}').end :: Any other brace holds one expression. The search continues from where that parse stopped.
```

These are the parses the loop asked for, in order, with the offset each started at:

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

## The result

`resolved` now has one entry per name the template uses, with the declaration it refers to:

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

Look at `pick(item)` at 475. It sits inside an arrow function, `() => pick(item)`, and the arrow declares no `item`, so the name came back unresolved and the lookup found the `each` block's. Had it been written `(item) => pick(item)`, the parser would have resolved `item` to the parameter itself, and it would not be in `resolved` at all.

## What to keep

- The `Source`, for as long as the file is open. Any piece can be parsed again from it. Release it when the file closes: see [Getting started](/getting-started#parse-it-again).
- `script`, the script's tree and bindings.
- `resolved`. A compiler reads it to know which template expressions depend on which declarations; a linter reads it to find a declaration nothing uses.

## Related

- [Inside a host](/inside-a-host) is the guide this example applies.
- [A document](/a-document) is the other way to do this: describe the template syntax to the parser once, and get the script and the template back as one tree.
- [Scopes](/scopes) has everything a binding and a reference carry.
