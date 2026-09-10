interface A<T = string> extends B, C<T> {
	x: number;
	readonly y?: string;
	m(a: T): void;
	new (b: number): A<T>;
	(c: string): void;
	[key: string]: unknown;
	get g(): number;
	set g(v: number);
}
