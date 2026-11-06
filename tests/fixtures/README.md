# Compatibility fixtures

Most data here is synthetic, derived from the observed format contracts, and
contains no personal data. `tools/generate_fixtures.py` regenerates those files
deterministically. The files marked *recorded* are excerpts of live responses of
the TSM desktop app's API with presigned URL queries removed and the user id
replaced.

| Files | Covers |
|---|---|
| `realms-v2.json`, `regions-v2.json`, `user-realms.json` | *Recorded.* Realm API and subscription excerpts: every game version, faction auction houses, numeric ids |
| `socket-poll.txt` | *Recorded.* Engine.IO polling packets (`\x1e`-joined): ping, `realm_auctiondb` and `region_auctiondb` with numeric and string `lastModified`, `addon`, `app_info` |
| `claims.json` | *Recorded.* Access token claims of a free account |
| `cloud.json` | Cloud backup list: this app's records, an official-app patch record, one without a URL |
| `savedvariables.lua`, `helper.lua`, `appdata.lua` | Lua escapes, numeric tables, scopes, helper region, preserving existing AppData |
| `scan.json`, `scan-wire.json` | Checksum, scan age, legacy filtering |
| `accounting-wire.json` | Sales/buy/expired/cancelled, cursor filter, seven protocol columns |
| `accounting-sales.csv` | Byte-exact CSV export with item names, quoting, CRLF and formula guard |
| `legacy-lzma.zip` | Flat legacy ZIP-LZMA with two SavedVariables files |
| `addon-helper.zip` | Staged install, TOC, preserved price data |
| `traversal.zip`, `symlink.zip`, `corrupt-lua.zip` | Rejected before any existing data changes |
| `realm-selection.json` | Realm identities across version, region and faction, including accents and unknown faction suffixes |

Archive tests shell out to `bsdtar` and `zip`, just like the app.

## Auction House

- `auction-prices.txt`: price block with decimal item IDs, full variant and
  battle-pet IDs, Base32 copper values including 2^32 and missing zeros.
- `auction-items.lua`: three v19 item metadata records (name, class, quality).
- `auction-tooltip-items.lua`: one v19 record with item level 70, required
  level 60, vendor price 12,345 copper, stack size 20, texture 132330 and
  bind-on-pickup, plus unknown fields and a known zero vendor price.
- `auction-appdata.lua`: separate EU/US regions, one realm, commodities and
  sale rates with different data timestamps.

Inline fixtures in `adapters::auction` and `adapters::icons` additionally cover
v19 texture IDs, product/build selection, BLTE chunk checksums and output
limits, BLP2 palette/alpha, BGRA and BC1/2/3. No Blizzard image files are
bundled.
