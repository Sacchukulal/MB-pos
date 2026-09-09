/**
 * The charts: shapes drawn from figures Rust already worked out. A point arrives with its share
 * of the largest point or of the whole, in per mille, and with every figure formatted; this file
 * only decides where the ink goes.
 */

import { useEffect, useRef, useState } from 'react';

import type { ChartView } from '../ipc/generated/ChartView';
import type { PointView } from '../ipc/generated/PointView';
import { cx } from './cx';
import { SectionHeader } from './display';

/** The run over time: a column per step. */
const COLUMNS_HEIGHT = 190;
const COLUMNS_TOP = 22;
const COLUMNS_AXIS = 22;
const COLUMN_MAX = 24;
const COLUMN_ROUND = 4;
/** How wide a step's label is, so labels are thinned rather than piled on each other. */
const LABEL_WIDTH = 46;

/** A share of the whole: a ring, its slices parted by a gap of the surface. */
const RING = 40;
const RING_WIDTH = 14;
const RING_GAP = 1.5;

/** The width the columns have to draw in, followed as the window changes. */
function useWidth(): [React.RefObject<HTMLDivElement | null>, number] {
  const ref = useRef<HTMLDivElement | null>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    const node = ref.current;
    if (!node) return undefined;
    setWidth(node.clientWidth);
    if (typeof ResizeObserver === 'undefined') return undefined;
    const watch = new ResizeObserver(() => setWidth(node.clientWidth));
    watch.observe(node);
    return () => watch.disconnect();
  }, []);
  return [ref, width];
}

/** "9 am · 1,240.00 · 12 bills" — the point, said in full. */
function said(point: PointView): string {
  return [point.label, point.value, point.note].filter((part) => part !== '').join(' · ');
}

function hueClass(hue: number): string {
  return hue > 0 ? `mb-chart__mark--hue-${hue}` : 'mb-chart__mark--mono';
}

export function Chart({ chart, className }: { chart: ChartView; className?: string }) {
  return (
    <section className={cx('mb-card', 'mb-chart', className)}>
      <SectionHeader title={chart.title} />
      {chart.note ? <p className="mb-chart__note">{chart.note}</p> : null}
      {chart.points.length === 0 ? (
        <p className="mb-chart__empty">{chart.empty}</p>
      ) : chart.kind === 'columns' ? (
        <Columns points={chart.points} />
      ) : chart.kind === 'donut' ? (
        <Donut points={chart.points} />
      ) : (
        <Bars points={chart.points} />
      )}
    </section>
  );
}

/** One column per step; the tallest reaches the top, and its figure is written on it. */
function Columns({ points }: { points: readonly PointView[] }) {
  const [ref, width] = useWidth();
  const [shown, setShown] = useState<number | null>(null);
  const plot = COLUMNS_HEIGHT - COLUMNS_TOP - COLUMNS_AXIS;
  const slot = width > 0 ? width / points.length : 0;
  const thick = Math.min(COLUMN_MAX, slot * 0.6);
  const every = slot > 0 ? Math.max(1, Math.ceil(LABEL_WIDTH / slot)) : 1;
  const tallest = points.reduce(
    (best, point, at) => (point.share > (points[best]?.share ?? 0) ? at : best),
    0,
  );
  const pointed = (shown === null ? points[tallest] : points[shown]) ?? points[0];

  return (
    <div className="mb-chart__columns" ref={ref}>
      <p className="mb-chart__tip" aria-live="polite">
        {pointed ? said(pointed) : ''}
      </p>
      {width > 0 ? (
        <svg
          className="mb-chart__plot"
          width={width}
          height={COLUMNS_HEIGHT}
          role="img"
          aria-label={points.map(said).join('; ')}
        >
          <line
            className="mb-chart__axis"
            x1={0}
            x2={width}
            y1={COLUMNS_TOP + plot}
            y2={COLUMNS_TOP + plot}
          />
          {points.map((point, at) => {
            const tall = (point.share / 1000) * plot;
            const x = at * slot + (slot - thick) / 2;
            const base = COLUMNS_TOP + plot;
            const top = base - tall;
            const round = Math.min(COLUMN_ROUND, tall);
            // Rounded at the data end, square at the baseline.
            const shape =
              tall <= 0
                ? ''
                : `M${x},${base} V${top + round} a${round},${round} 0 0 1 ${round},-${round} ` +
                  `H${x + thick - round} a${round},${round} 0 0 1 ${round},${round} V${base} Z`;
            return (
              <g
                key={point.label + at}
                className={cx('mb-chart__step', shown === at && 'mb-chart__step--on')}
                tabIndex={0}
                onMouseEnter={() => setShown(at)}
                onMouseLeave={() => setShown(null)}
                onFocus={() => setShown(at)}
                onBlur={() => setShown(null)}
              >
                <title>{said(point)}</title>
                {/* The whole slot answers the pointer, not just the ink. */}
                <rect className="mb-chart__hit" x={at * slot} y={0} width={slot} height={COLUMNS_HEIGHT} />
                {shape ? <path className={cx('mb-chart__mark', hueClass(point.hue))} d={shape} /> : null}
                {at === tallest && tall > 0 ? (
                  <text className="mb-chart__figure" x={x + thick / 2} y={top - 6} textAnchor="middle">
                    {point.value}
                  </text>
                ) : null}
                {at % every === 0 ? (
                  <text
                    className="mb-chart__label"
                    x={at * slot + slot / 2}
                    y={COLUMNS_HEIGHT - 6}
                    textAnchor="middle"
                  >
                    {point.label}
                  </text>
                ) : null}
              </g>
            );
          })}
        </svg>
      ) : null}
    </div>
  );
}

