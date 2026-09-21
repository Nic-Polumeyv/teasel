---
title: Host grammar
---

The text `new Plan(grammar)` takes: what a template language puts around the JavaScript it embeds. One statement per line, words separated by spaces; `//` starts a comment; a line indented under a `block` belongs to it. Every name a statement gives becomes the `type` of a node or the name of a field, exactly as written. The examples come from the two grammars in the parser's tests. [Parsing with a grammar](/parsing-with-a-grammar) has a third, complete one.

## The document

```text
host svelte
document Root  css=style  js=list  options=null  comments=comments  module?=script:module  { instance?=script  { fragment=fragment } }
```

`host` names the language; it must come first. `document` names the root node's type and lists its fields, each `field=holds` where `holds` is one of:

| holds | the field gets |
| --- | --- |
| `fragment` | the document's content |
| `script` | the script block, `script:module` the module one |
| `style` | the style block |
| `comments` | every comment of the document, host and JavaScript |
| `list` | an empty list |
| `null` | null |

`field?=` leaves the field out of the node when nothing filled it; without `?` it is null. Braces nest scopes: in the line above the instance script's scope sits inside the module script's, and the template's inside the instance's, so the template resolves names the scripts declare.

## Text and expressions

```text
delimiters {{ }}
sigils open=# branch=: close=/ tag=@
attributes expressions shorthand
autoclose
trim
void area base br col embed hr img input link meta param source track wbr
verbatim v-pre
```

- `delimiters` are what opens and closes an expression in text, `{` and `}` by default. This line keeps its words whole: `{{` is one delimiter.
- `sigils` are the characters after the opening delimiter that start a block (`{#if}`), a branch (`{:else}`), a close (`{/if}`) and a tag (`{@html}`). Without them a grammar has no blocks or tags.
- `attributes expressions` lets attribute values hold expressions between the delimiters, as text does; `attributes shorthand` makes `{name}` among the attributes mean `name={name}`.
- `autoclose` closes an element the browser would close when another opens, a `<p>` before a `<div>`.
- `trim` leaves whitespace at the end of the source out of the document.
- `void` lists the elements that have no closing tag.
- `verbatim` names the attribute that makes an element's subtree verbatim: text and plain attributes only, no expressions, no directives.

## Nodes

```text
fragment Fragment nodes scope
elements name=name attributes=attributes children=fragment
text Text data=data raw=raw
comment Comment data=data
```

- `fragment` wraps every list of children in a node of the given type, with the list in the given field; `scope` opens a scope on each. Without it a list of children is a plain array.
- `elements` names the fields every element gets: its name, its attributes and its children.
- `text` and `comment` name the type of a text node and a comment node and the field with the text; `raw=` gives a text node a second field with the text before entities are decoded.

## Elements

```text
element svelte:element    SvelteElement    this=tag:text
element svelte:window     SvelteWindow     root once
element title             TitleElement     inside svelte:head
element slot              SlotElement      outside shadowrootmode
element textarea          RegularElement   rcdata
element script            RegularElement   raw
element component-name    Component
element *                 RegularElement
```

`element NAME TYPE flags`: an element with this name gets this type. `component-name` matches a capitalized or dotted name, `*` everything else; rules are tried in order, first match wins. The flags:

| flag | meaning |
| --- | --- |
| `root` | allowed at the top level only |
| `once` | allowed once per document |
| `raw` | the content is text up to the closing tag, a script's or style's |
| `rcdata` | the content is text with the host's expressions in it, a textarea's |
| `inside NAME` | allowed only inside an element named NAME |
| `outside ATTR` | not allowed inside an element carrying the attribute ATTR |
| `this=FIELD` | the `this` attribute's expression goes to FIELD and leaves the attributes; `this=FIELD:text` accepts plain text there too, as a string |

## Scripts and styles

```text
script script  module=context:module  module=module  typescript=lang:ts
style  style
```

`script` names the element that holds JavaScript at the top level, and which attributes make it the module script (`context="module"` or a bare `module`) and which turn TypeScript on for the whole document (`lang="ts"`). The script's content is parsed as a program and lands in the document field that holds `script`. `style` names the element whose content is CSS; the parser reads it into a stylesheet tree in the field that holds `style`.

## Attributes and directives

```text
directives arg=: modifier=| field:arg=name field:modifiers=modifiers
directive bind        BindDirective        expression?name  unique:attribute
directive on          OnDirective          expression?
directive let         LetDirective         pattern?name  declares
directive transition  TransitionDirective  expression?  intro outro
directive in          TransitionDirective  expression?  intro !outro
spread  SpreadAttribute  expression
```

```text
directives prefix=v- arg=: modifier=. dynamic=[] field:name=name field:arg=arg field:modifiers=modifiers field:raw=rawName unique=raw
shorthand :  bind
shorthand @  on
shorthand .  bind  .prop
directive for   Directive  [ ( [ value?=pattern ] [ , [ key?=pattern ] [ , [ index?=pattern ] ] ] ) | [ value?=pattern ] [ , [ key?=pattern ] [ , [ index?=pattern ] ] ] ] { in | of } source=expression  declares value key index
directive on    Directive  handler?=code
directive *     Directive  exp?=expression
```

`directives` says how a directive's attribute name is spelled, `prefix name arg modifiers`, and keeps its words whole:

