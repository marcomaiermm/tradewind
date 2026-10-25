//! Reads local TSM snapshots and item metadata. No network, Lua execution, or writes.
use super::{
    lua,
    wow::{self, AppData, Installation},
};
use crate::{
    core::{
        self, Game,
        auction::{Catalog, Item, ItemDetails, Market, Value as Price},
    },
    i18n::{Language, Text},
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
};

fn known_kind(kind: &str) -> bool {
    matches!(
        kind,
        "AUCTIONDB_NON_COMMODITY_DATA"
            | "AUCTIONDB_NON_COMMODITY_SCAN_STAT"
            | "AUCTIONDB_NON_COMMODITY_HISTORICAL"
            | "AUCTIONDB_COMMODITY_DATA"
            | "AUCTIONDB_COMMODITY_SCAN_STAT"
            | "AUCTIONDB_COMMODITY_HISTORICAL"
            | "AUCTIONDB_REGION_STAT"
            | "AUCTIONDB_REGION_HISTORICAL"
            | "AUCTIONDB_REGION_SALE"
    )
}
fn is_region(kind: &str) -> bool {
    !kind.starts_with("AUCTIONDB_NON_COMMODITY_")
}

pub fn load(
    root: &Path,
    installation: &Installation,
    requested: Option<&Market>,
    hints: &[Market],
    language: Language,
) -> Result<Catalog> {
    let mut catalog = Catalog::default();
    let mut files = BTreeMap::new();
    for &game in &installation.games {
        let data = AppData::load(
            &wow::addon_dir(root, game).join("TradeSkillMaster_AppHelper/AppData.lua"),
        )?;
        let active = wow::active_realms(&installation.accounts, game).unwrap_or_else(|error| {
            catalog.warnings.push(msg!(
                AuctionMetadataWarning,
                error = Text::from_error(&error)
            ));
            Vec::new()
        });
        catalog.markets.extend(markets(game, &data, &active, hints));
        files.insert(game, data);
    }
    catalog
        .markets
        .sort_by(|a, b| (a.game, a.region_only, &a.scope).cmp(&(b.game, b.region_only, &b.scope)));
    catalog.markets.dedup();
    let market = if let Some(requested) = requested {
        catalog
            .markets
            .iter()
            .find(|m| {
                m.game == requested.game
                    && m.scope == requested.scope
                    && m.region_only == requested.region_only
            })
            .cloned()
            .context(msg!(AuctionMarketMissing))?
    } else if let Some(first) = catalog.markets.first() {
        first.clone()
    } else {
        return Ok(catalog);
    };
    let mut items = prices(&files[&market.game], &market)?;
    attach_names(installation, &market, language, &mut items, &mut catalog);
    catalog.selected = Some(market);
    catalog.items = items.into_values().collect();
    Ok(catalog)
}

/// Every realm and region snapshot in one version's AppData.
fn markets(
    game: Game,
    data: &AppData,
    active: &[(String, String)],
    hints: &[Market],
) -> Vec<Market> {
    let regions: BTreeSet<_> = data
        .records()
        .filter(|(kind, _, _, _)| known_kind(kind) && is_region(kind))
        .map(|(_, scope, _, _)| scope)
        .collect();
    data.records()
        .filter(|(kind, _, _, _)| known_kind(kind))
        .map(|(kind, scope, _, _)| {
            let region_only = is_region(kind);
            Market {
                game,
                scope: scope.into(),
                region: if region_only {
                    Some(scope.to_owned())
                } else {
                    infer_region(game, scope, &regions, active, hints)
                },
                region_only,
            }
        })
        .collect()
}

