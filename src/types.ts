export interface FileEntry {
  path: string;
  desc: string;
  size: number;
}

export interface TreeNode {
  [key: string]: TreeNode | FileEntry[];
}

export type Manifest = Record<string, FileEntry[]>;

export interface FlatTreeRow {
  id: string;
  name: string;
  depth: number;
  fileCount: number;
  hasChildren: boolean;
  isExpanded: boolean;
}

// Search types (from server /api/search)
export interface FileSearchResult {
  path: string;
  filename: string;
  dir: string;
  ext: string;
  desc: string;
  category: string;
  score: number;
  filenameIndices: number[];
  pathIndices: number[];
}

export interface ModuleSearchResult {
  id: string;
  name: string;
  fileCount: number;
  score: number;
  matchedIndices: number[];
}

export interface SearchResponse {
  files: FileSearchResult[];
  modules: ModuleSearchResult[];
  queryTime: number;
  totalFiles: number;
  totalModules: number;
}

// Dependency graph
export interface DepEntry {
  public: string[];
  private: string[];
  categoryPath: string;
}

export type DepGraph = Record<string, DepEntry>;

// File content API
export interface FileContentResponse {
  content: string;
  lines: number;
  size: number;
  path: string;
  truncated: boolean;
}

// Content search (grep)
export interface GrepMatch {
  line: string;
  lineNum: number;
}

export interface GrepFileResult {
  path: string;
  desc: string;
  matches: GrepMatch[];
  score: number;
}

export interface GrepResponse {
  results: GrepFileResult[];
  totalMatches: number;
  searchedFiles: number;
  queryTime: number;
}

// Collections
export interface Collection {
  id: string;
  name: string;
  paths: string[];
  created: number;
}

export type CopyMode = 'paths' | 'contents' | 'headers' | 'context';

// Smart Context (budget-aware batch read)
export interface ContextFileEntry {
  content: string;
  tier: number;
  tokens: number;
  importance: number;
  order: number;
}

export interface ContextSummary {
  totalTokens: number;
  totalChars: number;
  budget: number;
  unit: 'tokens' | 'chars';
  tierCounts: Record<string, number>;
  totalFiles: number;
}

export interface ContextResponse {
  files: Record<string, ContextFileEntry>;
  summary: ContextSummary;
}
