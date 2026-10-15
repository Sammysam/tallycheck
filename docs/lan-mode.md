# Office LAN mode

By default the app listens on `127.0.0.1` only, so nothing on the network can reach it. LAN mode lets other office PCs use the app in a browser through the main PC. **Other PCs never open the database file.** The data folder is refused if it sits on a network drive, and only one server may use a data folder at a time.

## Turning it on

1. In `%LOCALAPPDATA%\CenangoAccounting\config.toml` on the main PC:

   ```toml
   lan_enabled = true
   bind_address = "0.0.0.0"
   port = 47800
   # Optional. Default: the private ranges 10/8, 172.16/12, 192.168/16, 169.254/16, fc00::/7, fe80::/10
   lan_allowed_networks = ["192.168.1.0/24"]
   # Optional: extra names people type in the address bar (this PC's own name is always allowed)
   lan_host_names = ["accounts.cenango.local"]
   ```

2. Open the port to the office subnet only. Run as Administrator: `scripts\lan-firewall.ps1`. It adds one rule for the Private network profile and the local subnet. Mark the office network as *Private* in Windows settings.
3. Restart the app. Other PCs open `http://<main-pc-name>:47800/`.

## What protects it

* **Accounts and permissions.** Every API route requires a signed-in user and the route's permission. Failed logins are throttled and audited. Two-factor sign-in is optional per user (D16). With LAN mode on, strongly consider turning 2FA on for every account, especially Admin and anyone with payroll access.
* **Allowed networks.** Requests from addresses outside `lan_allowed_networks` are refused with 403, so a port-forwarded or public-Wi-Fi exposure still does not serve the internet.
* **Host check (DNS rebinding).** The address-bar name must be an IP address, `localhost`, this PC's name or a configured name. A malicious web page that re-points its own domain at the office PC is refused.
* **Request forgery.** The API authenticates with a bearer token sent in the `Authorization` header from the app's own page. No cookies are used, so another site cannot make a browser send authenticated requests. Cross-origin pages get no CORS permission, so they cannot read responses either.
* **Admin-only operations stay local.** Backup and restore answer only an Administrator connected from the main PC itself.
* Security headers: `X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`. Downloads are `Cache-Control: private, no-store`.

## Limits you must accept or address

* **No transport encryption.** LAN traffic is plain HTTP. Anyone who can capture traffic on the office network can read pages, including salaries, and the bearer tokens. A captured token is valid for up to 8 hours. Use LAN mode only on a trusted, wired, or WPA2/WPA3-protected office network with no guest devices. If that cannot be guaranteed, keep LAN mode off. TLS for LAN mode would need a certificate that the other PCs trust; it is not implemented.
* The main PC must be on for others to work, and its sleep settings must allow it.
* LAN users see everything their role permits, exactly as in the reference.
