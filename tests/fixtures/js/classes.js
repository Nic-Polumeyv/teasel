class A extends B {
	static s = 1;
	#p = 2;
	[computed] = 3;
	constructor() { super(); }
	get g() { return this.#p; }
	set g(v) {}
	static { A.s++; }
	async *m() {}
	static async #q() {}
	accessor a = 1;
}
const C = class {};
#p in obj;
