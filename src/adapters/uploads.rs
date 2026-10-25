use crate::{
    adapters::{
        lua,
        tsm::{Api, Body, encoded},
        wow::{self, Account},
    },
    core::AccountingRecord,
};
use anyhow::{Context, Result, ensure};
use chrono::Utc;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, time::UNIX_EPOCH};

/// Upload account data using the original endpoint formats and lastUpload cursors.
pub async fn upload(api: &Api, account: &Account, system_id: &str) -> Result<usize> {
    let helper = account.data("TradeSkillMaster_AppHelper")?;
    let Some(region) = helper["region"].as_str() else {
        return Ok(0);
    };
    let account_name = encoded(&account.protocol_name());
    let tsm = account.data("TradeSkillMaster")?;
    let mut count = 0;
    for (field, endpoint) in [("analytics", "analytics"), ("errorReports", "error_report")] {
        let data = &helper[field];
        if let Some(time) = data["updateTime"].as_i64() {
            let strings: Vec<_> = lua::values(&data["data"])
                .iter()
                .filter_map(|v| v.as_str())
                .collect();
            if strings.is_empty() {
                continue;
            }
            let payload = format!("[{}]", strings.join(","));
            let payload = if endpoint == "analytics" {
                payload.replace('\\', "")
            } else {
                payload.replace("\\\"", "\"")
            };
            // Validate embedded JSON before handing it to the server.
            let _: Value = serde_json::from_str(&payload)?;
            count += api
                .upload_newer(&[endpoint, &account_name], time, Body::Text(payload))
                .await? as usize;
        }
    }
    if let Some(realms) = helper["blackMarket"].as_object() {
        for (realm, data) in realms {
            let time = data["updateTime"].as_i64().unwrap_or(0);
            if time < Utc::now().timestamp() - 86400 {
                continue;
            }
            count += api
                .upload_newer(
                    &["black_market", region, &encoded(realm)],
                    time,
                    Body::Json(data.clone()),
                )
                .await? as usize;
        }
    }
    if let Some(profiles) = helper["shoppingMaxPrices"].as_object() {
        for (profile, data) in profiles {
            let Some(time) = data["updateTime"].as_i64() else {
                continue;
            };
            let mut data = data.clone();
            data.as_object_mut()
                .context(msg!(InvalidShopping))?
                .remove("updateTime");
            let payload = json!({"data":data,"updateTime":time,"profiles":profiles.keys().collect::<Vec<_>>()});
            count += api
                .upload_newer(
                    &["groups", &account_name, &encoded(profile)],
                    time,
                    Body::Json(payload),
                )
                .await? as usize;
        }
    }
    for realm in wow::scopes(&tsm, "realm") {
        let segments = ["sales", region, &encoded(&realm), &account_name];
        let last = api.get(&segments).await?["lastUpload"]
            .as_i64()
            .context(msg!(MissingSalesCursor))?;
        let records = sales_records(&tsm, &realm, last)?;
        if !records.is_empty() {
            api.post(
                &segments,
                Body::Json(Value::Array(records.iter().map(record_wire).collect())),
            )
            .await?;
            count += 1;
        }
    }
    for factionrealm in wow::scopes(&tsm, "factionrealm") {
        if let Some(payload) = scan_payload(&tsm, &factionrealm)? {
            let Some((faction, realm)) = factionrealm.split_once(" - ") else {
                continue;
            };
            let realm = if account.game.faction_realms() {
                format!("{realm}-{faction}")
            } else {
                realm.to_owned()
            };
            let time = payload["last_modified"].as_i64().unwrap_or(0);
            count += api
                .upload_newer(
                    &["auctiondb_scan", region, &encoded(&realm)],
                    time,
                    Body::Json(payload),
                )
                .await? as usize;
        }
    }
    if !tsm.is_null() {
        let time = fs::metadata(account.path.join("TradeSkillMaster.lua"))?
            .modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs() as i64;
        let payload = json!({"tsmDB":lua::prettify(&tsm)?,"appHelperDB":helper,"timestamp":time,"systemId":system_id});
        count += api
            .upload_newer(
                &["sv", region, &account_name],
                time,
                Body::GzippedJsonBinary(payload),
            )
            .await? as usize;
    }
    Ok(count)
}

