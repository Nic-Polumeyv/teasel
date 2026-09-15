---
title: TypeScript
---

Turn `typescript` on to read TypeScript; the tree then holds the TypeScript nodes, `TSTypeAnnotation`, `TSInterfaceDeclaration` and the rest, as typescript-eslint spells them.

```js
const { node } = new Source('let n: number = 1', { typescript: true }).parse();
node.body[0].declarations[0].id.typeAnnotation.type; // 'TSTypeAnnotation'
```

## Erasing

`typescript: 'erase'` reads TypeScript and answers with JavaScript, the types gone, in the same pass. Positions stay those of the original text.

```js
const { node, typescript } = new Source('let n: number = 1', { typescript: 'erase' }).parse();
node.body[0].declarations[0].id.typeAnnotation; // undefined
typescript;                                     // []
```

Erasure cannot remove what has a runtime meaning. The answer's `typescript` table lists what was left in place, by type and span: enums, namespaces with values, parameter properties, `export =`, `import =`, decorators and accessor fields. An empty table means the output is plain JavaScript.

## Decorators

Both decorator syntaxes are read by default. `decorators: 'legacy'` or `'proposal'` restricts the parse to one.