/// A realm snapshot does not name its region: take it from the synced realms, else from the
/// realms in SavedVariables when exactly one region snapshot fits.
fn infer_region(
    game: Game,
    scope: &str,
    regions: &BTreeSet<&str>,
    active: &[(String, String)],
    hints: &[Market],
) -> Option<String> {
    let hinted: BTreeSet<_> = hints
        .iter()
        .filter(|m| m.game == game && m.scope == scope && !m.region_only)
        .filter_map(|m| m.region.as_deref())
        .collect();
    if hinted.len() == 1 {
        return hinted.first().map(|region| (*region).to_owned());
    }
    let matched: BTreeSet<_> = active
        .iter()
        // Retail AppData scopes drop the faction that SavedVariables realms carry.
        .filter(|(_, name)| {
            let name = if game.faction_realms() {
                name.as_str()
            } else {
                core::split_faction(name).0
            };
            name == scope
        })
        .flat_map(|(region, _)| {
            regions
                .iter()
                .copied()
                .filter(move |candidate| candidate.rsplit('-').next() == Some(region))
        })
        .collect();
    (matched.len() == 1)
        .then(|| matched.first().map(|region| (*region).to_owned()))
        .flatten()
}

/// The market's price records, merged per item.
fn prices(data: &AppData, market: &Market) -> Result<BTreeMap<String, Item>> {
    let mut items = BTreeMap::<String, Item>::new();
    for (kind, scope, _, body) in data.records().filter(|(kind, scope, _, _)| {
        known_kind(kind)
            && if is_region(kind) {
                market.region.as_deref() == Some(*scope)
            } else {
                !market.region_only && *scope == market.scope
            }
    }) {
        let parsed = parse_record(kind, scope, body)
            .with_context(|| msg!(AuctionDataInvalid, scope = scope, kind = kind))?;
        for (id, values) in parsed.rows {
            let item = items.entry(id.clone()).or_insert_with(|| Item {
                id,
                ..Item::default()
            });
            if kind.starts_with("AUCTIONDB_COMMODITY_") {
                item.commodity = true;
            }
            for (field, value) in parsed.fields.iter().zip(values) {
                let target = match field.as_str() {
                    "minBuyout" => &mut item.min_buyout,
                    "numAuctions" => &mut item.auctions,
                    "marketValue" => &mut item.market,
                    "marketValueRecent" => &mut item.recent,
                    "historical" => &mut item.historical,
                    "regionMarketValue" => &mut item.region_market,
                    "regionHistorical" => &mut item.region_historical,
                    "regionSale" => &mut item.region_sale,
                    "regionSoldPerDay" => &mut item.sold_per_day,
                    "regionSalePercent" => &mut item.sale_rate,
                    _ => continue,
                };
                ensure!(target.is_none(), msg!(AuctionDuplicateMetric));
                *target = Price::new(value, parsed.timestamp);
            }
        }
    }
    Ok(items)
}

/// Names and metadata from the item caches of this version's accounts, one locale only,
/// preferring the UI language.
fn attach_names(
    installation: &Installation,
    market: &Market,
    language: Language,
    items: &mut BTreeMap<String, Item>,
    catalog: &mut Catalog,
) {
    let mut caches = Vec::new();
    for account in installation
        .accounts
        .iter()
        .filter(|a| a.game == market.game)
    {
        let result = (|| -> Result<_> {
            let mut source = String::new();
            fs::File::open(account.path.join("TradeSkillMaster.lua"))?
                .take(128 * 1024 * 1024 + 1)
                .read_to_string(&mut source)?;
            let db = lua::parse(&source)?;
            metadata(db.get("TSMItemInfoDB").unwrap_or(&Value::Null))
        })();
        match result {
            Ok(cache) => caches.push(cache),
            Err(error) => catalog.warnings.push(msg!(
                AuctionMetadataWarning,
                error = Text::from_error(&error)
            )),
        }
    }
    let preferred = match language {
        Language::English => "en",
        Language::German => "de",
    };
    caches.sort_by_key(|cache| {
        !cache
            .locale
            .as_deref()
            .is_some_and(|l| l.starts_with(preferred))
    });
    let mut names = BTreeMap::new();
    for cache in caches {
        if names.is_empty() {
            catalog.names_locale = cache.locale.clone();
        }
        if cache.locale == catalog.names_locale {
            for (id, info) in cache.items {
                names.entry(id).or_insert(info);
            }
        }
    }
    for item in items.values_mut() {
        let base = item.id.split(':').take(2).collect::<Vec<_>>().join(":");
        if let Some(info) = names.get(&item.id).or_else(|| names.get(&base)) {
            item.name = (!info.name.is_empty()).then(|| info.name.clone());
            item.class = info.class;
            item.quality = info.quality;
            item.texture_id = info.texture_id;
        }
        item.details = names.get(&item.id).and_then(|info| info.details.clone());
        item.index();
    }
}

