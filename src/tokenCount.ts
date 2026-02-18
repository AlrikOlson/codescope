/** Estimate tokens from raw byte count (~3 chars/token for code). */
export function estimateTokens(bytes: number): number {
  return Math.ceil(bytes / 3);
}

export function formatTokenCount(tokens: number): string {
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M tokens`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}K tokens`;
  return `${tokens} tokens`;
}

export function formatCharCount(chars: number): string {
  if (chars >= 1_000_000) return `${(chars / 1_000_000).toFixed(1)}M chars`;
  if (chars >= 1_000) return `${(chars / 1_000).toFixed(1)}K chars`;
  return `${chars} chars`;
}

export type TokenLevel = 'ok' | 'warn' | 'danger';

export function getTokenLevel(tokens: number): TokenLevel {
  if (tokens > 100_000) return 'danger';
  if (tokens > 50_000) return 'warn';
  return 'ok';
}

export const DEFAULT_BUDGET = 50_000;

const TIER_NAMES: Record<string, string> = {
  '1': 'full',
  '2': 'pruned',
  '3': 'TOC',
  '4': 'manifest',
};

export function formatTierCounts(tierCounts: Record<string, number>): string {
  const parts: string[] = [];
  for (const tier of ['1', '2', '3', '4']) {
    const count = tierCounts[tier];
    if (count && count > 0) {
      parts.push(`${count} ${TIER_NAMES[tier] || `T${tier}`}`);
    }
  }
  return parts.join(', ');
}
