import { error } from '@sveltejs/kit';
import { pages, render } from '#lib/content.ts';

export const entries = () => pages.map((page) => ({ page: page.href.slice(1) }));

export const load = ({ params }) => {
	const page = pages.find((page) => page.href === `/${params.page}`);
	if (!page) error(404);
	return { title: page.title, html: render(page) };
};
