# SaveSync — web

The static landing page for SaveSync. Self-contained HTML + inline CSS
+ minimal JS (one inline submit handler), no framework, no build step.
Deploys are a single `scp` away.

Files:

+ `index.html` — the landing page itself
+ `INSTALL.md` — linked from the footer
+ `screenshots/` — images referenced from the landing page (currently
  just `hero.svg`, the marketing-style main-view mockup)

## Deploying

This is a static site — three files (`index.html` + `INSTALL.md` +
`screenshots/`). Push the directory to any static host or a server
running nginx / caddy.

Specifics (server, paths, DNS) live in maintainer-local notes, not
this repo.

## Updating

The page is intentionally simple — no build step, no framework.
Edit `index.html` directly. Test by opening it in a browser, then
deploy with the `scp` above.
