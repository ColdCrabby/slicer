import { defineConfig } from '@hey-api/openapi-ts';

/**
 * Generates the Cold Crabby Preset Cloud API client.
 *
 * `input` is the **remote** OpenAPI document on `main`, not a vendored copy, so
 * regenerating always tracks the latest deployed contract — the frontend cannot
 * silently drift from the cloud. The generated client is written under
 * `src/generated/catalog-client/` (git-ignored, like every other generated
 * artifact) and consumed by the catalog service.
 *
 * Run with `pnpm --filter slicer-ui gen-catalog-client`.
 */
export default defineConfig({
  input: 'https://raw.githubusercontent.com/ColdCrabby/cloud-presets/main/openapi/openapi.gen.json',
  output: {
    path: './src/generated/catalog-client',
    postProcess: ['prettier'],
  },
  plugins: ['@hey-api/client-fetch'],
});
