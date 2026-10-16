import { hue } from "../lib/format";
import { cx } from "./ui";

/**
 * Avatar gerado a partir do nick: um mosaico 5x5 simétrico com cores do próprio nome
 * (identidade própria; nada de skins ou texturas do jogo).
 */
export function Avatar({ name, size = 36, ring, className }: { name: string; size?: number; ring?: boolean; className?: string }) {
  const h = hue(name);
  let seed = 0;
  for (const c of name.toLowerCase()) seed = (seed * 131 + c.charCodeAt(0)) >>> 0;
  const cells: boolean[] = [];
  for (let i = 0; i < 15; i++) {
    seed = (seed * 1103515245 + 12345) >>> 0;
    cells.push(((seed >>> 16) & 1) === 1);
  }
  const fg = `hsl(${h} 62% 62%)`;
  const fg2 = `hsl(${(h + 40) % 360} 70% 70%)`;
  const bg = `hsl(${h} 35% 16%)`;
  const rects = [];
  for (let y = 0; y < 5; y++) {
    for (let x = 0; x < 3; x++) {
      if (!cells[y * 3 + x]) continue;
      const fill = (x + y) % 3 === 0 ? fg2 : fg;
      rects.push(<rect key={`${x}-${y}`} x={x + 1} y={y + 1} width="1" height="1" fill={fill} />);
      if (x < 2) rects.push(<rect key={`m${x}-${y}`} x={5 - x} y={y + 1} width="1" height="1" fill={fill} />);
    }
  }
  return (
    <span
      title={name}
      className={cx("inline-block shrink-0 overflow-hidden rounded-[30%]", ring && "ring-2 ring-moss-400/70 ring-offset-2 ring-offset-night-850", className)}
      style={{ width: size, height: size }}
    >
      <svg viewBox="0 0 7 7" width={size} height={size} shapeRendering="crispEdges" aria-hidden>
        <rect width="7" height="7" fill={bg} />
        {rects}
      </svg>
    </span>
  );
}
