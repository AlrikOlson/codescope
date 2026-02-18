import { useState, useEffect } from 'react';
import type { TreeNode, Manifest, DepGraph } from '../types';

export function useDataFetch() {
  const [tree, setTree] = useState<TreeNode>({});
  const [manifest, setManifest] = useState<Manifest>({});
  const [deps, setDeps] = useState<DepGraph | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    Promise.all([
      fetch('/api/tree').then(r => r.json()),
      fetch('/api/manifest').then(r => r.json()),
      fetch('/api/deps').then(r => r.json()),
    ]).then(([t, m, d]) => {
      setTree(t);
      setManifest(m);
      setDeps(d);
      setLoading(false);
    });
  }, []);

  return { tree, manifest, deps, loading };
}
