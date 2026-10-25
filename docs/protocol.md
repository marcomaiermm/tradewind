# TSM protocol notes

These notes describe the wire and file formats used by the official
TradeSkillMaster desktop application (Windows client 4.14.2, protocol 41402),
as observed for interoperability. The original application is frozen
Python 3.4 / Qt5. `tools/inspect_pyc.py` reads its Python 3.4 marshal data and
disassembles `.pyc` functions **without executing them**. No original Python
code is included in this project.

```sh
python3 tools/inspect_pyc.py \
  "$HOME/Games/TradeSkillMaster Application/app/library.zip" \
  AppAPI.pyc --function _make_request
```

| Original function | Topic |
|---|---|
| `AppAPI._make_request` | Signed `/v2/` requests, gzip, hosts, channel, addon version |
| `AppAPI.token` / `auth` | OAuth form and TSM session |
| `AppAPI.backup` | Cloud metadata, Base64 file names, binary upload/download |
| `AppAPI.sales` / `sv` | Cursor queries, accounting and SavedVariables upload |
| `WoWHelper.get_accounts` | WoW account directories and game versions |
| `WoWHelper.get_active_classic_factionrealms` | Automatic Classic/Anniversary realm selection |
| `WoWHelper.export_accounting_csv` | CSV fields, item names, export kinds |
| `WoWHelper.do_backup` / `restore_backup` | Backup files, retention, restore |
| `WoWHelper.get_accounting_data` | Sales/buy/expired/cancelled records |
| `WoWHelper.get_auctiondb_scans` | Scan checksum, time window, field mapping |
| `WoWHelper.get_group_data` / `get_sv_data` | Shopping profiles, DB scope mapping |
| `MainThread._check_status` | Status, update order, AppData, backup reconciliation |
| `AppData` | `LoadData` calls and `<TYPE,realm,timestamp>` markers |

## Authentication and HTTP

OAuth token URL:
`https://id.tradeskillmaster.com/realms/app/protocol/openid-connect/token`.

Form: `client_id=legacy-desktop-app`, `grant_type=password`, `username`,
`password`, `code=""`, `redirect_uri=""`, `scope=openid`. Then `POST /v2/auth`
with gzip JSON `{"token":"<access_token>"}`. A successful response contains
`session`, `isPremium` and `endpointSubdomains`. Tradewind additionally uses the
OAuth refresh grant.

Every API request carries `session`, `version`, `time` and `token`, optionally
`channel` and `tsm_version`. `token` is the SHA-256 of
`"41402:<unix time>:<fixed protocol constant>"`.

`auth`, `log` and `realms2` use `app-server.tradeskillmaster.com`. Other services
take their subdomain from the auth response. Path segments are encoded
individually. Realm, account and archive names use URL-safe Base64 with padding.

| Endpoint | Representation |
|---|---|
| `auth` | gzip JSON |
| `status` | GET, channels, addons, realm/region lists, AppData URLs |
| `addon/<AddonName+GameSuffix>` | Archive, channel as query |
| `realms2/list`, `realms2/add/…`, `realms2/remove/…` | JSON, add/remove via GET like the original |
| `backup` | `data` object + `missing` list |
| `backup/<base64(file name)>` | ZIP bytes for upload/download |
| `sales/<region>/<realm>/<account>` | GET `lastUpload`, POST gzip JSON arrays |
| `auctiondb_scan`, `black_market`, `groups` | Time cursor + gzip JSON |
| `analytics`, `error_report`, `log/user` | gzip text in legacy format |
| `sv/<region>/<account>` | gzip JSON, content type `application/octet-stream` |

### Transport

Tradewind defaults to HTTPS with certificate verification. The original uses
plain HTTP for the `/v2/` API and disables TLS verification for OAuth. At
sign-in you can explicitly opt into **Legacy HTTP**: signed API requests then use
HTTP and server-provided HTTP download URLs are kept. HTTPS download URLs are
never downgraded, and OAuth password/refresh requests always use the fixed HTTPS
token URL with verification. There is no automatic fallback on TLS errors.

