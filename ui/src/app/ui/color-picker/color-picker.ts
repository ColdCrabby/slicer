import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  computed,
  effect,
  inject,
  input,
  output,
  signal,
  viewChild,
} from '@angular/core';
import type { ElementRef, TemplateRef } from '@angular/core';
import { FloatingService } from '../../shared/floating';
import type { FloatingRef } from '../../shared/floating';
import { ColorPickerPreference } from '../../services/color-picker-preference';
import { Icon } from '../../shared/icon/icon';

interface Hsv {
  /** Hue in degrees, 0–360. */
  h: number;
  /** Saturation 0–1. */
  s: number;
  /** Value 0–1. */
  v: number;
}

/** A restrained, filament-friendly default palette. */
const DEFAULT_PRESETS: readonly string[] = [
  '#1a1a1a',
  '#5b5b5b',
  '#f5f5f5',
  '#d92b2b',
  '#e0730f',
  '#f2c200',
  '#3ba55d',
  '#0f9ea0',
  '#2f6df0',
  '#7a5cff',
  '#c94fbf',
  '#8a5a2b',
];

/**
 * Design-system colour picker. A swatch trigger opens a premium HSV popover —
 * a saturation/value field, a hue slider, a hex field, and optional preset
 * swatches — rendered on a solid Nexus surface through the shared
 * FloatingService (so it never clips and dismisses like every other popover).
 *
 * The OS-native picker is trash on web/wasm but magnificent on macOS, so which
 * one the trigger opens follows the app-wide {@link ColorPickerPreference}
 * (Appearance → Colour picker): `app` opens the in-app popover, `os` opens the
 * native `<input type="color">` dialog directly, and `auto` picks the native
 * dialog only on macOS. Whenever the native dialog is enabled the in-app
 * popover also keeps a **System picker** button. Set `nativePicker` to `false`
 * to force the in-app picker for a specific instance.
 *
 * Controlled: the parent owns `value` (a `#rrggbb` hex string) and updates it
 * from `valueChange`.
 *
 * ```html
 * <nexus-color-picker [value]="color()" (valueChange)="color.set($event)" />
 * ```
 */
@Component({
  selector: 'nexus-color-picker',
  standalone: true,
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './color-picker.html',
  styleUrl: './color-picker.scss',
  host: {
    '[class.is-open]': 'open()',
    '[class.is-disabled]': 'disabled()',
  },
})
export class ColorPicker {
  private readonly triggerEl = viewChild.required<ElementRef<HTMLElement>>('triggerEl');
  private readonly menuTpl = viewChild.required<TemplateRef<unknown>>('menuTpl');
  private readonly nativeInput = viewChild<ElementRef<HTMLInputElement>>('nativeInput');

  private readonly floating = inject(FloatingService);
  private readonly preference = inject(ColorPickerPreference);
  private floatingRef: FloatingRef | null = null;

  readonly value = input('#000000');
  readonly disabled = input(false);
  /** Allow the OS-native dialog for this instance; `false` forces the in-app picker. */
  readonly nativePicker = input(true);
  /** Optional row of quick-pick swatches shown under the sliders. */
  readonly presets = input<readonly string[]>(DEFAULT_PRESETS);
  /** Accessible label for the trigger. */
  readonly ariaLabel = input('Choose colour');
  readonly valueChange = output<string>();

  protected readonly open = signal(false);

  /** Whether the OS-native dialog is available (per-instance opt-out + preference). */
  protected readonly nativeEnabled = computed(
    () => this.nativePicker() && this.preference.resolved() === 'os',
  );

  // Working HSV — the single source of truth while the popover is open. Seeded
  // from `value` and re-seeded whenever `value` changes from the outside.
  private readonly hsv = signal<Hsv>({ h: 0, s: 0, v: 0 });
  private lastEmitted: string | null = null;

  protected readonly hue = computed(() => this.hsv().h);
  protected readonly sat = computed(() => this.hsv().s);
  protected readonly val = computed(() => this.hsv().v);
  protected readonly current = computed(() => hsvToHex(this.hsv()));
  protected readonly hexDigits = computed(() => this.current().replace('#', '').toUpperCase());

  constructor() {
    // Reseed the working HSV from an external value change (but not from our
    // own emissions, which would clobber hue while dragging greys).
    effect(() => {
      const incoming = normalizeHex(this.value());
      if (incoming && incoming !== this.lastEmitted) {
        this.hsv.set(hexToHsv(incoming));
      }
    });
    inject(DestroyRef).onDestroy(() => this.closeMenu());
  }

