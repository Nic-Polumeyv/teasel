abstract class A<T> implements B, C {
	private x: number = 1;
	protected readonly y?: string;
	public static z: T;
	declare w: number;
	override v!: string;
	constructor(private a: number, public readonly b?: string) { super(); }
	abstract m(): void;
	protected n<U>(u: U): U { return u; }
	get g(): number { return 1; }
	static #p: number;
}
