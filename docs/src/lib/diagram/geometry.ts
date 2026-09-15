export type Rect = { x: number; y: number; w: number; h: number };
export type Side = 'top' | 'right' | 'bottom' | 'left';
export type Anchor = { x: number; y: number; dx: number; dy: number };

const outward: Record<Side, [number, number]> = { top: [0, -1], right: [1, 0], bottom: [0, 1], left: [-1, 0] };

export const center = (rect: Rect) => ({ x: rect.x + rect.w / 2, y: rect.y + rect.h / 2 });

export function anchor(rect: Rect, side: Side, at = 0.5): Anchor {
	const [dx, dy] = outward[side];
	return {
		x: dx === 0 ? rect.x + rect.w * at : rect.x + (dx > 0 ? rect.w : 0),
		y: dy === 0 ? rect.y + rect.h * at : rect.y + (dy > 0 ? rect.h : 0),
		dx,
		dy,
	};
}

function facing(from: Rect, to: Rect): [Side, Side] {
	const a = center(from);
	const b = center(to);
	const dx = b.x - a.x;
	const dy = b.y - a.y;
	if (Math.abs(dx) >= Math.abs(dy)) return dx >= 0 ? ['right', 'left'] : ['left', 'right'];
	return dy >= 0 ? ['bottom', 'top'] : ['top', 'bottom'];
}

const HEAD = 9;
const HALF = 5;
const GAP = 1.5;

export function arrow(from: Rect | Anchor, to: Rect | Anchor, bend?: number) {
	const auto = facing('dx' in from ? { x: from.x, y: from.y, w: 0, h: 0 } : from, 'dx' in to ? { x: to.x, y: to.y, w: 0, h: 0 } : to);
	const a = 'dx' in from ? from : anchor(from, auto[0]);
	const at = 'dx' in to ? to : anchor(to, auto[1]);
	const tip = { x: at.x + at.dx * GAP, y: at.y + at.dy * GAP };
	const k = bend ?? Math.min(80, Math.max(20, Math.hypot(tip.x - a.x, tip.y - a.y) / 2));
	const back = { x: tip.x + at.dx * HEAD, y: tip.y + at.dy * HEAD };
	const px = -at.dy * HALF;
	const py = at.dx * HALF;
	const p = (x: number, y: number) => `${Math.round(x * 10) / 10} ${Math.round(y * 10) / 10}`;
	return `M${p(a.x, a.y)} C${p(a.x + a.dx * k, a.y + a.dy * k)}, ${p(tip.x + at.dx * k, tip.y + at.dy * k)}, ${p(tip.x, tip.y)} M${p(back.x + px, back.y + py)} L${p(tip.x, tip.y)} L${p(back.x - px, back.y - py)}`;
}
