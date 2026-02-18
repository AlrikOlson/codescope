export interface GraphNode {
  id: string;
  x: number;
  y: number;
  z: number;
  vx: number;
  vy: number;
  vz: number;
  radius: number;
  color: string;
  depCount: number;
  categoryPath: string;
  group: string;
}

export interface GraphEdge {
  source: string;
  target: string;
  type: 'public' | 'private';
}

export interface GraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
  nodeMap: Map<string, number>;
  adjacency: Map<string, Set<string>>;
}

export interface DepNode {
  id: string;
  group: string;
  categoryPath: string;
  depCount: number;
  type: 'public' | 'private' | 'root';
  direction: 'depends-on' | 'depended-by' | 'root';
}

export interface DepLevel {
  depth: number;
  nodes: DepNode[];
}

export interface DepTree {
  dependsOn: DepLevel[];
  dependedBy: DepLevel[];
}

export interface MultiConnection {
  from: string;
  to: string;
  type: 'public' | 'private' | 'indirect';
  path: string[];
}

export interface MultiInspectData {
  modules: { id: string; group: string; categoryPath: string; depCount: number; fileCount: number }[];
  connections: MultiConnection[];
  sharedDeps: { id: string; group: string; dependedByCount: number }[];
}

export interface DepGraphTooltip {
  x: number;
  y: number;
  name: string;
  category: string;
  depCount: number;
}
