# eleven landing site

The one-page website for eleven, published at <https://www.justinrmiller.com/eleven/>. Plain
HTML, CSS and JavaScript: no build step, no dependencies, no third-party fonts or trackers.

## Files

| File | What it is |
|---|---|
| `index.html` | The page. All download links fall back to the GitHub releases page, so it works without JavaScript. |
| `styles.css` | Styles. The colours are the app's built-in look (`crates/app/src/ui/builtin.rs`). |
| `main.js` | Seven-segment digits, the 0–11 volume knob, the decorative analyzer, OS detection and the latest-release lookup. |
| `assets/` | `screenshot.png` (a copy of `docs/screenshot.png`) and `icon-256.png` (from `packaging/`); `main-window.png`, `equalizer.png` and `playlist.png` cropped from the screenshot for the tour; `og-image.png`, the 1200×630 link preview; a favicon and touch icon resized from `packaging/icon.png`. |

## What the script does

- **Latest release.** Fetches `api.github.com/repos/justinrmiller/eleven/releases/latest` once per
  browser session, then points each download button at the matching asset (`.dmg`, `.zip`, `.deb`,
  `.tar.gz`) and fills in the version and date. If the request fails or the
  unauthenticated rate limit (60 requests an hour per IP) is hit, the buttons keep linking to the
  releases page.
- **OS detection.** On macOS the hero button downloads the `.dmg` and the shortcut keys read `Cmd`;
  on Linux it jumps to the Linux downloads. The matching download card is marked "YOUR SYSTEM".
- **Knob and analyzer.** The knob is a keyboard-accessible ARIA slider (arrows, Page Up/Down,
  Home/End, wheel, drag). The analyzer is animated, not listening to audio; it pauses off-screen
  and in background tabs, and shows a still frame when the visitor prefers reduced motion.

## Preview locally

```sh
cd eleven-landing-site
python3 -m http.server 8000
# open http://localhost:8000
```

Open it through a server rather than `file://` so the release lookup behaves as it will in
production.

## Publishing

`.github/workflows/pages.yml` publishes this folder to GitHub Pages whenever a push to `main`
changes it, and can also be run by hand from the Actions tab (**Landing site** → **Run
workflow**). Nothing else in the repo is published.

- **One-time setup:** Settings → Pages → Source: **GitHub Actions**. Until then the workflow fails
  at the deploy step.
- **CI:** pushes to `main` that only touch this folder or `pages.yml` skip the Rust CI
  (`paths-ignore` in `ci.yml`). Pull requests still run it.
- **Link previews:** `index.html` points `og:url`, `og:image` and the canonical link at
  `https://www.justinrmiller.com/eleven/`. That domain comes from the custom domain on the
  `justinrmiller.github.io` user site, which `justinrmiller.github.io/eleven/` redirects to; if it
  changes, update them.

## Keeping it in sync

The page describes what you can do with the app rather than how it's built; build and architecture
details stay in the eleven README, which the download section links to. The copy matches eleven
v0.1.3. When the app gains features, loses a limitation, or gets a new screenshot, update
`index.html`, re-copy `docs/screenshot.png` into `assets/`, and re-crop the three tour images (main window rows
0–230, equalizer 231–464, playlist 465–743 of the double-size screenshot).
