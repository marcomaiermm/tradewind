# Compatibility fixtures

All data here is synthetic, derived from the observed format contracts. Nothing
is a recorded server response and nothing contains personal data.
`tools/generate_fixtures.py` regenerates the files deterministically.

| Files | Covers |
|---|---|
| `protocol.json` | Legacy version, time signature, Base64, gzip JSON, binary request envelopes, HTTP/HTTPS API selection with a fixed HTTPS OAuth URL, CDN host with synthetic path and query |
| `status.json`, `realms.json`, `cloud.json` | Channel object, four game versions, realm selection, cloud names |
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
