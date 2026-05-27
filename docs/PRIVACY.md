# Privacy

raria does not include telemetry, analytics, advertising identifiers, account
sync, crash upload, or automatic usage reporting.

## Local Data

raria stores data only for local operation unless the user or an integration
explicitly sends it elsewhere.

| Data | Purpose |
| --- | --- |
| `raria.toml` | Native configuration |
| native redb session store | task persistence and restore |
| BitTorrent fastresume directory | BitTorrent resume state owned by the selected backend |
| structured logs | diagnostics and operational evidence |
| downloaded files | user-requested output |
| generated shell completion | optional local shell integration |
| files under `var/` | local development, smoke, release, and scratch output |

## Network Behavior

raria connects to endpoints required by user-created tasks. Depending on the
task, this can include HTTP or HTTPS servers, FTP or FTPS servers, SFTP servers,
Metalink mirrors, BitTorrent trackers, DHT nodes, peers, and WebSeed endpoints.

The daemon also accepts native control requests through `/api/v1` and event
subscriptions through `/api/v1/events` on the configured listen address. These
requests are local or integration-driven; raria does not contact a project
service for telemetry.

## Credentials

raria can process bearer tokens, HTTP credentials, FTP credentials, SFTP
credentials, cookies, proxy credentials, `.netrc` data, and SSH keys when
configured by the user. These values should be kept out of logs, issue reports,
shell history, and shared terminal recordings.

## Public Network Evidence

Public smoke tests and real downloads may reveal client IP addresses to the
remote servers, trackers, peers, or hosting providers involved in the task.
This is normal network behavior for the protocols being used, not telemetry
collected by raria.
