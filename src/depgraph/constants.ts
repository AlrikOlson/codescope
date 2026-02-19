export const CLUSTER_POSITIONS: Record<string, [number, number, number]> = {
  'Source':    [ 300,    0,    0],
  'Runtime':   [ 300,    0,    0],
  'Editor':   [   0,  300,    0],
  'Developer':[   0, -300,    0],
  'Programs': [   0,    0,  300],
  'Other':    [   0,    0, -300],
};

export function getGroup(categoryPath: string): string {
  const first = categoryPath.split(' > ')[0] || '';
  if (CLUSTER_POSITIONS[first]) return first;
  return 'Other';
}