At the time of writing, `app-server.tradeskillmaster.com` presents a
certificate that only covers `internal.tradeskillmaster.com` and has expired, so
HTTPS session exchange fails and Legacy HTTP is required in practice. In legacy
mode, session tokens, sync/addon data and cloud backups travel unencrypted and
can be read or modified in transit. The sign-in form warns about this and the
account page shows the active mode.

Response sizes are bounded, redirects are not followed and download hosts are
restricted to `tradeskillmaster.com`, its subdomains and exactly
`app-cdn.tradeskillmaster.xyz`. Server errors and transport failures are
reported by service, host and a fixed category (DNS, TLS, refused, timeout)
without raw response bodies, full URLs or query strings, so echoed tokens never
reach the log.

## WoW and Lua

| Directory | Protocol suffix | Status lists |
|---|---|---|
| `_retail_` | *(none)* | `realms`, `regions` |
| `_classic_era_` | `-Classic` | `extraClassicRealms`, `extraClassicRegions` |
| `_classic_` | `-Progression` | `realms-Progression`, `regions-Progression` |
| `_anniversary_` | `-Anniversary` | `extraAnniversaryRealms`, `extraAnniversaryRegions` |
| `_forever_` | *(unknown)* | *(unknown)* |

*WoW: Forever* (Battle.net product `wowf`, release 4 November 2026) is detected
and shown, but TSM has no addon or price data for it yet. It is excluded from
addon and account scans (`wow::TSM_UNSUPPORTED`) until TSM ships support.

The old `-BCC` backup suffix is recognized. SavedVariables live in
`WTF/Account/<Account>/SavedVariables`. The main globals are
`TradeSkillMasterDB`, `TSMItemInfoDB` and `TradeSkillMaster_AppHelperDB`.
Account names are validated before use. The literal parser never evaluates Lua.

Scopes come from `_scopeKeys.realm` and `.factionrealm`. For automatic
Classic/Anniversary selection, `Alliance - Test Realm` becomes
`Test Realm-Alliance`. The helper region provides the region.

`Interface/AddOns/TradeSkillMaster_AppHelper/AppData.lua` contains one record
per data set:

```lua
select(2, ...).LoadData("TYPE","Realm",[[return <Payload>]]) --<TYPE,Realm,1700000000>
```

Existing markers and timestamps are preserved and only newer records are
replaced. Payloads that contain a long-string terminator get a longer
delimiter. `APP_INFO` holds the legacy protocol version, sync time and addon
messages.

## Backups and accounting

Local backups are named `<Account+Suffix>_<unix time>.zip`. Older 14-digit local
date names are recognized. Safety backups live in `keep/`. The original wrote
ZIP-LZMA. Tradewind reads it via libarchive (`bsdtar`) and writes Deflate ZIPs
with `zip`.

Cloud keys are `<SystemID>_<Account+Suffix>`. Metadata contains `timestamp` and
`keep`, archive names append `_<unix time>.zip`. Tradewind uses its own stable
system ID rather than silently adopting the Windows identity.

Accounting strings are `r@Realm@internalData@csvSales`, `csvBuys`, `csvIncome`,
`csvExpense`, `csvExpired` and `csvCancelled`. Both real newlines and the
literal `\\n` are accepted. Uploads use seven fields:

```text
[item_id, price, stack_size, quantity, occurred_at, saved_at, kind]
kind: sale=2, buy=3, expired=4, cancelled=5
```

Sales and buys only include `source=Auction`. The upload cursor compares save
times, not transaction times. CSV exports add item names and guard spreadsheet
apps against formula injection.

AuctionDB scans verify the djb2 checksum modulo 2^24, use a 24-hour window and
keep the original's `lastScan != scanTime` filter unchanged.

## Limits

Synthetic fixtures confirm the reconstructed representation. They do not prove
that the legacy OAuth client stays accepted or that the full current server
contract is covered. Token refresh, CDN behavior and the premium cloud round
trip need live verification.
