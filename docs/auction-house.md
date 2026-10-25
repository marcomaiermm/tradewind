# Auction House browser

The **Auction House** page browses TSM data that has already been synced to
your WoW installation. It has no buy/sell actions, needs no extra account and
adds no network source. It also works signed out. **Reload data** re-reads the
local files, and a successful sync refreshes an open browser.

Search (name or item ID), category, quality, minimum-price filter, **Hot deals
only**, sorting and the watchlist combine freely. Pages show up to 40 items.
Only the table body scrolls. Ctrl+F focuses search from anywhere on the page,
including the item detail view. Esc, **Back to results** and the sidebar entry
return to the filtered list without losing sort order or page.

Prices are shown as gold/silver/copper without floating-point rounding, using
the coin textures from your local WoW install. Every TSM price carries its own
data timestamp. Values that are missing or encoded as zero in AuctionDB are
shown as unavailable. These are stored snapshots, not live listings. See TSM's
[price source definitions](https://support.tradeskillmaster.com/which-price-sources-can-i-use-and-what-do-they-mean).

Item names, categories and quality come from `TSMItemInfoDB` per game version.
Known variants inherit name, category, quality and texture from their base item,
but **not** its prices or detail values. Items missing from the cache remain
reachable by ID. When several cache languages exist, the one matching the app
language is preferred.

Item icons are read from your local WoW installation: the texture ID from
`TSMItemInfoDB` is resolved through the selected game version's build in CASC.
Icons load in the background and are cached. A missing icon shows a placeholder
with a quality-colored frame.

## Recommended sort and hot deals

**Recommended** is the default sort: items with a minimum buyout first, then hot
deals, demand class, discount of qualifying deals, sales per day and sale rate.
Names and full IDs break ties deterministically. Clicking a column header sorts
the whole filtered result before paging. Clicking again reverses it. Unknown
values always sort last.

Demand is a transparent **app heuristic**, not a TSM rating:

| Class | Regional sale rate | Avg. sold per day per auction house |
|---|---|---|
| High | ≥ 20 % | ≥ 5 |
| Good | ≥ 10 % | ≥ 1 |
| Low | both known, below *Good* | |
| Unknown | either value missing or invalid | |

A **flame** (hot deal) requires a minimum buyout at least **30 % below the
market value of the selected market** and at least *Good* demand. The filter
uses the exact same core rule. Integer math (`u128`) avoids overflow and never
rounds 29.9 % up to 30 %. A missing market value is never replaced by another
price source. A flame is neither live availability nor a profit forecast.

## Data boundaries

- `core::auction`: market/item identity, copper values, missing prices, demand
  and deal rules, filters, stable sorting. Favorites are keyed by game version
  and full item ID.
- `application::auction::Browser`: immutable catalog behind `Arc`, queries,
  selection and paging.
- `Action::BrowseMarket` runs through the coordinator queue, never calls HTTP
  and never writes WoW files. `Action::ToggleFavorite` stores the watchlist
  atomically in the settings JSON.
- `adapters::auction` reads AppData and SavedVariables, validates the literal
  formats and joins only the selected realm with its known region. Without an
  unambiguous region, no regional prices are guessed.
- `adapters::icons` reads `.build.info`, build config and shared CASC archives
  read-only, selecting exactly by product and build. The cache lives in
  `$XDG_CACHE_HOME/tradewind/item-icons-v1`, stores RGBA thumbnails up to
  64 × 64 px verified by SHA-256, and is capped at 64 MiB per installation and
  product.

All reads are bounded and Lua is never executed.

## Formats

AppData records contain
`select(2, ...).LoadData(kind, scope, [=[return {downloadTime=…,fields={…},data={{…}}}]=])`
with a trailing marker. Kind and scope must match the marker. Long-string
delimiters and field counts are validated. Item IDs are decimal, price and
count values unquoted Base32 (`0–9`, `A–V`). Variant and battle-pet strings keep
their full identity. Regional sale rate and sold-per-day are encoded in
thousandths.

Item cache v19: parallel `names`/`itemStrings` lists separated by byte `0x02`,
plus `data` with 24 ASCII characters per item using the alphabet
`A–Z a–z 0–9 - _`. Multi-digit fields are little-endian 6-bit digits:

| Offset | Field |
|---|---|
| 0–1 | Item level |
| 2–3 | Required level |
| 4–8 | Vendor sell price (copper) |
| 9–10 | Stack size |
| 12–16 | Texture ID |
| 17 | Class ID |
| 19 | Quality |
| 20 / 21 | Bind-on-pickup / crafting-reagent confirmation flags |

An all-ones value means unknown. Other cache versions may still provide names.
Their binary fields are not guessed.

CASC: newest generation per bucket of the shared v7 index, encoding and root
manifest from the exact product build. Content and chunk checksums are
verified. Unencrypted BLTE raw/zlib blocks with output limits are supported, as
are BLP2 textures with palette/alpha, BC1/2/3 or BGRA. Archives are read with
plain reads instead of long-lived memory maps so a concurrent Battle.net update
cannot leave invalid mappings. CASC parsing uses
[`cascette-rs`](https://github.com/Osso/cascette-rs) (pinned revision) and
[`texpresso`](https://github.com/jansol/texpresso) for BCn decoding.
