import { fence } from '#lib/content.ts';

const facts = `import { parse, referenceOf } from '@teasel/parser';

const { node, scopes } = parse('let x = 1; x++', { scopes: true });

node.type;                                   // 'Program'
referenceOf(node.body[0].declarations[0].id).binding;  // { name: 'x', kind: 'let', scope, … }
scopes.length;                               // 1`;

export const load = () => ({ install: fence('npm install @teasel/parser', 'bash'), facts: fence(facts, 'js') });
