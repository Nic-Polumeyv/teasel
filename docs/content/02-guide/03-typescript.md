---
title: TypeScript
---

Say `typescript: true` and the parser reads TypeScript. The tree holds TypeScript nodes, `TSTypeAnnotation`, `TSInterfaceDeclaration` and friends, in the TS-ESTree shape.

```js typescript.js
const { node } = new Source('let n: number = 1', { typescript: true }).parse();
node.body[0].declarations[0].id.typeAnnotation.type; // 'TSTypeAnnotation'
```

## Or erase it

`typescript: 'erase'` reads the TypeScript and answers with JavaScript, types gone, in the one pass. Positions are still positions in the text you gave it.

```js typescript.js
const { node, typescript } = new Source('let n: number = 1', { typescript: 'erase' }).parse();
node.body[0].declarations[0].id.typeAnnotation; // undefined
typescript;                                     // []
```

Some TypeScript isn't just types. Enums, namespaces with values in them, parameter properties, `export =`, `import =`, decorators and accessor fields all do something at runtime, and erasing them would change the program. So they stay, and the answer's `typescript` table lists each one by type and span. An empty table means what you got is plain JavaScript.

## Decorators

Decorators are read when `typescript` is on. Both syntaxes are accepted unless you pick one: `decorators: 'legacy'` is what `experimentalDecorators` allows, `'proposal'` is the standard syntax and nothing else.
