import { useState, useEffect } from 'react';

export type Breakpoint = 'compact' | 'standard' | 'wide';

function getBreakpoint(w: number): Breakpoint {
  if (w >= 1920) return 'wide';
  if (w < 1366) return 'compact';
  return 'standard';
}

export function useBreakpoint(): Breakpoint {
  const [bp, setBp] = useState<Breakpoint>(() => getBreakpoint(window.innerWidth));

  useEffect(() => {
    const mqlWide = window.matchMedia('(min-width: 1920px)');
    const mqlCompact = window.matchMedia('(max-width: 1365px)');

    const update = () => setBp(getBreakpoint(window.innerWidth));

    mqlWide.addEventListener('change', update);
    mqlCompact.addEventListener('change', update);

    return () => {
      mqlWide.removeEventListener('change', update);
      mqlCompact.removeEventListener('change', update);
    };
  }, []);

  return bp;
}
