/**
 * **Your logo** — P31, and the owner's third item: *"Logo: browse and pick a
 * PNG."*
 *
 * # The conversion happens here, and that is decision D37
 *
 * > *"A PNG decoder is a dependency, an inflate implementation and a parser
 * > being fed a file a shopkeeper uploaded, all to answer a question with two
 * > possible answers per dot… the conversion happens once, upstream: P17's
 * > settings screen takes the JPEG or PNG the owner uploads, decodes it in the
 * > browser — which does that for free and can show the result before it is
 * > saved — thresholds it, and stores this."*
 *
 * The browser has a PNG decoder, a JPEG decoder and a resampler already, and
 * they are somebody else's problem to keep safe. So: Rust opens the file
 * dialog and hands back the bytes, this decodes them, resizes to the paper,
 * thresholds them to one bit, **shows the shopkeeper the actual dots**, and
 * sends those dots to `save_logo`.
 *
 * The screen showing the real dots is the whole point. A thermal printer has
 * no greys: a photograph with a soft grey background comes out as a black
 * rectangle, and the moment to find that out is now — not on the first bill of
 * the lunch rush.
 *
 * # What crosses the wire
 *
 * `MB1`, `mb_print::image`'s own format, base64'd:
 *
 * ```text
 * 0..3   "MB1"
 * 3      version, 1
 * 4..6   width in dots,  u16 little-endian
 * 6..8   height in dots, u16 little-endian
 * 8..    packed rows, ceil(width / 8) bytes each, MSB leftmost, set bit = ink
 * ```
 *
 * Rust decodes it before writing it, so a picture that would silently fail to
 * print is refused here instead, in front of the person who chose it.
 */

import { useCallback, useEffect, useRef, useState } from 'react';

import { Button, Card, Icon, Notice, SectionHeader, Select, Spinner, useToast } from '../kit';
import { call, inApp, isUiError } from '../ipc/call';
import type { LogoView } from '../ipc/generated/LogoView';

/**
 * The widest a stored logo is ever kept, in dots.
 *
 * 576 is the whole of 80 mm paper, so a logo stored at this size can print at
 * any width the shop asks for without ever being enlarged. Bigger would only
 * be dots the printer throws away — and `receipt.logo_width_pct` is what
 * decides how much of the paper it actually uses.
 */
const MAX_DOTS_WIDE = 576;

/**
 * How dark a dot has to be to become ink.
 *
 * Three named choices, not a slider with a number on it. The question a
 * shopkeeper is answering is "is my logo coming out too heavy or too faint",
 * and 0-255 is not the language of that question.
 */
const DARKNESS = [
  { value: '200', label: 'Fainter — keep only the dark parts' },
  { value: '128', label: 'Normal' },
  { value: '70', label: 'Bolder — take more of the picture' },
] as const;

