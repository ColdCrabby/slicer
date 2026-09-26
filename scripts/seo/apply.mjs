#!/usr/bin/env node
//
// apply.mjs — add what search engines and link previews need to a built site.
//
// The app's index.html is shared by every runtime: the desktop app, a
// self-hosted server, a pull-request preview and this site. So it stays
// neutral, with a bare title and nothing that names a host. Only the
// production deploy knows its public address and passes it as SLICER_SITE_URL
// (see site.mjs); this script then writes, into the finished build:
//
//   index.html       a descriptive title and description, the canonical URL,
//                    social-card tags, schema.org data, and home.html for
//                    anything that reads the page without running it
//   robots.txt       crawl everything, and where the sitemap is
//   sitemap.xml      the home page plus every docs page — the docs build
//                    writes its own sitemap, and this folds it in
//   llms.txt         a plain-text map of the site for language models
//   social-card.png  the picture a shared link unfurls into
//
// Every edit to index.html has to land exactly once or the build fails. A
// rewritten index.html that quietly lost its canonical URL would otherwise
// only show up weeks later, in the search results.
//
// Usage (scripts/build-site.sh runs it when the variable is set):
//   SLICER_SITE_URL=https://slicer.maxscopp.de node scripts/seo/apply.mjs _site

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { siteName, siteUrl, socialCard } from './site.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const out = process.argv[2];
const origin = siteUrl();

if (!out || !origin) {
  fail('usage: SLICER_SITE_URL=https://example.com node scripts/seo/apply.mjs <built site>');
}

const home = `${origin}/`;
const card = `${origin}${socialCard.path}`;

// What a search result shows for the home page: the name and what it is, then
// the one paragraph a stranger needs.
const title = 'Cold Crabby — Free Online 3D Printer Slicer';
const description =
  'Slice STL, OBJ and 3MF files to G-code in your browser — free, no account, no install. ' +
  'Your model never leaves your device. Works on desktop and iPad.';

const structuredData = {
  '@context': 'https://schema.org',
  '@graph': [
    {
      '@type': 'WebSite',
      '@id': `${home}#website`,
      url: home,
      name: siteName,
      alternateName: 'Cold Crabby Slicer',
      inLanguage: 'en',
    },
    {
      '@type': 'WebApplication',
      '@id': `${home}#app`,
      name: siteName,
      url: home,
      description,
      image: card,
      applicationCategory: 'DesignApplication',
      applicationSubCategory: '3D printing slicer',
      operatingSystem: 'Any',
      browserRequirements: 'Requires JavaScript, WebAssembly and WebGL 2',
      isAccessibleForFree: true,
      offers: { '@type': 'Offer', price: '0', priceCurrency: 'USD' },
      featureList: [
        'Slices STL, OBJ and 3MF files to G-code inside the browser',
        'Multi-object build plates with automatic arrangement',
        'Variable-width (Arachne) or classic walls',
        'Rectilinear, grid, honeycomb, gyroid and TPMS-D infill',
        'Supports, brims, rafts and ironing',
        'Spiral vase mode and sequential printing',
        'Marlin and Klipper G-code with custom start and end scripts',
        'Toolpath preview coloured by role, speed, flow or temperature',
      ],
      author: { '@type': 'Person', name: 'Max Scopp', url: 'https://github.com/max-scopp' },
      sameAs: ['https://github.com/ColdCrabby/slicer'],
      isPartOf: { '@id': `${home}#website` },
    },
  ],
};

