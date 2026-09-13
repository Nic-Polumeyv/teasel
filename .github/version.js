// node .github/version.js: turns the changesets under package/.changeset into the next release.
// Bumps package.json and the crates, prepends the changelog section, deletes the changesets, and
// prints the pull request body. Prints nothing and changes nothing when there are no changesets.
import { readFileSync, writeFileSync, readdirSync, unlinkSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';

const root = new URL('../', import.meta.url);
const at = (path) => new URL(path, root);
const repo = process.env.GITHUB_REPOSITORY ?? 'Nic-Polumeyv/teasel';
const name = '@teasel/parser';
const dir = fileURLToPath(at('package/.changeset/'));

const changesets = readdirSync(dir)
	.filter((file) => file.endsWith('.md') && file !== 'README.md')
	.sort()
	.map((file) => {
		const text = readFileSync(join(dir, file), 'utf8');
		const match = /^---\r?\n([^]*?)\r?\n---\r?\n([^]*)$/.exec(text.trim());
		if (!match) throw new Error(`${file}: a changeset starts with a --- frontmatter naming the bump`);
		const bump = match[1].match(new RegExp(`^["']?${name}["']?:\\s*(major|minor|patch)\\s*$`, 'm'))?.[1];
		if (!bump) throw new Error(`${file}: the frontmatter must say "${name}": major | minor | patch`);
		return { file, bump, summary: match[2].trim() };
	});
if (changesets.length === 0) process.exit(0);

const order = ['major', 'minor', 'patch'];
const bump = order.find((level) => changesets.some((c) => c.bump === level));
const manifest = JSON.parse(readFileSync(at('package/package.json'), 'utf8'));
const [major, minor, patch] = manifest.version.split('.').map(Number);
const version = { major: `${major + 1}.0.0`, minor: `${major}.${minor + 1}.0`, patch: `${major}.${minor}.${patch + 1}` }[bump];

// the commit that added each changeset, and its pull request, the way changelog-github links them
const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();
const api = (path) => {
	try {
		return JSON.parse(execFileSync('gh', ['api', path], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }));
	} catch {
		return [];
	}
};
const line = ({ file, summary }) => {
	const sha = git('log', '--diff-filter=A', '--format=%H', '-1', '--', `package/.changeset/${file}`);
	const [first, ...rest] = summary.split('\n');
	const body = [first, ...rest.map((l) => `  ${l}`)].join('\n');
	if (!sha) return `- ${body}`;
	const commit = `[\`${sha.slice(0, 7)}\`](https://github.com/${repo}/commit/${sha})`;
	const [pull] = api(`repos/${repo}/commits/${sha}/pulls`);
	if (!pull) return `- ${commit} - ${body}`;
	return `- [#${pull.number}](${pull.html_url}) ${commit} Thanks [@${pull.user.login}](${pull.user.html_url})! - ${body}`;
};
const sections = order
	.map((level) => [level, changesets.filter((c) => c.bump === level)])
	.filter(([, list]) => list.length)
	.map(([level, list]) => `### ${level[0].toUpperCase()}${level.slice(1)} Changes\n\n${list.map(line).join('\n\n')}`)
	.join('\n\n');

const changelog = at('package/CHANGELOG.md');
// a replacer function: a summary may contain `$&` or `$1`, which a replacement string would expand
writeFileSync(changelog, readFileSync(changelog, 'utf8').replace(`# ${name}\n`, () => `# ${name}\n\n## ${version}\n\n${sections}\n`));
manifest.version = version;
writeFileSync(at('package/package.json'), JSON.stringify(manifest, null, '\t') + '\n');
const patchFile = (path, from, to) => {
	const text = readFileSync(at(path), 'utf8');
	if (!from.test(text)) throw new Error(`${path}: nothing matches ${from}`);
	writeFileSync(at(path), text.replace(from, to));
};
for (const file of ['Cargo.toml', 'bindings/node/Cargo.toml', 'bindings/wasm/Cargo.toml']) {
	patchFile(file, /^version = "[^"]*"$/m, `version = "${version}"`);
}
for (const crate of ['teasel', 'teasel-node', 'teasel-wasm']) {
	patchFile('Cargo.lock', new RegExp(`(\\[\\[package\\]\\]\\nname = "${crate}"\\nversion = ")[^"]*`), `$1${version}`);
}
for (const { file } of changesets) unlinkSync(join(dir, file));

process.stdout.write(
	`Merging this releases ${name}@${version}. Changesets that land on main meanwhile are added to it.\n\n# Releases\n\n## ${name}@${version}\n\n${sections}\n`,
);
