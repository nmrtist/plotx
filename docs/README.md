# PlotX user manual

The PlotX user manual, built with [Starlight](https://starlight.astro.build).
English is the default language (site root); Simplified Chinese lives under
`/zh-cn/`. Pages are matched by filename across languages — an untranslated
page automatically falls back to the English version with a notice banner.

## Layout

- `src/content/docs/` — English pages (default locale)
- `src/content/docs/zh-cn/` — Chinese translations (same filenames)
- `astro.config.mjs` — site title, locales, and sidebar structure

## Commands

Run from this directory:

| Command | Action |
| --- | --- |
| `npm install` | Install dependencies |
| `npm run dev` | Local dev server at `localhost:4321` |
| `npm run build` | Build the static site into `dist/` |
| `npm run preview` | Preview the production build locally |

## Deployment (Cloudflare Workers)

Deployed as a Cloudflare Worker serving static assets (`wrangler.jsonc`);
the site URL is <https://docs.plotx.nmrtist.space>. The Workers Builds Git
integration builds on push with:

- **Path (root directory):** `docs`
- **Build command:** `npm run build`
- **Deploy command:** `npx wrangler deploy`
