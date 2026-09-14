import { Marked, type Token, type Tokens } from 'marked';
import { buttonVariants } from 'sheer-ui/components/button';
import { snippet } from '#lib/highlight.ts';
import { reference } from '#lib/reference.ts';

export type Heading = { id: string; text: string };
export type Page = { href: string; title: string; section: string; path: string; headings: Heading[] };
export type Section = { label: string; pages: Page[] };

export const slug = (text: string) => text.toLowerCase().replace(/[^\w]+/g, '-').replace(/^-|-$/g, '');

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
	.map((path, index) => {
		const [, section, file] = /\/content\/([^/]+)\/([^/]+)\.md$/.exec(path)!;
		const [, front = '', markdown = files[path]] = /^---\n([\s\S]*?)\n---\n?([\s\S]*)$/.exec(files[path]) ?? [];
		const title = /^title:\s*(.+)$/m.exec(front)?.[1] ?? label(file);
		return page({ href: index === 0 ? '/' : `/${name(file)}`, title, section: label(section), path: `docs${path}` }, markdown);
	});

export const pages: Page[] = [...written, page(reference.meta, reference.markdown)];

export const sections: Section[] = Object.entries(Object.groupBy(pages, (page) => page.section)).map(([label, pages]) => ({ label, pages: pages! }));
