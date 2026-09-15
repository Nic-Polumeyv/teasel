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

`pos` and `end` span the token that broke things. When the problem is somewhere else, a declaration seen earlier for instance, `end` equals `pos`. `unexpected_eof` points at the end of what was parsed. Give the parser a bad offset and you get `invalid_request`, which has no `loc` because there's nothing to point at.

## Or keep going

With `errorRecovery` the parse comes back anyway, errors included, in source order. Wherever something is missing, the tree holds an `Identifier` named `''` with no width, so the shape is still the shape and your walker doesn't have to care.

```text
x = ;
    ▲
    errors  [{ code: 'unexpected_token', pos: 4, end: 5, loc }]
    right   Identifier ''  at 4..4
```

```js
const { node, errors } = new Source('x = ;', { errorRecovery: true }).parse();
errors[0].code;                            // 'unexpected_token'
node.body[0].expression.right.name;        // ''
```

A statement that can't be read at all gets skipped to the next one. `f(a, ` on its own comes back as an empty program with one `unexpected_eof`.

Recovery is for editors and language servers, where a half-typed file still needs a tree right now. A compiler that has to reject the file should leave it off and catch the throw.
