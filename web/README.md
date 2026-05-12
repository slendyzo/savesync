# SaveSync — web

The static landing page for SaveSync. Self-contained HTML + inline CSS
+ no JS dependencies, so deploys are a single `scp` away.

## Deploying to your server

```bash
# From the project root
scp -i ~/.ssh/id_ed25519 web/index.html web/INSTALL.md \
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