  protected toggle(): void {
    if (this.disabled()) return;
    if (this.open()) {
      this.close();
      return;
    }
    // Preference decides whether the swatch jumps straight to the OS dialog
    // (great on macOS) or opens the in-app popover.
    if (this.nativeEnabled()) {
      this.openNative();
    } else {
      this.openMenu();
    }
  }

  protected openMenu(): void {
    const seed = normalizeHex(this.value());
    if (seed) this.hsv.set(hexToHsv(seed));
    this.open.set(true);

    const trigger = this.triggerEl().nativeElement;
    this.floatingRef = this.floating.openTemplate(
      this.menuTpl(),
      {},
      {
        reference: trigger,
        interactive: true,
        originElement: trigger,
        options: {
          placement: 'bottom-start',
          offset: 6,
          padding: 8,
          size: true,
        },
        onOutsidePointer: () => this.close(),
        onEscape: () => this.close(),
      },
    );
  }

  protected close(): void {
    this.open.set(false);
    this.closeMenu();
  }

  private closeMenu(): void {
    this.floatingRef?.close();
    this.floatingRef = null;
  }

  // --- Editing ------------------------------------------------------------

  private emit(hsv: Hsv): void {
    this.hsv.set(hsv);
    const hex = hsvToHex(hsv);
    this.lastEmitted = hex;
    this.valueChange.emit(hex);
  }

  /** Drag within the saturation/value field. */
  protected onSvPointer(event: PointerEvent, el: HTMLElement): void {
    event.preventDefault();
    el.setPointerCapture(event.pointerId);
    const move = (e: PointerEvent): void => {
      const rect = el.getBoundingClientRect();
      const s = clamp01((e.clientX - rect.left) / rect.width);
      const v = 1 - clamp01((e.clientY - rect.top) / rect.height);
      this.emit({ ...this.hsv(), s, v });
    };
    move(event);
    const up = (): void => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  }

  /** Drag along the hue track. */
  protected onHuePointer(event: PointerEvent, el: HTMLElement): void {
    event.preventDefault();
    el.setPointerCapture(event.pointerId);
    const move = (e: PointerEvent): void => {
      const rect = el.getBoundingClientRect();
      const h = clamp01((e.clientX - rect.left) / rect.width) * 360;
      this.emit({ ...this.hsv(), h });
    };
    move(event);
    const up = (): void => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  }

  protected onHexInput(event: Event): void {
    const raw = (event.target as HTMLInputElement).value;
    const hex = normalizeHex(raw.startsWith('#') ? raw : `#${raw}`);
    if (hex) this.emit(hexToHsv(hex));
  }

  protected pickPreset(hex: string): void {
    const norm = normalizeHex(hex);
    if (norm) this.emit(hexToHsv(norm));
  }

  protected isActivePreset(hex: string): boolean {
    return normalizeHex(hex) === this.current();
  }

  protected openNative(): void {
    this.nativeInput()?.nativeElement.click();
  }

  protected onNativeInput(event: Event): void {
    const hex = normalizeHex((event.target as HTMLInputElement).value);
    if (hex) this.emit(hexToHsv(hex));
  }
}

function clamp01(n: number): number {
  return Math.min(1, Math.max(0, n));
}

/** Normalise loose hex input to `#rrggbb`, or return null if unparseable. */
function normalizeHex(input: string): string | null {
  if (!input) return null;
  let hex = input.trim().replace(/^#/, '').toLowerCase();
  if (/^[0-9a-f]{3}$/.test(hex)) {
    hex = hex
      .split('')
      .map((c) => c + c)
      .join('');
  }
  if (!/^[0-9a-f]{6}$/.test(hex)) return null;
  return `#${hex}`;
}

function hexToHsv(hex: string): Hsv {
  const n = parseInt(hex.replace('#', ''), 16);
  const r = ((n >> 16) & 0xff) / 255;
  const g = ((n >> 8) & 0xff) / 255;
  const b = (n & 0xff) / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const d = max - min;
  let h = 0;
  if (d !== 0) {
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  const s = max === 0 ? 0 : d / max;
  return { h, s, v: max };
}

function hsvToHex({ h, s, v }: Hsv): string {
  const c = v * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = v - c;
  let r = 0;
  let g = 0;
  let b = 0;
  if (h < 60) [r, g, b] = [c, x, 0];
  else if (h < 120) [r, g, b] = [x, c, 0];
  else if (h < 180) [r, g, b] = [0, c, x];
  else if (h < 240) [r, g, b] = [0, x, c];
  else if (h < 300) [r, g, b] = [x, 0, c];
  else [r, g, b] = [c, 0, x];
  const to = (n: number): string =>
    Math.round((n + m) * 255)
      .toString(16)
      .padStart(2, '0');
  return `#${to(r)}${to(g)}${to(b)}`;
}
