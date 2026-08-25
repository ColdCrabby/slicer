import type { PrinterGcodeFlavor } from './printer.model';

/**
 * A ready-made G-code preset for a printer's start / end / layer-change blocks.
 *
 * Templates target common firmware setups (plain Marlin, mainline Klipper,
 * Klippain) so a user can populate the printer's G-code with one click instead
 * of hand-writing macros. Applying a template also aligns the printer's
 * {@link PrinterGcodeFlavor} when the preset is firmware-specific.
 *
 * The blocks may use the same placeholders the engine substitutes at slice time
 * (see {@link GCODE_PLACEHOLDER_HINT}) so a single preset works across
 * materials — the resolved temperatures are injected per print.
 */
export interface GcodeTemplate {
  /** Stable identifier, used as the dropdown value. */
  readonly id: string;
  /** Human-readable name shown in the dropdown. */
  readonly label: string;
  /** Short one-liner describing the preset. */
  readonly description?: string;
  /** When set, applying the template also switches the printer to this flavor. */
  readonly flavor?: PrinterGcodeFlavor;
  readonly startGcode: string;
  readonly endGcode: string;
  readonly layerGcode: string;
}

/** Sentinel id for "leave the fields as the user typed them". */
export const CUSTOM_TEMPLATE_ID = 'custom';

/**
 * Params key holding the template the user picked as their starting point.
 *
 * Distinct from {@link detectGcodeTemplateId}: this records the *chosen* base
 * even after the user has edited the blocks away from it, which is what lets us
 * show "Modified from …" instead of a bare "Custom". The slicer ignores this
 * key (`SlicingParams` has no `deny_unknown_fields`), so it rides along in the
 * printer's `params` bag harmlessly.
 */
export const GCODE_TEMPLATE_ID_KEY = 'gcode_template_id';

/**
 * Params key holding the {@link gcodeBlocksSignature} of the template's blocks
 * *at the moment it was applied*. Comparing this to the template's current
 * signature tells "the user edited it" apart from "we shipped a new version of
 * the template".
 */
export const GCODE_TEMPLATE_REV_KEY = 'gcode_template_rev';

/** Placeholders the engine resolves at slice time (shown as an editor hint). */
export const GCODE_PLACEHOLDER_HINT =
  '{nozzle_temp} · {bed_temp} · {nozzle_temp_first_layer} · {bed_temp_first_layer} · ' +
  '{chamber_temp} · {filament_type} · {layer_height} · {first_layer_height}; ' +
  'layer G-code also has {z} · {height} · {layer_num}';

const STANDARD_MARLIN: GcodeTemplate = {
  id: 'marlin-standard',
  label: 'Standard Marlin',
  description: 'Home, heat and wait using raw M-commands.',
  flavor: 'marlin',
  startGcode: `; Nexus standard Marlin start
G21 ; millimetres
G90 ; absolute positioning
M82 ; extruder absolute mode
M140 S{bed_temp_first_layer} ; set bed temperature
M104 S{nozzle_temp_first_layer} ; set nozzle temperature
G28 ; home all axes
M190 S{bed_temp_first_layer} ; wait for bed temperature
M109 S{nozzle_temp_first_layer} ; wait for nozzle temperature
G92 E0 ; reset extruder
G1 Z2.0 F3000 ; lift nozzle`,
  endGcode: `; Nexus standard Marlin end
G91 ; relative positioning
G1 E-2 F2700 ; retract
G1 Z10 F3000 ; lift
G90 ; absolute positioning
M104 S0 ; nozzle off
M140 S0 ; bed off
M84 ; disable steppers`,
  layerGcode: '',
};

const STANDARD_KLIPPER: GcodeTemplate = {
  id: 'klipper-standard',
  label: 'Standard Klipper',
  description: 'PRINT_START / PRINT_END macros (mainline convention).',
  flavor: 'klipper',
  startGcode: `PRINT_START EXTRUDER={nozzle_temp_first_layer} BED={bed_temp_first_layer}`,
  endGcode: `PRINT_END`,
  layerGcode: '',
};

const KLIPPAIN: GcodeTemplate = {
  id: 'klippain',
  label: 'Klippain',
  description: 'START_PRINT / END_PRINT with temperature, chamber and material parameters.',
  flavor: 'klipper',
  startGcode: `START_PRINT EXTRUDER={nozzle_temp_first_layer} BED={bed_temp_first_layer} CHAMBER={chamber_temp} MATERIAL={filament_type}`,
  endGcode: `END_PRINT`,
  layerGcode: `_ON_LAYER_CHANGE LAYER={layer_num} Z={z}`,
};

/** All selectable presets, in dropdown order. `custom` is appended by the UI. */
export const GCODE_TEMPLATES: readonly GcodeTemplate[] = [
  STANDARD_MARLIN,
  STANDARD_KLIPPER,
  KLIPPAIN,
];

/** Template a from-scratch printer starts attached to. */
export const DEFAULT_GCODE_TEMPLATE_ID = STANDARD_MARLIN.id;

/** The best default template id for a printer of the given firmware flavor. */
export function defaultGcodeTemplateIdForFlavor(flavor: PrinterGcodeFlavor | undefined): string {
  return flavor === 'klipper' ? STANDARD_KLIPPER.id : STANDARD_MARLIN.id;
}

/** Dropdown options including the trailing "Custom" entry. */
export const GCODE_TEMPLATE_OPTIONS: { value: string; label: string; description?: string }[] = [
  ...GCODE_TEMPLATES.map((t) => ({ value: t.id, label: t.label, description: t.description })),
  { value: CUSTOM_TEMPLATE_ID, label: 'Custom', description: 'Your own G-code (edited below).' },
];

