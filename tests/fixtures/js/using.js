using resource = source;
resource;
await using asynchronous = source;
{
	using first = source, second = first;
	using using = source;
}
function f() {
	using local = source;
	return local;
}
class C {
	static {
		using local = source;
		local;
	}
}
switch (choice) {
	case 0: {
		using local = source;
		local;
	}
}
for (using item of items) { item; }
for (using item = source;;) { item; }
for (using of = source;;) { of; }
async function g() {
	await using local = source;
	local;
	for await (using item of items) { item; }
	for (await using item of items) { item; }
	for (await using of of items) { of; }
}