const head = `
    <link rel="canonical" href="${attr(home)}" />
    <meta name="robots" content="max-image-preview:large" />
    <meta property="og:type" content="website" />
    <meta property="og:site_name" content="${attr(siteName)}" />
    <meta property="og:title" content="${attr(title)}" />
    <meta property="og:description" content="${attr(description)}" />
    <meta property="og:url" content="${attr(home)}" />
    <meta property="og:image" content="${attr(card)}" />
    <meta property="og:image:width" content="${socialCard.width}" />
    <meta property="og:image:height" content="${socialCard.height}" />
    <meta property="og:image:alt" content="${attr(socialCard.alt)}" />
    <meta name="twitter:card" content="summary_large_image" />
    <script type="application/ld+json">${JSON.stringify(structuredData).replace(/</g, '\\u003c')}</script>
    <style>
      /* home.html: hidden until the <noscript> rule below reveals it. It has to
         be its own scroll container because the app's reset pins html and body. */
      .site-intro {
        display: none;
        position: fixed;
        inset: 0;
        overflow-y: auto;
        padding: 48px 20px 64px;
        font: 17px/1.6 'Plus Jakarta Sans', system-ui, sans-serif;
      }
      .site-intro__body {
        max-width: 42rem;
        margin: 0 auto;
      }
      .site-intro h1 {
        margin: 0 0 16px;
        font-size: 2rem;
        line-height: 1.2;
      }
      .site-intro h2 {
        margin: 32px 0 8px;
        font-size: 1.2rem;
      }
      .site-intro p,
      .site-intro ul {
        margin: 0 0 16px;
      }
      .site-intro a {
        color: inherit;
      }
    </style>
    <noscript><style>.boot-splash { display: none; } .site-intro { display: block; }</style></noscript>
  `;

const intro = read(path.join(here, 'home.html')).replace(/^\s*<!--[\s\S]*?-->\s*/, '');

const indexPath = path.join(out, 'index.html');
let html = read(indexPath);
html = replaceOnce(html, /<title>[^<]*<\/title>/, `<title>${text(title)}</title>`);
html = replaceOnce(
  html,
  /<meta\s+name="description"\s+content="[^"]*"\s*\/?>/,
  `<meta name="description" content="${attr(description)}" />`,
);
html = replaceOnce(html, /<\/head>/, `${head}</head>`);
html = replaceOnce(html, /<nexus-root><\/nexus-root>/, `<nexus-root>\n${intro}</nexus-root>`);
fs.writeFileSync(indexPath, html);

fs.writeFileSync(
  path.join(out, 'robots.txt'),
  `User-agent: *\nAllow: /\n\nSitemap: ${origin}/sitemap.xml\n`,
);

// The docs config wrote docs/sitemap.xml from the same variable. Reusing its
// <urlset> opening tag keeps whatever namespaces its entries rely on.
const docsSitemap = read(path.join(out, 'docs', 'sitemap.xml'));
if (!docsSitemap.includes('<loc>')) {
  fail('docs/sitemap.xml lists no pages — did the docs build see SLICER_SITE_URL?');
}
fs.writeFileSync(
  path.join(out, 'sitemap.xml'),
  replaceOnce(docsSitemap, /<urlset\b[^>]*>/, (open) => `${open}<url><loc>${home}</loc></url>`),
);

fs.writeFileSync(
  path.join(out, 'llms.txt'),
  read(path.join(here, 'llms.txt')).replaceAll('{{SITE_URL}}', origin),
);

fs.copyFileSync(path.join(here, 'social-card.png'), path.join(out, socialCard.path.slice(1)));

console.log(`Search-engine layer applied for ${home}`);

function replaceOnce(source, pattern, replacement) {
  const matches = source.match(new RegExp(pattern.source, `${pattern.flags}g`)) ?? [];
  if (matches.length !== 1) {
    fail(`expected exactly one match for ${pattern} in the built site, found ${matches.length}`);
  }
  // A function, so a `$` in the copy is never read as a replacement pattern.
  return source.replace(pattern, typeof replacement === 'function' ? replacement : () => replacement);
}

function read(file) {
  if (!fs.existsSync(file)) {
    fail(`missing ${path.relative(process.cwd(), file)}`);
  }
  return fs.readFileSync(file, 'utf8');
}

function text(value) {
  return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function attr(value) {
  return text(value).replace(/"/g, '&quot;');
}

function fail(message) {
  console.error(`seo: ${message}`);
  process.exit(1);
}