/** A ranking: the label, the bar against its track, the figure at the tip. */
function Bars({ points }: { points: readonly PointView[] }) {
  return (
    <ol className="mb-chart__bars">
      {points.map((point, at) => (
        <li className="mb-chart__row" key={point.label + at} title={said(point)}>
          <span className="mb-chart__rowlabel">{point.label}</span>
          <svg className="mb-chart__track" width="100%" role="presentation" aria-hidden="true">
            <rect className="mb-chart__trackline" x={0} y={0} width="100%" height="100%" />
            <rect
              className={cx('mb-chart__mark', hueClass(point.hue))}
              x={0}
              y={0}
              width={`${point.share / 10}%`}
              height="100%"
              rx={COLUMN_ROUND}
            />
          </svg>
          <span className="mb-chart__rowfigure">
            <span className="mb-numeric">{point.value}</span>
            {point.note ? <span className="mb-chart__rownote">{point.note}</span> : null}
          </span>
        </li>
      ))}
    </ol>
  );
}

/** A share of the whole: the ring, and the legend that names every slice. */
function Donut({ points }: { points: readonly PointView[] }) {
  let offset = 0;
  return (
    <div className="mb-chart__donut">
      <svg className="mb-chart__ring" viewBox="0 0 100 100" role="img" aria-label={points.map(said).join('; ')}>
        <circle className="mb-chart__trackline" cx={50} cy={50} r={RING} fill="none" strokeWidth={RING_WIDTH} />
        {points.map((point, at) => {
          const slice = Math.max(point.share / 10 - RING_GAP, 0);
          const start = offset;
          offset += point.share / 10;
          return (
            <circle
              key={point.label + at}
              className={cx('mb-chart__mark', 'mb-chart__slice', hueClass(point.hue))}
              cx={50}
              cy={50}
              r={RING}
              fill="none"
              strokeWidth={RING_WIDTH}
              pathLength={100}
              strokeDasharray={`${slice} ${100 - slice}`}
              strokeDashoffset={-start}
            >
              <title>{said(point)}</title>
            </circle>
          );
        })}
      </svg>
      <ul className="mb-chart__legend">
        {points.map((point, at) => (
          <li className="mb-chart__key" key={point.label + at}>
            <span className={cx('mb-chart__swatch', hueClass(point.hue))} aria-hidden="true" />
            <span className="mb-chart__keylabel">{point.label}</span>
            <span className="mb-chart__keyfigure">
              <span className="mb-numeric">{point.value}</span>
              {point.note ? <span className="mb-chart__rownote">{point.note}</span> : null}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
