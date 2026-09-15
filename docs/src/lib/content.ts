import { Marked, type Token, type Tokens } from 'marked';
import type { Component } from 'svelte';
import { render as ssr } from 'svelte/server';
import { buttonVariants } from 'sheer-ui/components/button';
import { snippet } from '#lib/highlight.ts';
import { reference } from '#lib/reference.ts';

export type Heading = { id: string; text: string };
export type Page = { href: string; title: string; section: string; path: string; headings: Heading[] };
export type Section = { label: string; pages: Page[] };

export const slug = (text: string) => text.toLowerCase().replace(/[^\w]+/g, '-').replace(/^-|-$/g, '');

const escape = (text: string) => text.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!);

const copy = buttonVariants({ variant: 'ghost', size: 'sm', class: 'absolute top-1.5 right-2 h-7 text-xs text-white/60 hover:bg-white/10 hover:text-white' });

const sources = Object.fromEntries(Object.entries(import.meta.glob('/content/**/*.js', { query: '?raw', import: 'default', eager: true })).map(([path, text]) => [path.slice(path.lastIndexOf('/') + 1), text as string]));

/** A whole file the page is about, shown as the sheet it is: a name tab, every line numbered, scrolling past a screen's worth. */
function sheet(name: string) {
	const text = sources[name];
	if (text === undefined) throw new Error(`no ${name} under content/`);
	const lines = snippet(text.replace(/\n$/, ''), 'javascript').html.split('\n');
	return (
		`<figure class="my-8 overflow-hidden rounded-sm border border-white/10 bg-[#2d353b] text-[#d3c6aa] shadow-lg">` +
		`<figcaption class="flex items-center gap-2 border-b border-white/10 bg-white/5 px-4 py-2 font-mono text-xs text-white/70"><svg viewBox="0 0 16 16" class="size-3.5" fill="none" stroke="currentColor" stroke-width="1.2"><path d="M4 1.5h5l3 3v10H4z M9 1.5v3h3"/></svg>${escape(name)}<span class="ml-auto text-white/40">${lines.length} lines</span></figcaption>` +
		`<pre class="max-h-[38rem] overflow-auto py-4 font-mono text-[13px] leading-6"><code>${lines.map((line, i) => `<span class="inline-block w-12 select-none pr-4 text-right text-white/30">${i + 1}</span>${line}`).join('\n')}</code></pre></figure>`
	);
}

const diagrams = Object.fromEntries(Object.entries(import.meta.glob('/content/**/*.svelte', { import: 'default', eager: true })).map(([path, diagram]) => [path.slice(path.lastIndexOf('/') + 1), diagram as Component<{ label: string }>]));

export function fence(text: string, language = 'js', file?: string) {
	const html = language === 'text' ? escape(text) : snippet(text, language === 'ts' || language === 'typescript' ? 'typescript' : language === 'bash' || language === 'sh' ? 'bash' : 'javascript').html;
	return (
		`<div class="relative my-6 overflow-hidden rounded-lg border border-white/10 bg-[#2d353b] text-sm text-[#d3c6aa]">` +
		(file ? `<div class="flex h-10 items-center border-b border-white/10 px-4 font-mono text-xs text-white/60">${escape(file)}</div>` : '') +
		`<pre class="overflow-x-auto px-4 py-4 font-mono leading-6"><code>${html}</code></pre>` +
		`<button type="button" data-copy="${escape(text)}" class="${copy}${file ? '' : ' top-2'}">Copy</button></div>`
	);
}

const marked = new Marked({
	renderer: {
		image({ href, text }) {
			const diagram = diagrams[href];
			return diagram ? ssr(diagram, { props: { label: text } }).body : `<img src="${href}" alt="${escape(text)}">`;
		},
		code({ text, lang = '' }) {
			const [language, file] = lang.split(/\s+/);
			return language === 'file' ? sheet(file) : fence(text, language, file);
		},
		heading({ tokens, depth, text }) {
			return `<h${depth} id="${slug(text)}">${this.parser.parseInline(tokens)}</h${depth}>`;
		},
	},
});

const tokens = new Map<string, Token[]>();

function page(meta: Omit<Page, 'headings'>, markdown: string): Page {
	const lexed = marked.lexer(markdown);
	tokens.set(meta.href, lexed);
	const headings = lexed.filter((token): token is Tokens.Heading => token.type === 'heading' && token.depth === 2).map(({ text }) => ({ id: slug(text), text }));
	return { ...meta, headings };
}

export const render = (page: Page) => marked.parser(tokens.get(page.href)!);

const name = (segment: string) => segment.replace(/^\d+-/, '');
const label = (segment: string) => name(segment).replace(/-/g, ' ').replace(/^\w/, (c) => c.toUpperCase());

const files = import.meta.glob('/content/**/*.md', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

const written = Object.keys(files)
	.sort()
	.map((path) => {
		const [, section, file] = /\/content\/([^/]+)\/([^/]+)\.md$/.exec(path)!;
		const [, front = '', markdown = files[path]] = /^---\n([\s\S]*?)\n---\n?([\s\S]*)$/.exec(files[path]) ?? [];
		const title = /^title:\s*(.+)$/m.exec(front)?.[1] ?? label(file);
		return page({ href: `/${name(file)}`, title, section: label(section), path: `docs${path}` }, markdown);
	});

export const pages: Page[] = [...written, page(reference.meta, reference.markdown)];

export const sections: Section[] = Object.entries(Object.groupBy(pages, (page) => page.section)).map(([label, pages]) => ({ label, pages: pages! }));
