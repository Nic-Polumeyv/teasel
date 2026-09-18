---
title: TypeScript
---

With `typescript: true` the parser reads TypeScript. The tree has TypeScript nodes such as `TSTypeAnnotation` and `TSInterfaceDeclaration`, in the TS-ESTree shape.

```js typescript.js
const { node } = new Source('let n: number = 1', { typescript: true }).parse();
node.body[0].declarations[0].id.typeAnnotation.type; // 'TSTypeAnnotation'
```

## Erase the types

`typescript: 'erase'` reads TypeScript and returns JavaScript, with the types removed, in the same pass. Positions are positions in the text you gave it.

```js typescript.js
const { node, typescript } = new Source('let n: number = 1', { typescript: 'erase' }).parse();
node.body[0].declarations[0].id.typeAnnotation; // undefined
typescript;                                     // []
```

Some TypeScript syntax does something at runtime: enums, namespaces that hold values, parameter properties, `export =`, `import =`, decorators and accessor fields. Removing those would change the program, so they stay in the tree, and the answer's `typescript` lists each one by type and span. An empty list means the tree is plain JavaScript.

## Decorators

Decorators are read when `typescript` is on. Both syntaxes are accepted by default. `decorators: 'legacy'` accepts only what `experimentalDecorators` allows, and `decorators: 'proposal'` accepts only the standard syntax.