| key | meaning |
| --- | --- |
| `prefix=` | what every directive name starts with, `v-`; without it the directive's own name is the start, `bind:` |
| `arg=` | what separates the argument, `:` |
| `modifier=` | what separates the modifiers, `\|` or `.` |
| `dynamic=` | two characters bracketing an argument that is an expression, `[]` |
| `field:name=`, `field:arg=`, `field:modifiers=`, `field:raw=` | the node fields for the directive's name, argument, modifier list and whole attribute name |
| `unique=raw` | no two directives on an element may share a whole attribute name |

`shorthand CHAR NAME [.modifier]` makes a leading character stand for a prefixed directive, `:x` for `v-bind:x`, with modifiers it implies.

`directive NAME TYPE VALUE flags`: a directive with this name gets this type; `*` matches every other. The value is one of:

| value | the attribute value is |
| --- | --- |
| `expression` | one expression; `expression?` may be absent; `expression?name` takes the argument as the expression when absent, `bind:value` |
| `pattern` | one binding pattern, likewise with `?` and `?name` |
| `value` | kept as it is, text and expressions |
| a form | read by the form, `v-for="item in items"`; see below |

The flags after the value: a bare word is a boolean field set to true, `!word` one set to false; `unique` forbids two directives of this kind with the same argument on an element, `unique:attribute` counts plain attributes with that name too; `declares` makes what the value binds visible in the element's scope, `declares a b` the named fields of the form.

`spread TYPE FIELD` names the node of a `{...expression}` attribute and the field holding the expression.

## Blocks

```text
block if  IfBlock  chain=elseif
  open    test=expression  -> consequent
  branch  else if test=expression  -> alternate chain consequent
  branch  else  -> alternate

block each  EachBlock
  open    expression=expression [ as context=pattern ] [ , index?=identifier ] [ ( key?=expression ) ]  -> body declares context index
  branch  else  -> fallback?

block await  AwaitBlock
  open    expression=expression [ then [ value=pattern ] -> then declares value | catch [ error=pattern ] -> catch declares error ]  -> pending
  branch  then [ value=pattern ]  -> then declares value
  branch  catch [ error=pattern ]  -> catch declares error

block snippet  SnippetBlock
  open    expression=identifier [ typeParams?=typeParameters ] parameters=params  -> body declares expression:outside parameters
```

`block NAME TYPE` opens with `{#NAME …}`, closes with `{/NAME}`, and gets this type. `chain=FIELD` names a boolean field set on a block that a chained branch opened. Indented under it:

- `open FORM -> BODY` reads the opening tag by the form; the body is where the content goes.
- `branch WORDS FORM -> BODY` reads `{:WORDS …}`, the longest run of words first (`else if` before `else`), and starts a new body. A branch with `chain FIELD` nests a new block of the same kind into the field instead, so `{:else if}` becomes an `IfBlock` inside `alternate`.

A body is `-> field`, or `-> field?` to leave the field out of blocks that never opened it, then optionally `chain FIELD` and `declares a b`: the fields of the form whose patterns are declared in the body's scope. `declares name:outside` declares in the scope around the block instead, a snippet's name. A name declared twice in a scope the host opens is a `redeclaration` error of the scope analysis: only a parse with `scopes` reports it. A `params` list that repeats a name is `duplicate_parameter` either way.

## Tags

```text
tag html    HtmlTag    expression=expression
tag const   ConstTag   declaration=const
tag attach  AttachTag  expression=expression  attribute
declaration  DeclarationTag  declaration=statement
expression   ExpressionTag   expression=expression
```

`tag NAME TYPE FORM` reads `{@NAME …}`; `attribute` at the end lets the tag stand among an element's attributes. `declaration TYPE FORM` reads a `{…}` in text that starts with `let`, `const` or `type` as one declaration statement, into the form's field; `var`, `interface` and `enum` are refused there. `expression TYPE FORM` reads any other `{…}`.

## Forms

A form is what a tag or an opening reads between the delimiters: literals, entries and groups in order.

- A literal is a word of the host's own, `as`, `in`, `(`.
- An entry is `field=kind`, a piece of JavaScript read into the field. `field?=kind` leaves the field out when the entry was not read; the plain form gives it null. The entry ends where the next literal of the form, or the closing delimiter, follows.
- When several optional groups follow an `expression`, they are tried in the order the form writes them. A literal of a later group ends the expression only when no literal of an earlier group follows it. With `[ as context=pattern ] [ , index?=identifier ]`, the comma in `a, b as x` belongs to the expression, and the comma in `items, i` starts the index.
- `[ a | b ]` reads at most one alternative, tried in order; `{ a | b }` exactly one. An alternative may end in its own `-> body`, as `await`'s does.

The kinds of entry:

| kind | reads |
| --- | --- |
| `expression` | an expression |
| `pattern` | an assignment target: an identifier or a destructuring pattern |
| `params` | a parenthesized parameter list |
| `identifier` | one identifier |
| `identifiers` | identifiers separated by commas, possibly none |
| `typeParameters` | a `<T extends U>` list, TypeScript |
| `statement` | one statement |
| `const` | `pattern = expression`, a const declaration the host spells without the keyword |
| `code` | an expression, or when what holds it is not one, its statements as a program |
| `text` | the text up to the closing delimiter, unread, for a host that parses it later |
