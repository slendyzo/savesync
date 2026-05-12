# SaveSync — web

The static landing page for SaveSync. Self-contained HTML + inline CSS
+ minimal JS (one inline submit handler), no framework, no build step.
Deploys are a single `scp` away.

Files:

- `index.html` — the landing page itself
- `INSTALL.md` — linked from the footer
- `screenshots/` — images referenced from the landing page (currently
  just `hero.svg`, the marketing-style main-view mockup)

## Deploying to your server

```bash
# From the project root
ssh -i ~/.ssh/id_ed25519 root@YOUR_SERVER_IP 'mkdir -p /var/www/savesync/screenshots'
scp -i ~/.ssh/id_ed25519 -r \
  web/index.html web/INSTALL.md web/screenshots \
  root@YOUR_SERVER_IP:/var/www/savesync/
```

Then on the LXC, configure nginx to serve `/var/www/savesync/` at
`savesync.savesync.app`. The Cloudflare Tunnel handles TLS + the
public route.

Suggested nginx server block:

```nginx
server {
    listen 80;
    server_name savesync.savesync.app;
    root /var/www/savesync;
    index index.html;
    location / { try_files $uri $uri/ =404; }
}
```

## Updating

The page is intentionally simple — no build step, no framework.
Edit `index.html` directly. Test by opening it in a browser, then
deploy with the `scp` above.
