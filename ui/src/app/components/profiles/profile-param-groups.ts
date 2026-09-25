/**
 * Which schema-driven sections each profile editor renders.
 *
 * The printer, filament and print-profile editors each show the slice
 * parameters their contract owns, parsed from the same generated schema the
 * slice sidebar consumes — so a new `SlicingParams` field appears in the right
 * editor with no hand-maintained row. Each editor used to parse the schema
 * itself; the three copies live here now because the Settings search needs the
 * same answer ("which page, and which section, holds this setting?") and must
 * not be able to disagree with the pages about it.
 *
 * Parsed once at module load: the schema never changes at runtime.
 */
import globalSettingsSchema from '../../../schemas/slicer-engine-global-settings-v1.json';
import { SETTING_CONTRACTS, type SettingContractId } from '../../models/setting-contract';
import { controlFor } from '../../schema-form/models/field-control';
import type { FieldDef, SchemaGroup } from '../../schema-form/models/field-def';
import { parseSchema } from '../../schema-form/models/schema-parser';

/** The `SlicingParams` sub-schema, with the definitions it refers to. */
export const SLICING_PARAMS_SCHEMA = {
  ...(globalSettingsSchema.$defs.SlicingParams as Record<string, unknown>),
  $defs: globalSettingsSchema.$defs as Record<string, unknown>,
};

const PARSED = parseSchema(SLICING_PARAMS_SCHEMA);

function contractGroups(id: SettingContractId): string[] {
  return SETTING_CONTRACTS.find((c) => c.id === id)!.groups;
}

/**
 * The contract's groups that the editor renders, in the contract's order, with
 * `skip` fields and every `array` field removed — `nexus-param-field` renders
 * every control in the shared taxonomy except `array`, which fan curves and
 * pause triggers have editors of their own for. Groups left with nothing are
 * dropped entirely.
 */
function editorGroups(names: readonly string[], skip: ReadonlySet<string>): SchemaGroup[] {
  const order = new Map(names.map((name, index) => [name, index]));
  return PARSED.groups
    .filter((g) => order.has(g.name))
    .map((g) => ({
      ...g,
      fields: g.fields.filter((f) => !skip.has(f.key) && controlFor(f) !== 'array'),
    }))
    .filter((g) => g.fields.length > 0)
    .sort((a, b) => order.get(a.name)! - order.get(b.name)!);
}

// ── Printer ────────────────────────────────────────────────────────────────

/**
 * Params the printer editor does not offer.
 *
 * `resolve` writes both from the chosen printer profile on every slice, so a
 * box here would accept an edit and then quietly discard it. The machine's
 * vendor and model are shown with its name in the header instead, where they
 * read as what they are: a description of the printer, not a setting.
 */
const PRINTER_DERIVED_KEYS = new Set(['printer_vendor', 'printer_model']);

/**
 * The slice-parameter groups rendered in the printer editor.
 *
 * The Printer contract also owns `Output`, but its `gcode_flavor` is the
 * bespoke "Firmware" control and its `*_gcode` fields are multiline strings
 * edited through the dedicated G-code block — both need typed widgets
 * `nexus-param-field` can't provide. So the printer only schema-drives
 * `Hardware` and `Retraction`.
 */
export const PRINTER_PARAM_GROUPS: SchemaGroup[] = editorGroups(
  contractGroups('printer').filter((name) => name === 'Hardware' || name === 'Retraction'),
  PRINTER_DERIVED_KEYS,
);

/**
 * The group `fan_configs` declares, and so where its own editor is rendered.
 *
 * Read off the schema rather than written down, so the editor follows the field
 * if `x-group` moves again in `params.rs`. `null` when the field is gone, which
 * renders nothing rather than an orphan section.
 *
 * The fan table is the machine's, not the spool's: it names the physical fans a
 * printer has and the Klipper object each one is wired to. It lived on the
 * filament while `fan_configs` sat in `Cooling`, where editing it for one spool
 * replaced the machine's whole fan list — the filament layer resolves above the
 * printer's — and where the Klipper name field asked a roll of PLA what a fan on
 * your gantry is called.
 */
export const FAN_TABLE_GROUP: string | null =
  PARSED.groups.find((g) => g.fields.some((f) => f.key === 'fan_configs'))?.name ?? null;

/**
 * The settings a machine may correct per material, in schema order.
 *
 * Read from the schema's `x-per-machine-material` annotations, so the engine's
 * `PER_MACHINE_MATERIAL_KEYS` stays the only list of them and a new one appears
 * here with no change.
 */
export const CORRECTABLE_FIELDS: FieldDef[] = PARSED.fields.filter(
  (field) => field.perMachineMaterial,
);

// ── Filament ───────────────────────────────────────────────────────────────

/**
 * Param keys the filament editor lays out itself, in the Identity card at the
 * top.
 *
 * They are schema params like any other, so they would render a second time
 * inside their group. The curated row wins — it sits with the name and colour
 * it belongs beside.
 *
 * The identity fields are here for a second reason: `resolve` overwrites them
 * from the chosen profile on every slice, so the Identity card is not just the
 * nicer place to edit them, it is the only one that has any effect.
 */
const FILAMENT_BESPOKE_KEYS = new Set([
  'filament_diameter_mm',
  'filament_cost_per_kg',
  'filament_name',
  'filament_color',
  'filament_type',
]);

/**
 * The filament-parameter groups rendered in the editor, in the Filament
 * contract's display order (`Material`, `Temperature`, `Cooling`, `Extrusion`,
 * `Filament G-code`).
 */
export const FILAMENT_PARAM_GROUPS: SchemaGroup[] = editorGroups(
  contractGroups('filament'),
  FILAMENT_BESPOKE_KEYS,
);

// ── Process ────────────────────────────────────────────────────────────────

/**
 * The process-parameter groups rendered in the print-profile editor, in the
 * Process contract's display order. Groups owned by other contracts
 * (Hardware, Temperature, …) are left out so it only shows process settings.
 */
export const PROCESS_PARAM_GROUPS: SchemaGroup[] = editorGroups(
  contractGroups('process'),
  new Set(),
);

/** Every schema-driven section, per editor — what the Settings search indexes. */
export const EDITOR_PARAM_GROUPS: Record<SettingContractId, SchemaGroup[]> = {
  printer: PRINTER_PARAM_GROUPS,
  filament: FILAMENT_PARAM_GROUPS,
  process: PROCESS_PARAM_GROUPS,
};
