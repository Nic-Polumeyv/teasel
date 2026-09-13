import { Marked } from 'marked';
import { buttonVariants } from 'sheer-ui/components/button';
import { snippet } from '#lib/highlight.ts';

// Docs as svelte.dev keeps them: content/<nn>-<section>/<nn>-<page>.md, the numbers order the
// sidebar and the frontmatter names the page. The first page is the site root.

export type Page = { href: string; title: string; section: string; body: string };
export type Section = { label: string; links: { href: string; title: string }[] };

const files = import.meta.glob('/content/**/*.md', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

const words = (name: string) => name.replace(/^\d+-/, '').replace(/-/g, ' ').replace(/^\w/, (c) => c.toUpperCase());

export const pages: Page[] = Object.keys(files)
	.sort()
	.map((path, index) => {
		const [, section, file] = /\/content\/([^/]+)\/([^/]+)\.md$/.exec(path)!;
		const text = files[path];
		const title = /^---\n(?:.*\n)*?title:\s*(.+)\n(?:.*\n)*?---\n/.exec(text)?.[1] ?? words(file);
		return { href: index === 0 ? '/' : `/${file.replace(/^\d+-/, '')}`, title, section: words(section), body: text.replace(/^---\n[\s\S]*?\n---\n/, '') };
	});

export const sections: Section[] = pages.reduce<Section[]>((sections, page) => {
	const link = { href: page.href, title: page.title };
	const last = sections.at(-1);
	if (last?.label === page.section) last.links.push(link);
	else sections.push({ label: page.section, links: [link] });
	return sections;
}, []);

const escape = (text: string) => text.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!);

const copy = buttonVariants({ variant: 'ghost', size: 'sm', class: 'absolute top-1.5 right-2 h-7 text-xs text-white/60 hover:bg-white/10 hover:text-white' });

const marked = new Marked({
	renderer: {
		code({ text, lang = '' }) {
			const [language, file] = lang.split(/\s+/);
			const { html } = snippet(text, language === 'ts' || language === 'typescript' ? 'typescript' : language === 'bash' || language === 'sh' ? 'bash' : 'javascript');
			return (
				`<div class="relative my-6 overflow-hidden rounded-lg border border-white/10 bg-[#2d353b] text-sm text-[#d3c6aa]">` +
				(file ? `<div class="flex h-10 items-center border-b border-white/10 px-4 font-mono text-xs text-white/60">${escape(file)}</div>` : '') +
				`<pre class="overflow-x-auto px-4 py-4 font-mono leading-6"><code>${html}</code></pre>` +
				`<button type="button" data-copy="${escape(text)}" class="${copy}${file ? '' : ' top-2'}">Copy</button></div>`
			);
		},
		heading({ tokens, depth, text }) {
			const id = text.toLowerCase().replace(/[^\w]+/g, '-').replace(/^-|-$/g, '');
			return `<h${depth} id="${id}">${this.parser.parseInline(tokens)}</h${depth}>`;
		},
	},
});

export const render = (page: Page) => marked.parse(page.body, { async: false });
