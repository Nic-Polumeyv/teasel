abstract class A { abstract x: number; abstract get y(): string; abstract m(): void; }
class B extends A { override x = 1; override get y() { return ''; } override m() {} }
