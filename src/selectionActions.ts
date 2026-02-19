import type { DepGraph, Manifest } from './types';
import { getFilesForModule } from './utils';

/** Resolve either a dep graph key ("Core") or category path ("Runtime > Core") to a dep key. */
function resolveModuleId(id: string, deps: DepGraph): string | null {
  if (deps[id]) return id;
  const lastSeg = id.split(' > ').pop() || '';
  if (lastSeg && deps[lastSeg]) return lastSeg;
  for (const [mod, entry] of Object.entries(deps)) {
    if (entry.categoryPath === id) return mod;
  }
  return null;
}

/** Given a dep key or categoryPath, return the categoryPath for tree navigation. */
export function resolveCategoryPath(id: string, deps: DepGraph): string | null {
  if (deps[id]) return deps[id].categoryPath || id;
  for (const entry of Object.values(deps)) {
    if (entry.categoryPath === id) return id;
  }
  return id;
}

/** Toggle all files in a single module (for sidebar checkboxes). */
export function toggleModule(
  moduleId: string,
  manifest: Manifest,
  selected: Set<string>,
): Set<string> {
  const paths = getFilesForModule(moduleId, manifest);
  if (paths.length === 0) return selected;
  const next = new Set(selected);
  const allSelected = paths.every(p => selected.has(p));
  if (allSelected) {
    for (const p of paths) next.delete(p);
  } else {
    for (const p of paths) next.add(p);
  }
  return next;
}

/** Select a module and all its direct dependencies (for graph/treemap clicks).
 *  Accepts both dep graph keys ("Core") and category paths ("Runtime > Core").
 *  If the clicked module is already fully selected, deselect the whole neighborhood. */
export function selectWithDeps(
  moduleId: string,
  deps: DepGraph,
  manifest: Manifest,
  selected: Set<string>,
): Set<string> {
  // Resolve to dep graph key regardless of input format
  const resolvedId = resolveModuleId(moduleId, deps);
  const entry = resolvedId ? deps[resolvedId] : null;
  const depKey = resolvedId || moduleId;

  const connectedIds = new Set<string>([depKey]);

  // Add direct dependencies (public + private)
  if (entry) {
    for (const d of entry.public) connectedIds.add(d);
    for (const d of entry.private) connectedIds.add(d);
  }

  // Add reverse deps (modules that depend on this one)
  for (const [mod, e] of Object.entries(deps)) {
    if (e.public.includes(depKey) || e.private.includes(depKey)) {
      connectedIds.add(mod);
    }
  }

  // Resolve each connected module to file paths via its categoryPath
  const allPaths: string[] = [];
  for (const modId of connectedIds) {
    const modEntry = deps[modId];
    const catPath = modEntry?.categoryPath || modId;
    allPaths.push(...getFilesForModule(catPath, manifest));
  }
  if (allPaths.length === 0) return selected;

  // Determine toggle direction from the clicked module's own files
  const ownCatPath = entry?.categoryPath || moduleId;
  const ownPaths = getFilesForModule(ownCatPath, manifest);
  const ownAllSelected = ownPaths.length > 0 && ownPaths.every(p => selected.has(p));

  const next = new Set(selected);
  if (ownAllSelected) {
    for (const p of allPaths) next.delete(p);
  } else {
    for (const p of allPaths) next.add(p);
  }
  return next;
}
