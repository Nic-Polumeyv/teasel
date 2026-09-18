// `node scripts/codes.ts` writes src/lib/codes.ts from the codes of crates/teasel/src/error.rs; the tests check they agree
import { readFileSync, writeFileSync } from 'node:fs';

export const codes = (): string[] => [...readFileSync(new URL('../../crates/teasel/src/error.rs', import.meta.url), 'utf8').matchAll(/^\t[A-Z][A-Za-z0-9]* "([a-z_0-9]+)" =>/gm)].map((m) => m[1]);

if (process.argv[1] === new URL(import.meta.url).pathname) {
	const list = codes();
	writeFileSync(new URL('../src/lib/codes.ts', import.meta.url), `// written by scripts/codes.ts from crates/teasel/src/error.rs\n/** What a parse error names in \`code\`: every code the engine can report. */\nexport type Code =\n${list.map((code) => `\t| '${code}'`).join('\n')};\n`);
	console.log(`${list.length} codes`);
}
