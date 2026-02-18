import { useMemo } from 'react';
import { getExt, getFilename } from './utils';
import { getExtColor } from './colors';
import type { Manifest, TreeNode } from './types';
import './styles/stats.css';

interface Props {
  manifest: Manifest;
  tree: TreeNode | null;
}

interface LangEntry {
  ext: string;
  count: number;
  bytes: number;
  color: string;
}

interface SizeBucket {
  label: string;
  count: number;
}

interface TopFile {
  path: string;
  size: number;
  ext: string;
  color: string;
}

interface Stats {
  totalFiles: number;
  totalBytes: number;
  totalLOC: number;
  moduleCount: number;
  extensionCount: number;
  langBreakdown: LangEntry[];
  sizeDistribution: SizeBucket[];
  topFiles: TopFile[];
}

function formatBytes(bytes: number): string {
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  if (bytes >= 1_000) return `${(bytes / 1_000).toFixed(1)} KB`;
  return `${bytes} B`;
}

function computeStats(manifest: Manifest, tree: TreeNode | null): Stats | null {
  const allFiles: Array<{ path: string; size: number }> = [];
  for (const entries of Object.values(manifest)) {
    for (const f of entries) {
      allFiles.push({ path: f.path, size: f.size });
    }
  }

  if (allFiles.length === 0) return null;

  const totalFiles = allFiles.length;
  const totalBytes = allFiles.reduce((s, f) => s + f.size, 0);
  const totalLOC = Math.ceil(totalBytes / 40);

  // Module count: top-level keys in tree excluding _files
  let moduleCount = 0;
  if (tree) {
    for (const key of Object.keys(tree)) {
      if (key !== '_files') moduleCount++;
    }
  }

  // Language breakdown
  const extMap = new Map<string, { count: number; bytes: number }>();
  for (const f of allFiles) {
    const ext = getExt(f.path);
    const entry = extMap.get(ext);
    if (entry) {
      entry.count++;
      entry.bytes += f.size;
    } else {
      extMap.set(ext, { count: 1, bytes: f.size });
    }
  }

  const langBreakdown: LangEntry[] = Array.from(extMap.entries())
    .map(([ext, { count, bytes }]) => ({
      ext,
      count,
      bytes,
      color: getExtColor(ext),
    }))
    .sort((a, b) => b.bytes - a.bytes);

  const extensionCount = langBreakdown.length;

  // Size distribution (log-scale buckets)
  const buckets: SizeBucket[] = [
    { label: '<1KB', count: 0 },
    { label: '1-10KB', count: 0 },
    { label: '10-100KB', count: 0 },
    { label: '100KB-1MB', count: 0 },
    { label: '>1MB', count: 0 },
  ];
  for (const f of allFiles) {
    if (f.size < 1_000) buckets[0].count++;
    else if (f.size < 10_000) buckets[1].count++;
    else if (f.size < 100_000) buckets[2].count++;
    else if (f.size < 1_000_000) buckets[3].count++;
    else buckets[4].count++;
  }

  // Top 20 files by size
  const topFiles: TopFile[] = allFiles
    .sort((a, b) => b.size - a.size)
    .slice(0, 20)
    .map(f => {
      const ext = getExt(f.path);
      return { path: f.path, size: f.size, ext, color: getExtColor(ext) };
    });

  return {
    totalFiles,
    totalBytes,
    totalLOC,
    moduleCount,
    extensionCount,
    langBreakdown,
    sizeDistribution: buckets,
    topFiles,
  };
}

function StatCard({ value, label }: { value: string; label: string }) {
  return (
    <div className="stats-card">
      <div className="stats-card-value">{value}</div>
      <div className="stats-card-label">{label}</div>
    </div>
  );
}

