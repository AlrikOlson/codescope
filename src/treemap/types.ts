export interface TreemapNode {
  id: string;
  name: string;
  value: number;       // layout weight (dampened for balanced proportions)
  fileCount: number;   // true file count for display
  children: TreemapNode[];
  extBreakdown: Record<string, number>;
  depth: number;
  color: string;  // pre-computed fill color
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Viewport {
  x: number;
  y: number;
  scale: number;
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}