export function Logo() {
  const [view, setView] = useState<LogoView | null>(null);
  const [busy, setBusy] = useState(false);
  /** The picture that was just browsed for, still being looked at. */
  const [chosen, setChosen] = useState<{ name: string; dataUrl: string } | null>(null);
  const [darkness, setDarkness] = useState('128');
  const stored = useRef<HTMLCanvasElement>(null);
  const trying = useRef<HTMLCanvasElement>(null);
  const toast = useToast();

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const load = useCallback(() => {
    if (!inApp()) return;
    call('logo').then(setView).catch(complain);
  }, [complain]);

  useEffect(load, [load]);

  // **Draw the dots that are actually stored**, one canvas pixel per printed
  // dot. Not the original file: the original is not what comes out.
  useEffect(() => {
    const canvas = stored.current;
    const dots = view?.dots;
    if (!canvas || !dots) return;
    canvas.width = dots.width;
    canvas.height = dots.height;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const image = ctx.createImageData(dots.width, dots.height);
    for (let i = 0; i < dots.ink.length; i += 1) {
      const on = dots.ink[i] === 1;
      const at = i * 4;
      // Ink is black on white — the paper's colours, not the theme's, because
      // this is a picture of paper.
      image.data[at] = on ? 0 : 255;
      image.data[at + 1] = on ? 0 : 255;
      image.data[at + 2] = on ? 0 : 255;
      image.data[at + 3] = 255;
    }
    ctx.putImageData(image, 0, 0);
  }, [view]);

  /**
   * Decode, resize, threshold — and give back both the dots to draw and the
   * `MB1` bytes to send.
   *
   * Nothing here is a rule about a bill; it is an image being turned into the
   * only thing a thermal head can print.
   */
  const convert = useCallback(
    (dataUrl: string, cutoff: number) =>
      new Promise<{ width: number; height: number; ink: Uint8Array; encoded: string }>(
        (resolve, reject) => {
          const img = new Image();
          img.onerror = () => reject(new Error('That picture could not be read.'));
          img.onload = () => {
            const width = Math.max(1, Math.min(MAX_DOTS_WIDE, img.naturalWidth));
            const height = Math.max(
              1,
              Math.round((img.naturalHeight * width) / Math.max(1, img.naturalWidth)),
            );
            const canvas = document.createElement('canvas');
            canvas.width = width;
            canvas.height = height;
            const ctx = canvas.getContext('2d');
            if (!ctx) {
              reject(new Error('This computer could not draw the picture.'));
              return;
            }
            // **White behind it first.** A PNG with a transparent background
            // arrives as transparent black, which thresholds to solid ink —
            // a logo that comes out as a filled rectangle. Found by trying one.
            // The same value as `--print-paper`, and for the same reason that
            // token exists: a thermal roll is white when the app is dark, and
            // thresholding against the theme's surface would change what
            // PRINTS when somebody toggles the lights. It cannot read the
            // token — a canvas fill is not a cascade.
            ctx.fillStyle = '#ffffff'; // mb-tokens-allow: the colour of paper, not of the app
            ctx.fillRect(0, 0, width, height);
            ctx.drawImage(img, 0, 0, width, height);

            const pixels = ctx.getImageData(0, 0, width, height).data;
            const ink = new Uint8Array(width * height);
            const stride = Math.ceil(width / 8);
            const bits = new Uint8Array(stride * height);
            for (let y = 0; y < height; y += 1) {
              for (let x = 0; x < width; x += 1) {
                const at = (y * width + x) * 4;
                // Rec. 601 luma, which is what every thresholding tool uses
                // and what makes a red logo behave the way somebody expects.
                // `?? 0` for `noUncheckedIndexedAccess`: every index here is
                // inside the buffer by construction, and asserting that with a
                // `!` would be a lie the day the construction changes.
                const luma =
                  0.299 * (pixels[at] ?? 0) +
                  0.587 * (pixels[at + 1] ?? 0) +
                  0.114 * (pixels[at + 2] ?? 0);
                if (luma < cutoff) {
                  ink[y * width + x] = 1;
                  const byte = y * stride + (x >> 3);
                  bits[byte] = (bits[byte] ?? 0) | (0x80 >> (x % 8));
                }
              }
            }

            const out = new Uint8Array(8 + bits.length);
            out[0] = 0x4d; // M
            out[1] = 0x42; // B
            out[2] = 0x31; // 1
            out[3] = 1;
            out[4] = width & 0xff;
            out[5] = (width >> 8) & 0xff;
            out[6] = height & 0xff;
            out[7] = (height >> 8) & 0xff;
            out.set(bits, 8);

            let binary = '';
            for (const byte of out) binary += String.fromCharCode(byte);
            resolve({ width, height, ink, encoded: btoa(binary) });
          };
          img.src = dataUrl;
        },
      ),
    [],
  );

  // Redraw the trial whenever the picture or the darkness changes, so the
  // three choices are three pictures rather than three words.
  useEffect(() => {
    const canvas = trying.current;
    if (!canvas || !chosen) return;
    let stale = false;
    convert(chosen.dataUrl, Number(darkness))
      .then(({ width, height, ink }) => {
        if (stale) return;
        canvas.width = width;
        canvas.height = height;
        const ctx = canvas.getContext('2d');
        if (!ctx) return;
        const image = ctx.createImageData(width, height);
        for (let i = 0; i < ink.length; i += 1) {
          const on = ink[i] === 1;
          const at = i * 4;
          image.data[at] = on ? 0 : 255;
          image.data[at + 1] = on ? 0 : 255;
          image.data[at + 2] = on ? 0 : 255;
          image.data[at + 3] = 255;
        }
        ctx.putImageData(image, 0, 0);
      })
      .catch(() => undefined);
    return () => {
      stale = true;
    };
  }, [chosen, darkness, convert]);

  const browse = async () => {
    setBusy(true);
    try {
      const picked = await call('pick_a_logo');
      // `null` is Cancel, and Cancel is not a failure.
      if (picked) setChosen({ name: picked.name, dataUrl: picked.dataUrl });
    } catch (cause) {
      complain(cause);
    } finally {
      setBusy(false);
    }
  };

  const keep = async () => {
    if (!chosen) return;
    setBusy(true);
    try {
      const { encoded } = await convert(chosen.dataUrl, Number(darkness));
      setView(await call('save_logo', { encoded }));
      setChosen(null);
      toast.show('ok', 'That is your logo. Turn it on below to print it.');
    } catch (cause) {
      complain(cause);
    } finally {
      setBusy(false);
    }
  };

  if (!view) return <Spinner label="Looking for your logo" />;

  return (
    <Card className="mb-logo">
      <SectionHeader
        title="Your logo"
        note="A picture at the top of every bill. Choose where it goes, and how wide, with the two settings above."
      />

      {view.hasOne ? (
        <div className="mb-logo__has">
          <div className="mb-logo__paper">
            <canvas ref={stored} className="mb-logo__dots" aria-label="Your logo, as it will print" />
          </div>
          <div className="mb-stack">
            <span className="mb-muted">{view.says}</span>
            <div className="mb-row">
              <Button disabled={busy} onClick={() => void browse()}>
                <Icon name="upload" size="sm" />
                Choose a different one
              </Button>
              <Button
                variant="quiet"
                disabled={busy}
                onClick={() => {
                  call('remove_logo').then(setView).catch(complain);
                }}
              >
                Remove it
              </Button>
            </div>
          </div>
        </div>
      ) : (
        <div className="mb-stack">
          <p className="mb-muted">
            No logo yet. A PNG works best — and it will be printed as plain
            black dots, because that is all a bill printer can make.
          </p>
          <div className="mb-row">
            <Button variant="primary" disabled={busy} onClick={() => void browse()}>
              <Icon name="upload" size="sm" />
              Browse…
            </Button>
          </div>
        </div>
      )}

      {chosen ? (
        <div className="mb-logo__trying">
          <Notice tone="info">
            This is exactly how <strong>{chosen.name}</strong> will print. A bill
            printer has no greys — if it looks like a black block, try a fainter
            setting or a picture with a plain white background.
          </Notice>
          <div className="mb-logo__has">
            <div className="mb-logo__paper">
              <canvas ref={trying} className="mb-logo__dots" aria-label="How this will print" />
            </div>
            <div className="mb-stack">
              <Select
                label="How much of the picture to print"
                value={darkness}
                onChange={(event) => setDarkness(event.currentTarget.value)}
                options={DARKNESS.map((d) => ({ value: d.value, label: d.label }))}
              />
              <div className="mb-row">
                <Button variant="primary" disabled={busy} onClick={() => void keep()}>
                  Use this
                </Button>
                <Button variant="quiet" disabled={busy} onClick={() => setChosen(null)}>
                  Cancel
                </Button>
              </div>
            </div>
          </div>
        </div>
      ) : null}
    </Card>
  );
}
