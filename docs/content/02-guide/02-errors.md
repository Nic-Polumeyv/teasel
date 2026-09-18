---
title: Errors
---

A syntax error throws a `SyntaxError`. It has a code you can branch on and a position you can point at.

```js
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

## Or keep going

Sometimes a broken file still needs a tree: an editor, a language server, anything that runs while someone is typing. Turn on `errorRecovery` and the parse comes back with the tree and the errors together.

```js
const { node, errors } = new Source('x = ;', { errorRecovery: true }).parse();
errors[0].code;                            // 'unexpected_token'
node.body[0].expression.right.name;        // ''
```

Wherever something is missing, the tree holds an `Identifier` named `''` with no width. The shape is still the shape, and a walker doesn't have to care.

```text
x = ;
    ▲
    errors  [{ code: 'unexpected_token', pos: 4, end: 5, loc }]
    right   Identifier ''  at 4..4
```

A statement that can't be read at all is skipped to the next one. `f(a, ` on its own comes back as an empty program with one `unexpected_eof`.

A compiler that has to reject the file should leave recovery off and catch the throw.

## Where an error points

`pos` and `end` span the token that broke things. When the problem is somewhere else, a declaration seen earlier for instance, `end` equals `pos`. `unexpected_eof` points at the end of what was parsed. A bad offset from your side is `invalid_request`, with no `loc`, because there's nothing in the text to point at. Every code is listed as the type [`Code`](/reference/parser#code), so a comparison against one is checked.
