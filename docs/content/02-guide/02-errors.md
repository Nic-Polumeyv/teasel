---
title: Errors
---

A syntax error throws a `SyntaxError` with a code to branch on and a position to point at.

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

`pos` and `end` span the token being read. An error reported elsewhere, at a declaration seen earlier say, has `end` equal to `pos`. `unexpected_eof` points at the end of what was parsed. A bad offset from the caller is an `invalid_request` without a `loc`.

## Recovering

With `errorRecovery`, the parse comes back and the errors come with it, in source order. Where something is missing, the tree holds an `Identifier` named `''` of no width, so the shape stays walkable.

```text
{ f(a, }
       ▲
       errors  [{ code: 'unexpected_token', pos: 7, end: 7, loc }]
       node    Identifier ''  at 7..7
       end     7
```

```js
const { node, errors } = new Source('f(a, ', { errorRecovery: true }).parse();
errors.length;                                   // 1
node.body[0].expression.arguments[1].name;       // ''
```

Use recovery for an editor or a language server, where a half-typed file still needs a tree. A compiler that must reject the file leaves it off and catches the throw.
