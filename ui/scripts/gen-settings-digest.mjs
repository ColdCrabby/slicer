// Distils the generated settings schema down to what the app needs at startup.
//
// `src/schemas/slicer-engine-global-settings-v1.json` is ~110 kB: every
// setting's title, description, bounds and enum labels, which only the settings
// forms read. Startup needs two facts per setting — its engine default and which
// setting a proportional one is measured against — and importing the whole
// schema for those put all of it in the initial bundle. This writes just those
// two maps to `src/generated/settings-digest.json` (git-ignored, like the rest
// of `src/generated/`), so the schema itself stays with the lazy forms.
//
// Runs as part of `gen-types`, after the schema is (re)generated.

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const schemaPath = join(root, 'src/schemas/slicer-engine-global-settings-v1.json');
const outPath = join(root, 'src/generated/settings-digest.json');

const schema = JSON.parse(readFileSync(schemaPath, 'utf8'));
const properties = schema.$defs?.SlicingParams?.properties;
if (!properties) {
  throw new Error(`gen-settings-digest: no $defs.SlicingParams.properties in ${schemaPath}`);
}

const defaults = {};
const derivedFrom = {};
for (const [key, prop] of Object.entries(properties)) {
  if (prop.default !== undefined) {
    defaults[key] = prop.default;
  }
  if (typeof prop['x-derived-from'] === 'string') {
    derivedFrom[key] = prop['x-derived-from'];
  }
}

writeFileSync(outPath, JSON.stringify({ defaults, derivedFrom }, null, 2) + '\n');
console.log(
  `gen-settings-digest: ${Object.keys(defaults).length} defaults, ` +
    `${Object.keys(derivedFrom).length} derived → ${outPath}`,
);
