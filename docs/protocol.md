# TSM protocol notes

These notes describe the wire and file formats of the official TradeSkillMaster
desktop application API (Keycloak, REST, Socket.IO), as observed for
interoperability. The legacy `/v2/` API (password grant, signed requests,
`status`, `realms2`) is gone and no longer used.

## Authentication

Keycloak realm `app`, public client `desktop-app`, authorization code with PKCE
(S256) at `https://id.tradeskillmaster.com/realms/app/protocol/openid-connect/`.
Tradewind opens the browser and listens on `http://localhost:<port>/auth/callback`
(the host must be literally `localhost`, the path exact). The `state` is checked.
The refresh token is an offline token: it goes to the keyring when *Remember
sign-in* is on and is revoked on logout. A `User-Agent` is always sent, the token
endpoint answers 403 without one.

Claims are read from the access token payload: `sub` is the user ID, premium is
`premium-gold` or `premium-platinum` in `realm_access.roles`, and addon channels
are `release` plus `channel/<name>` in `resource_access["app-addon-version"].roles`.

## REST

Bearer auth, bounded response sizes, no redirects.

| Endpoint | Use |
|---|---|
| `https://realm-api.tradeskillmaster.com/v2/realms`, `/v2/regions` | Realm and region catalog |
| `https://desktop-backend.tradeskillmaster.com/user-realms` | GET `?userIds=`, POST `{realmId,userId,auctionHouseType}`, DELETE `/<record id>` |
| `/backups`, `/backups/uploads` | Cloud backups (premium only) |

Errors are reported by service and status with the server `message` (capped)
and never with raw bodies or signed URLs.

## Socket.IO feed

Socket.IO v4 over Engine.IO long-polling, with the raw access token in the
`Authorization` header (no `Bearer`). Packets are separated by `\x1e`: `0{sid}`
open, POST `40`, poll with GET, answer ping `2` with `3`, events are `42[...]`,
`44` means refused. Events: `realm_auctiondb`, `region_auctiondb`, `addon`,
`app_info`. A sync ends when `addon` and `app_info` have arrived (20 s timeout).

Price and addon URLs are presigned (DigitalOcean Spaces): GET without auth, gzip.
Hosts are restricted to `digitaloceanspaces.com` and `tradeskillmaster.com` plus
subdomains, https only, no port or userinfo.

`gameVersion` maps to a game and an AppData key prefix: Retail (none), Wrath
(`BCC-`, Progression), Classic Era (`Classic-`), Classic Era - Hardcore (`HC-`),
Season of Discovery (`SoD-`), Classic Era - Fresh (`Fresh-`, Anniversary),
Forever (`Forever-`). Others are skipped. Realm keys are the localized name for
type All, otherwise `<name>-Alliance|Horde`; region keys are prefix plus the
upper-case region prefix.

## WoW and Lua

| Directory | Protocol suffix |
|---|---|
| `_retail_` | *(none)* |
| `_classic_era_` | `-Classic` |
| `_classic_` | `-Progression` |
| `_anniversary_` | `-Anniversary` |
| `_forever_` | `-Forever` |

The old `-BCC` backup suffix is recognized. SavedVariables live in
`WTF/Account/<Account>/SavedVariables`. The main globals are
`TradeSkillMasterDB`, `TSMItemInfoDB` and `TradeSkillMaster_AppHelperDB`.
Account names are validated before use. The literal parser never evaluates Lua.

Scopes come from `_scopeKeys.realm` and `.factionrealm`. For automatic
Classic/Anniversary selection from local data, `Alliance - Test Realm` becomes
`Test Realm-Alliance`. The helper region provides the region.

`Interface/AddOns/TradeSkillMaster_AppHelper/AppData.lua` contains one record
per data set:

```lua
select(2, ...).LoadData("TYPE","Realm",[[return <Payload>]]) --<TYPE,Realm,1700000000>
```

Existing markers and timestamps are preserved and only newer records are
replaced. Payloads that contain a long-string terminator get a longer
delimiter. `APP_INFO` is a Lua string `return {version=…,lastSync=…,message={id,msg},news={…}}`.

## Backups and accounting

Local backups are named `<Account+Suffix>_<unix time>.zip`. Older 14-digit local
date names are recognized. Safety backups live in `keep/`. Tradewind reads
ZIP-LZMA via libarchive (`bsdtar`) and writes Deflate ZIPs with `zip`.

Cloud backups (premium) are records with `type: "original"`, `backupId`,
`gameVersion`, `timestamp` and the description `Tradewind backup`, uploaded via
presigned URL (`POST /backups/uploads`, `PUT`, `POST /backups`). The official app
stores JSON patches instead; Tradewind lists only its own records and cannot
restore patch backups. Record fields are unverified against a premium account.

Accounting upload and log upload are not ported yet. The CSV formats below are
still used for local export.

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

Synthetic fixtures confirm the reconstructed representation. Token refresh,
offline-token revocation and the premium cloud backup round trip need live
verification.