/** The params keys a template writes. */
export interface GcodeTemplatePatch {
  start_gcode: string;
  end_gcode: string;
  layer_gcode: string;
  gcode_flavor?: PrinterGcodeFlavor;
  /** The chosen base template id, so later edits are still traceable to it. */
  gcode_template_id: string;
  /** Signature of the blocks at apply time (see {@link gcodeBlocksSignature}). */
  gcode_template_rev: string;
}

function normalize(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}

/**
 * A stable, order-sensitive signature of the three G-code blocks.
 *
 * Used to detect drift between a printer's stored blocks and a template's
 * current definition. FNV-1a over the trimmed, `\u0000`-joined blocks — small,
 * deterministic, and collision-resistant enough for change detection (this is
 * not a security hash).
 */
export function gcodeBlocksSignature(start: unknown, end: unknown, layer: unknown): string {
  const input = `${normalize(start)}\u0000${normalize(end)}\u0000${normalize(layer)}`;
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    // FNV prime 16777619, kept in 32-bit range via Math.imul.
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(16).padStart(8, '0');
}

/** Signature of a template's own blocks. */
function templateSignature(template: GcodeTemplate): string {
  return gcodeBlocksSignature(template.startGcode, template.endGcode, template.layerGcode);
}

/** Build the params patch for a template id, or `null` for `custom`/unknown. */
export function gcodeTemplatePatch(id: string): GcodeTemplatePatch | null {
  const template = GCODE_TEMPLATES.find((t) => t.id === id);
  if (!template) {
    return null;
  }
  const patch: GcodeTemplatePatch = {
    start_gcode: template.startGcode,
    end_gcode: template.endGcode,
    layer_gcode: template.layerGcode,
    gcode_template_id: template.id,
    gcode_template_rev: templateSignature(template),
  };
  if (template.flavor) {
    patch.gcode_flavor = template.flavor;
  }
  return patch;
}

/**
 * Params patch that detaches a printer from any template ("Custom"), leaving the
 * blocks untouched. Clears the rev so the state resolves to {@link CUSTOM_TEMPLATE_ID}.
 */
export function customGcodeTemplatePatch(): Record<string, unknown> {
  return { [GCODE_TEMPLATE_ID_KEY]: CUSTOM_TEMPLATE_ID, [GCODE_TEMPLATE_REV_KEY]: '' };
}

/**
 * Identify which template the current params match, or {@link CUSTOM_TEMPLATE_ID}
 * when they match none. Compares the three G-code blocks after trimming.
 */
export function detectGcodeTemplateId(params: unknown): string {
  const bag = (params ?? {}) as Record<string, unknown>;
  const start = normalize(bag['start_gcode']);
  const end = normalize(bag['end_gcode']);
  const layer = normalize(bag['layer_gcode']);
  const match = GCODE_TEMPLATES.find(
    (t) =>
      normalize(t.startGcode) === start &&
      normalize(t.endGcode) === end &&
      normalize(t.layerGcode) === layer,
  );
  return match?.id ?? CUSTOM_TEMPLATE_ID;
}

/**
 * The relationship between a printer's current G-code and its chosen template.
 *
 * - `custom`   — no template chosen (free-form G-code).
 * - `synced`   — blocks match the chosen template's current definition exactly.
 * - `modified` — the user edited the blocks away from the template.
 * - `updated`  — the user hasn't touched the blocks, but the template's own
 *                definition changed since it was applied (review recommended).
 */
export type GcodeTemplateStatusKind = 'custom' | 'synced' | 'modified' | 'updated';

export interface GcodeTemplateStatus {
  readonly kind: GcodeTemplateStatusKind;
  /** The chosen template, when one applies (`null` for `custom`). */
  readonly template: GcodeTemplate | null;
  /** Convenience id: the template's id, or {@link CUSTOM_TEMPLATE_ID}. */
  readonly id: string;
}

/**
 * Classify a printer's G-code against its chosen template.
 *
 * Honours the stored {@link GCODE_TEMPLATE_ID_KEY} so edits made after applying
 * a template are still traced back to it. For printers created before templates
 * were tracked (no stored id) it falls back to {@link detectGcodeTemplateId}, so
 * blocks that happen to match a preset verbatim still report `synced`.
 */
export function gcodeTemplateStatus(params: unknown): GcodeTemplateStatus {
  const bag = (params ?? {}) as Record<string, unknown>;
  const storedId = normalize(bag[GCODE_TEMPLATE_ID_KEY]);

  // No tracked template → fall back to verbatim block matching (back-compat).
  if (!storedId || storedId === CUSTOM_TEMPLATE_ID) {
    const detected = GCODE_TEMPLATES.find((t) => t.id === detectGcodeTemplateId(bag));
    return detected
      ? { kind: 'synced', template: detected, id: detected.id }
      : { kind: 'custom', template: null, id: CUSTOM_TEMPLATE_ID };
  }

  const template = GCODE_TEMPLATES.find((t) => t.id === storedId);
  if (!template) {
    // Stored a template id we no longer ship → treat as free-form.
    return { kind: 'custom', template: null, id: CUSTOM_TEMPLATE_ID };
  }

  const liveSig = templateSignature(template);
  const currentSig = gcodeBlocksSignature(bag['start_gcode'], bag['end_gcode'], bag['layer_gcode']);
  if (currentSig === liveSig) {
    return { kind: 'synced', template, id: template.id };
  }

  const savedSig = normalize(bag[GCODE_TEMPLATE_REV_KEY]);
  if (savedSig && savedSig === currentSig) {
    // Blocks are exactly what the template shipped when applied, but the
    // template has since changed → an upstream update to review.
    return { kind: 'updated', template, id: template.id };
  }

  return { kind: 'modified', template, id: template.id };
}

