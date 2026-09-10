@dec
@dec.with(1)
class A {
	@prop x: number;
	@method m(@param p: string) {}
	@acc accessor y = 1;
}
export @dec class B {}
@dec export class C {}