pub fn sales_records(tsm: &Value, realm: &str, last: i64) -> Result<Vec<AccountingRecord>> {
    let mut result = Vec::new();
    for (csv_key, time_key, kind, price) in [
        ("csvSales", "saveTimeSales", 2, true),
        ("csvBuys", "saveTimeBuys", 3, true),
        ("csvExpired", "saveTimeExpires", 4, false),
        ("csvCancelled", "saveTimeCancels", 5, false),
    ] {
        let Some(csv) = tsm[format!("r@{realm}@internalData@{csv_key}")].as_str() else {
            continue;
        };
        let Some(times) = tsm[format!("r@{realm}@internalData@{time_key}")].as_str() else {
            continue;
        };
        let records: Vec<_> = wow::csv_records(csv)?
            .into_iter()
            .filter(|r| !price || r.get("source").is_some_and(|s| s == "Auction"))
            .collect();
        let times = times
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::parse::<i64>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure!(records.len() == times.len(), msg!(AccountingTimes));
        for (row, save_time) in records.iter().zip(times) {
            let Some(item) = row
                .get("itemString")
                .and_then(|s| s.strip_prefix("i:"))
                .and_then(|s| s.split(':').next())
            else {
                continue;
            };
            let number = |key: &str| -> Result<i64> {
                Ok(row
                    .get(key)
                    .context(msg!(MissingAccountingColumn))?
                    .parse()?)
            };
            let record = AccountingRecord {
                item_id: item.parse()?,
                price: if price { number("price")? } else { 0 },
                stack_size: number("stackSize")?,
                quantity: number("quantity")?,
                occurred_at: number("time")?,
                saved_at: save_time,
                kind,
            };
            if record.newer_than(last) {
                result.push(record);
            }
        }
    }
    Ok(result)
}
fn record_wire(record: &AccountingRecord) -> Value {
    json!([
        record.item_id,
        record.price,
        record.stack_size,
        record.quantity,
        record.occurred_at,
        record.saved_at,
        record.kind
    ])
}
pub fn scan_payload(tsm: &Value, factionrealm: &str) -> Result<Option<Value>> {
    scan_payload_at(tsm, factionrealm, Utc::now().timestamp())
}
fn scan_payload_at(tsm: &Value, factionrealm: &str, now: i64) -> Result<Option<Value>> {
    let prefix = format!("f@{factionrealm}@internalData@");
    let Some(csv) = tsm[format!("{prefix}csvAuctionDBScan")].as_str() else {
        return Ok(None);
    };
    let time = tsm[format!("{prefix}auctionDBScanTime")]
        .as_i64()
        .unwrap_or(0);
    if time < now - 86400 {
        return Ok(None);
    }
    let hash = csv
        .replace("\\n", "\n")
        .bytes()
        .fold(5381u32, |h, b| (h * 33 + b as u32) % (1 << 24));
    ensure!(
        Some(hash as u64) == tsm[format!("{prefix}auctionDBScanHash")].as_u64(),
        msg!(ScanChecksum)
    );
    let mut items = BTreeMap::new();
    for row in wow::csv_records(csv)? {
        // Match original: records with lastScan equal to the scan timestamp are excluded.
        if row.get("lastScan").and_then(|s| s.parse::<i64>().ok()) == Some(time) {
            continue;
        }
        let Some(item) = row
            .get("itemString")
            .and_then(|s| s.strip_prefix("i:"))
            .filter(|s| !s.contains(':'))
        else {
            continue;
        };
        let number = |key: &str| -> Result<i64> {
            let n: f64 = row.get(key).context(msg!(MissingScanColumn))?.parse()?;
            ensure!(
                n.is_finite() && n >= 0.0 && n <= i64::MAX as f64,
                msg!(InvalidScanValue)
            );
            Ok(n.floor() as i64)
        };
        items.insert(item.to_owned(), json!({"min_buyout":number("minBuyout")?,"market_value":number("marketValue")?,"num_auctions":number("numAuctions")?,"quantity":number("quantity")?}));
    }
    if items.is_empty() {
        return Ok(None);
    }
    Ok(Some(json!({"items":items,"last_modified":time})))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accounting_cursor_filters_and_preserves_protocol_columns() {
        let db = json!({"r@Realm@internalData@csvSales":"itemString,price,stackSize,quantity,time,source\\ni:42,100,1,2,90,Auction\\ni:43,200,1,1,91,Auction", "r@Realm@internalData@saveTimeSales":"100,101"});
        assert_eq!(
            sales_records(&db, "Realm", 100)
                .unwrap()
                .iter()
                .map(record_wire)
                .collect::<Vec<_>>(),
            vec![json!([43, 200, 1, 1, 91, 101, 2])]
        );
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn accounting_fixture_matches_four_legacy_record_types() {
        let globals = lua::parse(include_str!("../../tests/fixtures/savedvariables.lua")).unwrap();
        let actual: Vec<_> = sales_records(&globals["TradeSkillMasterDB"], "Test Realm", 100)
            .unwrap()
            .iter()
            .map(record_wire)
            .collect();
        let expected: Vec<Value> =
            serde_json::from_str(include_str!("../../tests/fixtures/accounting-wire.json"))
                .unwrap();
        assert_eq!(actual, expected);
        assert!(
            sales_records(&globals["TradeSkillMasterDB"], "Test Realm", 104)
                .unwrap()
                .is_empty()
        );
    }
}

#[cfg(test)]
mod scan_fixtures {
    use super::*;
    #[test]
    fn fixed_scan_checksum_age_and_legacy_filters() {
        let mut db: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/scan.json")).unwrap();
        let expected: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/scan-wire.json")).unwrap();
        assert_eq!(
            scan_payload_at(&db, "Alliance - Test Realm", 1700000100).unwrap(),
            Some(expected)
        );
        assert!(
            scan_payload_at(&db, "Alliance - Test Realm", 1700086401)
                .unwrap()
                .is_none()
        );
        db["f@Alliance - Test Realm@internalData@auctionDBScanHash"] = json!(0);
        assert!(scan_payload_at(&db, "Alliance - Test Realm", 1700000100).is_err());
    }
}
