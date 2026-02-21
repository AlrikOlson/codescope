import type { FindResult } from '../types';

export function MatchTypeBadge({ type, count }: { type: FindResult['matchType']; count: number }) {
  const label = type === 'both' ? `name+${count}` : type === 'content' ? `${count} matches` : 'name';
  const cls = `match-type-badge match-type-${type}`;
  return <span className={cls}>{label}</span>;
}