function DonutChart({ data }: { data: LangEntry[] }) {
  const total = data.reduce((s, d) => s + d.bytes, 0);
  if (total === 0) return null;

  const radius = 80;
  const circumference = 2 * Math.PI * radius;
  let offset = 0;

  const segments = data.slice(0, 10).map(d => {
    const pct = d.bytes / total;
    const dash = pct * circumference;
    const segment = (
      <circle
        key={d.ext || '__none'}
        cx="100"
        cy="100"
        r={radius}
        fill="none"
        stroke={d.color}
        strokeWidth="24"
        strokeDasharray={`${dash} ${circumference - dash}`}
        strokeDashoffset={-offset}
        filter="url(#neonGlow)"
        opacity="0.85"
      />
    );
    offset += dash;
    return segment;
  });

  return (
    <div className="stats-donut-container">
      <svg width="200" height="200" viewBox="0 0 200 200">
        <defs>
          <filter id="neonGlow">
            <feGaussianBlur stdDeviation="2" result="blur" />
            <feMerge>
              <feMergeNode in="blur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>
        {segments}
        <text
          x="100"
          y="95"
          textAnchor="middle"
          fill="var(--neon-cyan)"
          fontSize="24"
          fontFamily="var(--font-mono)"
        >
          {data.length}
        </text>
        <text
          x="100"
          y="115"
          textAnchor="middle"
          fill="var(--text2)"
          fontSize="10"
          fontFamily="var(--font-mono)"
        >
          EXTENSIONS
        </text>
      </svg>
      <div className="stats-donut-legend">
        {data.slice(0, 10).map(d => (
          <div key={d.ext || '__none'} className="stats-legend-item">
            <span className="stats-legend-dot" style={{ background: d.color }} />
            <span className="stats-legend-ext">{d.ext || '(none)'}</span>
            <span className="stats-legend-count">{d.count}</span>
            <span className="stats-legend-pct">
              {((d.bytes / total) * 100).toFixed(1)}%
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function SizeHistogram({ data }: { data: SizeBucket[] }) {
  const maxCount = Math.max(...data.map(d => d.count), 1);
  const barWidth = 60;
  const chartHeight = 160;
  const gap = 12;
  const topPad = 24;
  const bottomPad = 20;
  const totalWidth = data.length * (barWidth + gap) - gap;
  const svgHeight = topPad + chartHeight + bottomPad;

  return (
    <svg
      viewBox={`0 0 ${totalWidth + 40} ${svgHeight}`}
      preserveAspectRatio="xMidYMid meet"
    >
      <defs>
        <filter id="barGlow">
          <feGaussianBlur stdDeviation="3" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>
      {data.map((d, i) => {
        const barHeight = maxCount > 0 ? (d.count / maxCount) * chartHeight : 0;
        const x = 20 + i * (barWidth + gap);
        const baseline = topPad + chartHeight;
        const y = baseline - barHeight;
        return (
          <g key={d.label}>
            <rect
              x={x}
              y={y}
              width={barWidth}
              height={barHeight}
              fill="var(--neon-cyan)"
              opacity="0.7"
              rx="2"
              filter="url(#barGlow)"
            />
            <text
              x={x + barWidth / 2}
              y={y - 6}
              textAnchor="middle"
              fill="var(--neon-cyan)"
              fontSize="11"
              fontFamily="var(--font-mono)"
            >
              {d.count}
            </text>
            <text
              x={x + barWidth / 2}
              y={baseline + 14}
              textAnchor="middle"
              fill="var(--text2)"
              fontSize="9"
              fontFamily="var(--font-mono)"
            >
              {d.label}
            </text>
          </g>
        );
      })}
    </svg>
  );
}

export default function StatsDashboard({ manifest, tree }: Props) {
  const stats = useMemo(() => computeStats(manifest, tree), [manifest, tree]);

  if (!stats) return null;

  return (
    <div className="stats-dashboard">
      <div className="stats-grid">
        <div className="stats-summary">
          <div className="stats-section-header">OVERVIEW</div>
          <div className="stats-cards">
            <StatCard value={stats.totalFiles.toLocaleString()} label="FILES" />
            <StatCard value={formatBytes(stats.totalBytes)} label="TOTAL SIZE" />
            <StatCard value={stats.totalLOC.toLocaleString()} label="EST. LINES" />
            <StatCard value={stats.moduleCount.toString()} label="MODULES" />
            <StatCard
              value={stats.extensionCount.toString()}
              label="LANGUAGES"
            />
          </div>
        </div>
        <div className="stats-languages">
          <div className="stats-section-header">LANGUAGES</div>
          <DonutChart data={stats.langBreakdown} />
        </div>
        <div className="stats-sizes">
          <div className="stats-section-header">FILE SIZE DISTRIBUTION</div>
          <SizeHistogram data={stats.sizeDistribution} />
        </div>
        <div className="stats-top">
          <div className="stats-section-header">TOP FILES BY SIZE</div>
          <div className="stats-file-list">
            {stats.topFiles.map((f, i) => (
              <div key={f.path} className="stats-file-row">
                <span className="stats-file-rank">
                  {String(i + 1).padStart(2, '0')}
                </span>
                <span
                  className="stats-file-dot"
                  style={{ background: f.color }}
                />
                <span className="stats-file-name" title={f.path}>
                  {getFilename(f.path)}
                </span>
                <span className="stats-file-size">{formatBytes(f.size)}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
