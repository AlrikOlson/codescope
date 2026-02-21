import { forwardRef } from 'react';

interface Props {
  value: string;
  onChange: (v: string) => void;
  loading: boolean;
  onClear: () => void;
}

export const SearchInput = forwardRef<HTMLInputElement, Props>(
  ({ value, onChange, loading, onClear }, ref) => {
    const hasQuery = value.trim().length > 0;

    return (
      <div className="sw-hero">
        <div className="sw-hero-field">
          <svg className="sw-hero-icon" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/>
          </svg>
          <input
            ref={ref}
            type="text"
            className="sw-hero-input"
            placeholder="Search code semantically..."
            value={value}
            onChange={e => onChange(e.target.value)}
            autoFocus
          />
          {loading && <div className="sw-hero-spinner" />}
          {hasQuery && !loading && (
            <button className="sw-hero-clear" onClick={onClear}>&times;</button>
          )}
        </div>
      </div>
    );
  },
);

SearchInput.displayName = 'SearchInput';
