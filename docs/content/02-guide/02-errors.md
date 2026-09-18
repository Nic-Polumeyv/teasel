---
title: Errors
---

A syntax error throws a `SyntaxError`. It has a `code` to branch on and a position.

```js errors.js
try {
	new Source('x = ;').parse();
} catch (e) {
	e.code;    // 'unexpected_token'
	e.pos;     // 4
	e.end;     // 5
	e.loc;     // { line: 1, column: 4 }
	e.message; // 'Unexpected token'
}
```

## Error recovery

An editor or a language server needs a tree from a file that does not parse, because it runs while someone is typing. With `errorRecovery` on, the parse returns the tree and the errors together and does not throw.

```js errors.js
const { node, errors } = new Source('x = ;', { errorRecovery: true }).parse();
errors[0].code;                            // 'unexpected_token'
node.body[0].expression.right.name;        // ''
```

Where something is missing, the tree has an `Identifier` named `''` that is zero characters wide. Every node still has the fields its type has, so code that walks the tree needs no special case.

```text
x = ;
    ▲
    errors  [{ code: 'unexpected_token', pos: 4, end: 5, loc }]
    right   Identifier ''  at 4..4
```

A statement that cannot be read at all is skipped, and the parse continues at the next one. `f(a, ` alone returns an empty program with one `unexpected_eof`.

A compiler that must reject the file leaves recovery off and catches the throw.

## Error positions

`pos` and `end` span the token the parser could not accept. When the cause is elsewhere, an earlier declaration of the same name for instance, `end` equals `pos`. `unexpected_eof` points at the end of what was parsed. An offset you pass that is outside the text is `invalid_request`, with no `loc`. Every code is in the type [`Code`](/reference/parser#code), so a comparison against one is type-checked.
