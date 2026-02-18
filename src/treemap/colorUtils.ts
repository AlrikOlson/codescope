import { getExtRgb } from '../colors';

const BASE: [number, number, number] = [46, 46, 66];
const MIX = 0.35;

export function blendExtColors(breakdown: Record<string, number>): string {
  let total = 0;
  let r = 0, g = 0, b = 0;

  for (const [ext, count] of Object.entries(breakdown)) {
    const rgb = getExtRgb(ext);
    if (!rgb) continue;
    r += rgb[0] * count;
    g += rgb[1] * count;
    b += rgb[2] * count;
    total += count;
  }

  if (total === 0) return `rgb(${BASE[0]},${BASE[1]},${BASE[2]})`;

  r /= total;
  g /= total;
  b /= total;

  const fr = Math.round(BASE[0] * (1 - MIX) + r * MIX);
  const fg = Math.round(BASE[1] * (1 - MIX) + g * MIX);
  const fb = Math.round(BASE[2] * (1 - MIX) + b * MIX);

  return `rgb(${fr},${fg},${fb})`;
}
