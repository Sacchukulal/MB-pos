/** Chart geometry uses only the normalized shares supplied by the backend. */
import { useEffect, useId, useRef, useState } from 'react';
import type { PointView } from '../ipc/generated/PointView';
import { Button } from './controls';
import { Icon } from './Icon';

const HEIGHT = 128;
const LEFT = 30;
const TOP = 12;
const BASE = 100;

export function Trend({ points }: { points: readonly PointView[] }) {
  const gradient = useId();
  const container = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(800);
  useEffect(() => {
    const element = container.current;
    if (!element || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry && entry.contentRect.width > 0) setWidth(entry.contentRect.width);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const [mode, setMode] = useState<'area' | 'bars'>('area');
  const [active, setActive] = useState<number | null>(null);
  const peak = points.reduce((best, point, index) => point.share > (points[best]?.share ?? 0) ? index : best, 0);
  const selected = active !== null && points[active] ? active : peak;
  const point = points[selected];
  const step = (width - LEFT * 2) / Math.max(points.length - 1, 1);
  const x = (index: number) => points.length === 1 ? width / 2 : LEFT + index * step;
  const y = (share: number) => BASE - Math.max(0, Math.min(1000, share)) / 1000 * (BASE - TOP);
  // Horizontal tangents keep each segment within its endpoints: no invented peaks or dips.
  const line = points.map((p, i) => {
    if (!i) return `M${x(i)},${y(p.share)}`;
    const middle = (x(i - 1) + x(i)) / 2;
    return `C${middle},${y(points[i - 1]!.share)} ${middle},${y(p.share)} ${x(i)},${y(p.share)}`;
  }).join(' ');
  const every = Math.max(1, Math.ceil(points.length / Math.max(2, Math.floor(width / 80))));
  return <div className="mb-trend" ref={container}>
    <div className="mb-trend__toolbar">
      <div className="mb-trend__readout" aria-live="polite">
        <span>{active === null ? 'Peak sales' : 'Sales'}{point ? ` · ${point.label}` : ''}</span>
        <strong className="mb-numeric">{point?.value}</strong><span>{point?.note}</span>
      </div>
      <div className="mb-trend__switch" role="group" aria-label="Chart style">
        <Button size="sm" variant="quiet" aria-pressed={mode === 'area'} onClick={() => setMode('area')}>Area</Button>
        <Button size="sm" variant="quiet" aria-pressed={mode === 'bars'} onClick={() => setMode('bars')}>Bars</Button>
      </div>
    </div>
    <svg className="mb-trend__plot" viewBox={`0 0 ${width} ${HEIGHT}`} preserveAspectRatio="none" role="group" aria-label="Sales trend. Focus a point to see its figures.">
      <defs><linearGradient id={gradient} x1="0" y1="0" x2="0" y2="1"><stop className="mb-trend__gradienttop" offset="0%" /><stop className="mb-trend__gradientbottom" offset="100%" /></linearGradient></defs>
      {[0, 1, 2, 3].map((n) => <line key={n} className="mb-trend__grid" x1={LEFT} x2={width - LEFT} y1={TOP + (BASE - TOP) * n / 3} y2={TOP + (BASE - TOP) * n / 3} />)}
      {mode === 'area' ? <>
        <path className="mb-trend__area" fill={`url(#${gradient})`} d={`${line} L${x(points.length - 1)},${BASE} L${x(0)},${BASE} Z`} />
        <path className="mb-trend__line" pathLength={1} d={line} />
        {point ? <line className="mb-trend__guide" x1={x(selected)} x2={x(selected)} y1={TOP} y2={BASE} /> : null}
      </> : null}
      {points.map((p, index) => <g key={`${p.label}-${index}`} tabIndex={0} role="img"
        aria-label={[p.label, p.value, p.note].filter(Boolean).join(' · ')}
        className="mb-trend__point" onMouseEnter={() => setActive(index)} onMouseLeave={() => setActive(null)}
        onFocus={() => setActive(index)} onBlur={() => setActive(null)} onClick={() => setActive(index)}>
        <title>{[p.label, p.value, p.note].filter(Boolean).join(' · ')}</title>
        <rect className="mb-trend__hit" x={Math.max(0, x(index) - step / 2)} y={0} width={Math.min(step, width)} height={BASE} />
        {mode === 'bars' ? <rect className={index === selected ? 'mb-trend__bar mb-trend__bar--active' : 'mb-trend__bar'} x={x(index) - Math.min(22, step * 0.6) / 2} y={y(p.share)} width={Math.min(22, step * 0.6)} height={BASE - y(p.share)} rx={4} /> :
          <circle className={index === selected ? 'mb-trend__dot mb-trend__dot--active' : 'mb-trend__dot'} cx={x(index)} cy={y(p.share)} r={index === selected ? 5 : 3} />}
        {(index % every === 0 && (index === 0 || index < points.length - every)) || index === points.length - 1 ? <text className="mb-trend__label" x={x(index)} y={HEIGHT - 7} textAnchor={index === 0 ? 'start' : index === points.length - 1 ? 'end' : 'middle'}>{p.label}</text> : null}
      </g>)}
    </svg>
  </div>;
}

/** Deliberately neutral scaffolding, never illustrative sales masquerading as data. */
export function ChartPlaceholder({ kind, message }: { kind: string; message: string }) {
  return <div className={`mb-chartblank mb-chartblank--${kind}`}>
    {kind === 'donut' ? <div className="mb-chartblank__ring" aria-hidden="true"><Icon name="wallet" size="lg" /></div> :
      kind === 'columns' ? <div className="mb-chartblank__grid" aria-hidden="true"><span /><span /><span /><span /></div> :
        <div className="mb-chartblank__ranks" aria-hidden="true"><span /><span /><span /></div>}
    <div className="mb-chartblank__words"><span className="mb-chartblank__icon"><Icon name={kind === 'columns' ? 'chart' : kind === 'donut' ? 'card' : 'boxes'} /></span><strong>{message || 'No activity in this period'}</strong><span>Recorded activity will appear here.</span></div>
  </div>;
}
