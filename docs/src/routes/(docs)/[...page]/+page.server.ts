import { error } from '@sveltejs/kit';
import { pages, render } from '#lib/content.ts';

export const entries = () => pages.map((page) => ({ page: page.href.slice(1) }));

export const load = ({ params }) => {
	const index = pages.findIndex((page) => page.href === `/${params.page}`);
	if (index < 0) error(404);
	const page = pages[index];
	const link = ({ href, title }: { href: string; title: string }) => ({ href, title });
	return {
		title: page.title,
		html: render(page),
		edit: `https://github.com/Nic-Polumeyv/teasel/edit/main/${page.path}`,
		prev: index > 0 ? link(pages[index - 1]) : null,
		next: index + 1 < pages.length ? link(pages[index + 1]) : null,
	};
};
