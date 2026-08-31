// Dev-server proxy: the browser talks only to the origin it was loaded from.
//
// scripts/dev.mjs picks the engine's port from a seed, so hardcoding it in the
// UI would defeat the point. Instead the dev server forwards /api and /ws to
// wherever the engine is listening (SLICER_API_PORT), which also mirrors
// production — where one origin serves both the app and the API — and keeps CORS
// out of the picture.
//
// Without the launcher this falls back to the engine's own default port, so
// `pnpm run ui:dev` + `cargo run -- serve` still works.

const port = process.env.SLICER_API_PORT || '5201';
const target = `http://127.0.0.1:${port}`;

export default {
  '/api': { target, changeOrigin: true },
  '/ws': { target, ws: true },
};
