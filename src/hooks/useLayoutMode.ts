import { useMemo } from 'react';

export type LayoutMode = 'default' | 'search' | 'selection' | 'preview';

export interface LayoutState {
  mode: LayoutMode;
  sidebarVisible: boolean;
  treemapProminent: boolean;
  fileListVisible: boolean;
  contextPanelVisible: boolean;
  previewExpanded: boolean;
}

export function deriveLayoutMode(
  globalSearch: string | null,
  selectedCount: number,
  previewPath: string | null,
): LayoutMode {
  if (previewPath) return 'preview';
  if (selectedCount > 0) return 'selection';
  if (globalSearch) return 'search';
  return 'default';
}

export function getLayoutState(mode: LayoutMode): LayoutState {
  switch (mode) {
    case 'default':
      return {
        mode: 'default',
        sidebarVisible: true,
        treemapProminent: true,
        fileListVisible: true,
        contextPanelVisible: false,
        previewExpanded: false,
      };
    case 'search':
      return {
        mode: 'search',
        sidebarVisible: true,
        treemapProminent: false,
        fileListVisible: true,
        contextPanelVisible: false,
        previewExpanded: false,
      };
    case 'selection':
      return {
        mode: 'selection',
        sidebarVisible: true,
        treemapProminent: false,
        fileListVisible: true,
        contextPanelVisible: true,
        previewExpanded: false,
      };
    case 'preview':
      return {
        mode: 'preview',
        sidebarVisible: false,
        treemapProminent: false,
        fileListVisible: false,
        contextPanelVisible: false,
        previewExpanded: true,
      };
  }
}

export function useLayoutMode(
  globalSearch: string | null,
  selectedCount: number,
  previewPath: string | null,
): LayoutState {
  return useMemo(
    () => getLayoutState(deriveLayoutMode(globalSearch, selectedCount, previewPath)),
    [globalSearch, selectedCount, previewPath],
  );
}
