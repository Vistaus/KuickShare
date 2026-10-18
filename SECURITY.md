# Security

KuickShare accepts files and connections from the local network, so security reports are welcome.

## Reporting a vulnerability

Please do not open a public issue. Use GitHub's private reporting: **Security → Report a vulnerability** on the
[repository](https://github.com/the-beper/KuickShare/security/advisories/new). Include what you did, what happened and the
version (Help → About). I will answer as soon as I can; this is a personal project, so there is no fixed response time.

Only the latest release is supported.

## What the application does about untrusted senders

- A transfer from an unknown sender always asks for approval in a notification, and is declined after five minutes without an answer.
- A transfer whose file names are not plain names (empty, `.`, `..`, containing `/`, `\` or control characters, or longer than 255 bytes) is declined before it can be accepted.
- The trust channel (TCP port 47891) is rate limited per address and capped in the number of open connections.
- Pairing needs both users to confirm the same 8-digit code, and a pairing window that the user opens.

## Known limits

These come from the Quick Share protocol or from the library used for it (`rqs_lib`), and are not fixed:

- **Senders are not authenticated.** The device name is chosen by the sender. A device in *Trusted devices* is recognised by name and MAC address,
  so someone on the same network who copies both would be accepted. Use trusted devices only on networks you trust.
- The announcement from a paired computer is matched by IP address for 60 seconds.
- Received files are written under the name the sender chose, and the library does not check free disk space or limit sizes.
- A cancelled or interrupted transfer leaves the partial file in the download folder.
- Discovery uses mDNS and a fixed or random TCP port, which must be reachable on the local network.
