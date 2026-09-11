using resource: T = source;
await using asynchronous: T = source;
namespace N {
	using local: T = source;
	local;
}
for (using x!: T of items) { x; }
for (using of: T = source;;) { of; }