struct Record {
    fields: Vec<String>,
    timestamp: i64,
    rows: Vec<(String, Vec<u64>)>,
}
fn parse_record(kind: &str, scope: &str, body: &str) -> Result<Record> {
    let args = body
        .strip_prefix("select(2, ...).LoadData(")
        .and_then(|s| s.strip_suffix(')'))
        .context(msg!(AuctionInvalidFormat))?;
    let (labels, literal) = args.split_once(",[").context(msg!(AuctionInvalidFormat))?;
    let wrapper = lua::parse(&format!("record = {{{labels}}}"))?;
    let labels = lua::values(&wrapper["record"]);
    ensure!(
        labels.len() == 2 && labels[0].as_str() == Some(kind) && labels[1].as_str() == Some(scope),
        msg!(AuctionInvalidFormat)
    );
    let (equals, rest) = literal
        .split_once('[')
        .context(msg!(AuctionInvalidFormat))?;
    ensure!(
        equals.len() <= 64 && equals.bytes().all(|b| b == b'='),
        msg!(AuctionInvalidFormat)
    );
    let closing = format!("]{equals}]");
    let payload = rest
        .strip_suffix(&closing)
        .context(msg!(AuctionInvalidFormat))?;
    ensure!(!payload.contains(&closing), msg!(AuctionInvalidFormat));
    parse_payload(payload)
}
fn canonical_id(raw: &str) -> Result<String> {
    let raw = raw.trim_matches('"');
    ensure!(
        !raw.is_empty() && raw.len() <= 256,
        msg!(AuctionInvalidFormat)
    );
    if raw.bytes().all(|b| b.is_ascii_digit()) {
        let number: u32 = raw.parse()?;
        ensure!(number > 0, msg!(AuctionInvalidFormat));
        return Ok(format!("i:{number}"));
    }
    ensure!(
        (raw.starts_with("i:") || raw.starts_with("p:"))
            && raw
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b':' || b == b'-'),
        msg!(AuctionInvalidFormat)
    );
    ensure!(
        raw.split(':')
            .nth(1)
            .and_then(|id| id.parse::<u32>().ok())
            .is_some_and(|id| id > 0),
        msg!(AuctionInvalidFormat)
    );
    Ok(raw.into())
}
fn parse_payload(payload: &str) -> Result<Record> {
    let payload = payload
        .trim()
        .strip_prefix("return ")
        .context(msg!(AuctionInvalidFormat))?;
    let (header, rows) = payload
        .split_once(",data={")
        .context(msg!(AuctionInvalidFormat))?;
    let metadata = lua::parse(&format!("header={header}}}"))?;
    let metadata = &metadata["header"];
    let timestamp = metadata["downloadTime"]
        .as_i64()
        .filter(|t| *t > 0)
        .context(msg!(AuctionInvalidFormat))?;
    let fields = lua::values(&metadata["fields"])
        .iter()
        .map(|f| {
            f.as_str()
                .map(str::to_owned)
                .context(msg!(AuctionInvalidFormat))
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        (2..=32).contains(&fields.len()) && fields[0] == "itemString",
        msg!(AuctionInvalidFormat)
    );
    ensure!(
        fields.iter().collect::<BTreeSet<_>>().len() == fields.len(),
        msg!(AuctionInvalidFormat)
    );
    let fields: Vec<_> = fields.into_iter().skip(1).collect();
    let mut tail = rows
        .strip_suffix("}}")
        .context(msg!(AuctionInvalidFormat))?;
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    while !tail.is_empty() {
        let (row, rest) = tail
            .strip_prefix('{')
            .and_then(|s| s.split_once('}'))
            .context(msg!(AuctionInvalidFormat))?;
        let mut columns = row.split(',');
        let id = canonical_id(columns.next().context(msg!(AuctionInvalidFormat))?)?;
        ensure!(seen.insert(id.clone()), msg!(AuctionInvalidFormat));
        let values = columns
            .map(|value| {
                ensure!(
                    !value.is_empty()
                        && value.len() <= 13
                        && value
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'A'..=b'V').contains(&b)),
                    msg!(AuctionInvalidFormat)
                );
                u64::from_str_radix(value, 32).context(msg!(AuctionInvalidFormat))
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            values.len() == fields.len() && rows.len() < 1_000_000,
            msg!(AuctionInvalidFormat)
        );
        rows.push((id, values));
        tail = if rest.is_empty() {
            rest
        } else {
            rest.strip_prefix(',')
                .filter(|s| !s.is_empty())
                .context(msg!(AuctionInvalidFormat))?
        };
    }
    Ok(Record {
        fields,
        timestamp,
        rows,
    })
}
#[derive(Default)]
struct Metadata {
    locale: Option<String>,
    items: BTreeMap<String, ItemMetadata>,
}
struct ItemMetadata {
    name: String,
    class: Option<u8>,
    quality: Option<u8>,
    texture_id: Option<u32>,
    details: Option<ItemDetails>,
}
fn chunks(value: &Value) -> Result<Vec<String>> {
    let pieces = if let Some(s) = value.as_str() {
        vec![s]
    } else {
        lua::values(value)
            .iter()
            .map(|s| s.as_str().context(msg!(AuctionInvalidMetadata)))
            .collect::<Result<Vec<_>>>()?
    };
    Ok(pieces
        .into_iter()
        .flat_map(|s| s.split('\u{2}').map(str::to_owned))
        .collect())
}
fn metadata(value: &Value) -> Result<Metadata> {
    if value.is_null() {
        return Ok(Metadata::default());
    }
    let ids = chunks(&value["itemStrings"])?;
    let names = chunks(&value["names"])?;
    ensure!(ids.len() == names.len(), msg!(AuctionInvalidMetadata));
    let version: Vec<_> = value["versionStr"]
        .as_str()
        .unwrap_or("")
        .split(',')
        .collect();
    let encoded = value["data"].as_str().unwrap_or("");
    // Only decode the independently documented 24-character v19 layout.
    let decode = version.first() == Some(&"19") && encoded.len() == ids.len() * 24;
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    if decode {
        ensure!(
            encoded.bytes().all(|b| ALPHABET.contains(&b)),
            msg!(AuctionInvalidMetadata)
        );
    }
    let mut result = Metadata {
        locale: version
            .get(1)
            .map(|s| (*s).to_owned())
            .or_else(|| value["locale"].as_str().map(str::to_owned)),
        ..Metadata::default()
    };
    for (index, (id, name)) in ids.into_iter().zip(names).enumerate() {
        let id = canonical_id(&id)?;
        let field = |offset, width| {
            decode
                .then(|| {
                    (0..width).fold(0u32, |value, digit| {
                        let n = ALPHABET
                            .iter()
                            .position(|b| *b == encoded.as_bytes()[index * 24 + offset + digit])
                            .unwrap();
                        value | ((n as u32) << (digit * 6))
                    })
                })
                .filter(|value| *value != (1 << (width * 6)) - 1)
        };
        result.items.insert(
            id,
            ItemMetadata {
                name,
                class: field(17, 1).map(|v| v as u8),
                quality: field(19, 1).map(|v| v as u8),
                texture_id: field(12, 5).filter(|id| *id != 0),
                details: decode.then(|| ItemDetails {
                    level: field(0, 2),
                    required_level: field(2, 2),
                    vendor_price: field(4, 5),
                    stack_size: field(9, 2),
                    binds_on_pickup: field(20, 1) == Some(1),
                    crafting_reagent: field(21, 1) == Some(1),
                }),
            },
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tooltip_metadata_decodes_fixed_v19_values_without_inventing_variant_stats() {
        let fixture = include_str!("../../tests/fixtures/auction-tooltip-items.lua");
        let mut values = lua::parse(fixture).unwrap();
        let cache = metadata(&values["TSMItemInfoDB"]).unwrap();
        let item = &cache.items["i:100"];
        assert_eq!(item.texture_id, Some(132330));
        let details = item.details.as_ref().unwrap();
        assert_eq!(details.level, Some(70));
        assert_eq!(details.required_level, Some(60));
        assert_eq!(details.vendor_price, Some(12345));
        assert_eq!(details.stack_size, Some(20));
        assert!(details.binds_on_pickup);
        assert!(!details.crafting_reagent);
        let unknown = cache.items["i:200"].details.as_ref().unwrap();
        assert_eq!(unknown.level, None);
        assert_eq!(unknown.required_level, None);
        assert_eq!(unknown.stack_size, None);
        assert_eq!(unknown.vendor_price, Some(0));
        assert!(!unknown.binds_on_pickup);
        assert!(unknown.crafting_reagent);
        values["TSMItemInfoDB"]["versionStr"] = "20,enUS,12,1".into();
        assert!(
            metadata(&values["TSMItemInfoDB"]).unwrap().items["i:100"]
                .details
                .is_none()
        );

        let root = tempfile::tempdir().unwrap();
        let helper = root
            .path()
            .join("_retail_/Interface/AddOns/TradeSkillMaster_AppHelper");
        let account = root.path().join("_retail_/WTF/Account/TEST/SavedVariables");
        fs::create_dir_all(&helper).unwrap();
        fs::create_dir_all(&account).unwrap();
        let prices = include_str!("../../tests/fixtures/auction-appdata.lua");
        fs::write(account.join("TradeSkillMaster.lua"), fixture).unwrap();
        fs::write(helper.join("AppData.lua"), prices).unwrap();
        let installation = wow::scan(root.path()).unwrap();
        let market = Market {
            game: Game::Retail,
            scope: "Silvermoon".into(),
            region: None,
            region_only: false,
        };
        let exact = load(
            root.path(),
            &installation,
            Some(&market),
            &[],
            Language::English,
        )
        .unwrap();
        assert_eq!(exact.items[0].details.as_ref().unwrap().level, Some(70));
        fs::write(
            helper.join("AppData.lua"),
            prices.replace("{100,", "{\"i:100:0:1\","),
        )
        .unwrap();
        let variant = load(
            root.path(),
            &installation,
            Some(&market),
            &[],
            Language::English,
        )
        .unwrap();
        assert_eq!(variant.items[0].id, "i:100:0:1");
        assert_eq!(variant.items[0].name.as_deref(), Some("Fixture Handguards"));
        assert_eq!(variant.items[0].quality, Some(4));
        assert!(variant.items[0].details.is_none());
        assert_eq!(variant.items[0].min_buyout, exact.items[0].min_buyout);
    }
    #[test]
    fn item_texture_ids_use_five_little_endian_digits_and_unknown_sentinels() {
        // v19: 12 fields before the 30-bit texture ID, followed by seven fields.
        let cache = |texture: &str| {
            serde_json::json!({
                "versionStr": "19,enUS,2.5.6,69795",
                "itemStrings": "i:2770",
                "names": "Copper Ore",
                "data": format!("____________{texture}_______"),
            })
        };
        assert_eq!(
            metadata(&cache("BAAAA")).unwrap().items["i:2770"].texture_id,
            Some(1)
        );
        assert_eq!(
            metadata(&cache("ABAAA")).unwrap().items["i:2770"].texture_id,
            Some(64)
        );
        assert_eq!(
            metadata(&cache("AAAAB")).unwrap().items["i:2770"].texture_id,
            Some(1 << 24)
        );
        assert_eq!(
            metadata(&cache("_____")).unwrap().items["i:2770"].texture_id,
            None
        );
        assert_eq!(
            metadata(&cache("AAAAA")).unwrap().items["i:2770"].texture_id,
            None
        );
        let mut unknown_version = cache("BAAAA");
        unknown_version["versionStr"] = "20,enUS,2.5.6,69795".into();
        assert_eq!(
            metadata(&unknown_version).unwrap().items["i:2770"].texture_id,
            None
        );
    }
    #[test]
    fn fixed_base32_prices_variants_and_invalid_input() {
        let record =
            parse_payload(include_str!("../../tests/fixtures/auction-prices.txt")).unwrap();
        assert_eq!(record.rows[0], ("i:2770".into(), vec![12345, 20, 15000]));
        assert_eq!(record.rows[1].0, "i:123:0:1");
        assert_eq!(record.rows[1].1[0], 4294967296);
        assert_eq!(record.rows[2].0, "p:42:1:3");
        for data in [
            "return {downloadTime=1,fields={\"itemString\",\"minBuyout\"},data={{42,ZZ}}}",
            "return {downloadTime=1,fields={\"itemString\",\"minBuyout\"},data={{42,1},{42,2}}}",
            "return {downloadTime=1,fields={\"itemString\",\"minBuyout\"},data={{42,1,2}}}",
            "return {downloadTime=1,fields={\"itemString\",\"minBuyout\"},data={{42,VVVVVVVVVVVVV}}}",
            "return {downloadTime=1,fields={\"itemString\",\"minBuyout\"},data={{42,1}}}; os.execute('bad')",
        ] {
            assert!(parse_payload(data).is_err(), "accepted {data}");
        }
    }
    #[test]
    fn fixed_item_cache_names_categories_quality_and_unknown_version() {
        let mut values =
            lua::parse(include_str!("../../tests/fixtures/auction-items.lua")).unwrap();
        let cache = metadata(&values["TSMItemInfoDB"]).unwrap();
        assert_eq!(cache.items["i:2770"].name, "Copper Ore");
        assert_eq!(cache.items["i:2770"].class, Some(7));
        assert_eq!(cache.items["i:2770"].quality, Some(1));
        values["TSMItemInfoDB"]["versionStr"] = "20,enUS,12,1".into();
        assert_eq!(
            metadata(&values["TSMItemInfoDB"]).unwrap().items["i:2770"].class,
            None
        );
        values["TSMItemInfoDB"]["names"] = "Wrong count".into();
        assert!(metadata(&values["TSMItemInfoDB"]).is_err());
    }
    #[test]
    fn fixed_markets_join_only_matching_region_and_keep_metric_dates() {
        let root = tempfile::tempdir().unwrap();
        let helper = root
            .path()
            .join("_retail_/Interface/AddOns/TradeSkillMaster_AppHelper");
        fs::create_dir_all(&helper).unwrap();
        fs::write(
            helper.join("AppData.lua"),
            include_str!("../../tests/fixtures/auction-appdata.lua"),
        )
        .unwrap();
        let installation = wow::scan(root.path()).unwrap();
        let eu = Market {
            game: Game::Retail,
            scope: "Silvermoon".into(),
            region: Some("EU".into()),
            region_only: false,
        };
        let catalog = load(
            root.path(),
            &installation,
            Some(&eu),
            std::slice::from_ref(&eu),
            Language::English,
        )
        .unwrap();
        assert_eq!(catalog.items.len(), 2);
        let realm = catalog.items.iter().find(|i| i.id == "i:100").unwrap();
        assert_eq!(realm.min_buyout.unwrap().amount, 12345);
        assert_eq!(realm.region_market.unwrap().amount, 17000);
        assert_eq!(realm.market.unwrap().timestamp, 1700000000);
        assert_eq!(realm.region_market.unwrap().timestamp, 1699900000);
        assert_eq!(realm.sale_rate.unwrap().amount, 125);
        assert_eq!(realm.sold_per_day.unwrap().amount, 1250);
        let commodity = catalog.items.iter().find(|i| i.id == "i:200").unwrap();
        assert!(commodity.commodity);
        assert_eq!(commodity.min_buyout.unwrap().amount, 250);
        assert_eq!(commodity.min_buyout.unwrap().timestamp, 1700000020);
        let unlinked = load(
            root.path(),
            &installation,
            Some(&eu),
            &[],
            Language::English,
        )
        .unwrap();
        assert_eq!(unlinked.items.len(), 1);
        assert_eq!(unlinked.items[0].region_market, None);
        let us = Market {
            game: Game::Retail,
            scope: "US".into(),
            region: Some("US".into()),
            region_only: true,
        };
        let region = load(
            root.path(),
            &installation,
            Some(&us),
            &[],
            Language::English,
        )
        .unwrap();
        assert!(
            region
                .items
                .iter()
                .find(|i| i.id == "i:100")
                .unwrap()
                .min_buyout
                .is_none()
        );
        assert_eq!(
            region
                .items
                .iter()
                .find(|i| i.id == "i:200")
                .unwrap()
                .min_buyout
                .unwrap()
                .amount,
            9999
        );
        let before = fs::read(helper.join("AppData.lua")).unwrap();
        let escaped = Market {
            game: Game::ClassicEra,
            ..eu
        };
        assert!(
            load(
                root.path(),
                &installation,
                Some(&escaped),
                &[],
                Language::English
            )
            .is_err()
        );
        assert_eq!(before, fs::read(helper.join("AppData.lua")).unwrap());
    }
    #[test]
    fn wrapper_tags_and_long_string_boundaries_are_verified() {
        let payload = include_str!("../../tests/fixtures/auction-prices.txt");
        let body = format!(
            "select(2, ...).LoadData(\"AUCTIONDB_NON_COMMODITY_DATA\",\"Realm\",[=[{payload}]=])"
        );
        assert!(parse_record("AUCTIONDB_NON_COMMODITY_DATA", "Realm", &body).is_ok());
        assert!(parse_record("AUCTIONDB_NON_COMMODITY_DATA", "Other", &body).is_err());
        assert!(
            parse_record(
                "AUCTIONDB_NON_COMMODITY_DATA",
                "Realm",
                &(body.clone() + "; attack()")
            )
            .is_err()
        );
        assert!(
            parse_record(
                "AUCTIONDB_NON_COMMODITY_DATA",
                "Realm",
                &body.replace("return ", "]=]; attack(); [=[return ")
            )
            .is_err()
        );
    }
    #[test]
    #[ignore = "Read-only local installation check, explicitly opted in with TSM_AUCTION_WOW_PATH"]
    fn local_auction_snapshot() {
        let root = std::env::var_os("TSM_AUCTION_WOW_PATH").expect("read-only WoW path");
        let root = Path::new(&root);
        let installation = wow::scan(root).unwrap();
        let data = load(root, &installation, None, &[], Language::English).unwrap();
        println!(
            "markets={}, items={}, names={}, categories={}, warnings={}",
            data.markets.len(),
            data.items.len(),
            data.items.iter().filter(|i| i.name.is_some()).count(),
            data.items.iter().filter(|i| i.class.is_some()).count(),
            data.warnings.len()
        );
        assert!(!data.items.is_empty());
        assert!(data.items.iter().any(|i| i.min_buyout.is_some()));
        assert!(data.items.iter().any(|i| i.region_market.is_some()));
    }
}
