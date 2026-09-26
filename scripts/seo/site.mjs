//
// site.mjs — what the public site says about itself, shared by the two builds
// that publish it: apply.mjs for the app at `/`, and the docs config for
// everything under `/docs/`. One gate and one social card, so the two halves
// of the site can never disagree about either.
//

/**
 * The public origin of the official site, from `SLICER_SITE_URL` — or
 * `undefined` for every other build.
 *
 * Only the production deploy sets it. A pull-request preview, a fork or a
 * self-hosted server leaves it unset, and so never publishes canonical URLs,
 * a sitemap or a social card that claim to be this site.
 */
export function siteUrl() {
  const origin = process.env.SLICER_SITE_URL?.trim().replace(/\/+$/, '');
  if (!origin) {
    return undefined;
  }
  // The app is built for `/` and the docs for `/docs/`, so the site can only
  // live at the root of its host — a path here would be a broken site.
  if (!/^https?:\/\/[^/\s]+$/.test(origin)) {
    throw new Error(`SLICER_SITE_URL must be a bare origin like https://example.com, got "${origin}"`);
  }
  return origin;
}

export const siteName = 'Cold Crabby';

/** The picture a shared link unfurls into. apply.mjs publishes it at `path`. */
export const socialCard = {
  path: '/social-card.png',
  width: 1200,
  height: 630,
  alt: 'Cold Crabby, a 3D printer slicer that runs in your browser — the crab-and-ice-cube mascot beside the name',
};
