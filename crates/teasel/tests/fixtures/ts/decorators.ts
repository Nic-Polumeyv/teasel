@dec
@dec.with(1)
class A {
	@prop x: number;
	@method m(@param p: string) {}
	@acc accessor y = 1;
}
export @dec class B {}
@dec export class C {}
const D = (@dec class {});
export default @dec class {
	constructor(@param x: number) {}
	set x(@param value: number) {}
}
@dec abstract class E {
	@dec #x = 1;
	@dec declare x: number;
	@dec abstract y: number;
	@dec static m() {}
	@dec ["computed"]() {}
}
