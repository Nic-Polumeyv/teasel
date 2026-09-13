import { writeFileSync } from 'node:fs';
import { svelte } from './svelte';
import { vue } from './vue';

for (const [name, plan] of Object.entries({ svelte, vue })) {
  writeFileSync(new URL(`./${name}.json`, import.meta.url), JSON.stringify(plan, null, 2) + '\n');
}
