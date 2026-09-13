import { writeFileSync } from 'node:fs';
import { svelte } from './svelte';
import { vue } from './vue';

// one rule per line keeps the generated file diffable
const print = (plan: object) => {
  const { rules, ...rest } = plan as { rules: Record<string, unknown> };
  const lines = Object.entries(rules).map(([name, rule]) => `  ${JSON.stringify(name)}: ${JSON.stringify(rule)}`);
  const top = Object.entries(rest).map(([key, value]) => `${JSON.stringify(key)}: ${JSON.stringify(value)}`);
  return `{${top.slice(0, 2).join(', ')}, "rules": {\n${lines.join(',\n')}\n}, ${top.slice(2).join(', ')}}\n`;
};
for (const [name, plan] of Object.entries({ svelte, vue })) {
  writeFileSync(new URL(`./${name}.json`, import.meta.url), print(plan));
}
