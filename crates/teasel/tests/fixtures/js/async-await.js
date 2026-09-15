async function f() {
	await g();
	const [a, b] = await Promise.all([h(), i()]);
	for await (const x of y) {}
	async () => { await 1; };
}
