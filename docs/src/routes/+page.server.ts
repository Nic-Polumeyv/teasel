import { fence } from '#lib/content.ts';

const facts = `import { Source, bindingOf } from '@teasel/parser';

const { node, scopes } = new Source('let x = 1; x++', { scopes: true }).parse();

node.type;                                   // 'Program', as acorn would say
bindingOf(node.body[0].declarations[0].id);  // { name: 'x', kind: 'let', scope, … }
scopes.length;                               // 1`;

export const load = () => ({ install: fence('npm install @teasel/parser', 'bash'), facts: fence(facts, 'js') });
