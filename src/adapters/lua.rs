//! Parser for WoW's serialized SavedVariables literals. Never executes Lua.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Map, Value};

pub fn parse(input: &str) -> Result<Map<String, Value>> {
    ensure!(
        input.len() <= 128 * 1024 * 1024,
        msg!(SavedVariablesTooLarge)
    );
    let mut parser = Parser {
        bytes: input.as_bytes(),
        pos: 0,
    };
    let mut globals = Map::new();
    while parser.space() < parser.bytes.len() {
        let name = parser.identifier()?;
        parser.expect(b'=')?;
        globals.insert(name, parser.value(0)?);
        if parser.peek() == Some(b';') {
            parser.pos += 1;
        }
    }
    Ok(globals)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl Parser<'_> {
    fn space(&mut self) -> usize {
        loop {
            while self
                .bytes
                .get(self.pos)
                .is_some_and(u8::is_ascii_whitespace)
            {
                self.pos += 1;
            }
            if self.bytes.get(self.pos..self.pos + 2) == Some(b"--") {
                self.pos += 2;
                while self.bytes.get(self.pos).is_some_and(|b| *b != b'\n') {
                    self.pos += 1;
                }
            } else {
                return self.pos;
            }
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.space();
        self.bytes.get(self.pos).copied()
    }
    fn expect(&mut self, byte: u8) -> Result<()> {
        ensure!(
            self.peek() == Some(byte),
            msg!(InvalidLuaByte, offset = self.pos)
        );
        self.pos += 1;
        Ok(())
    }
    fn identifier(&mut self) -> Result<String> {
        self.space();
        let start = self.pos;
        ensure!(
            self.bytes
                .get(start)
                .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_'),
            msg!(LuaIdentifierExpected, offset = start)
        );
        while self
            .bytes
            .get(self.pos)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            self.pos += 1;
        }
        Ok(std::str::from_utf8(&self.bytes[start..self.pos])?.to_owned())
    }
    fn value(&mut self, depth: usize) -> Result<Value> {
        ensure!(depth < 128, msg!(LuaTooDeep));
        match self.peek() {
            Some(b'{') => {
                self.pos += 1;
                let mut table = Map::new();
                let mut index = 1;
                while self.peek() != Some(b'}') {
                    let key = if self.peek() == Some(b'[') {
                        self.pos += 1;
                        let key = self.value(depth + 1)?;
                        self.expect(b']')?;
                        self.expect(b'=')?;
                        match key {
                            Value::String(s) => s,
                            Value::Number(n) => n.to_string(),
                            _ => bail!(msg!(InvalidLuaKey)),
                        }
                    } else if self
                        .peek()
                        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                    {
                        let saved = self.pos;
                        let key = self.identifier()?;
                        if self.peek() == Some(b'=') {
                            self.pos += 1;
                            key
                        } else {
                            self.pos = saved;
                            let key = index.to_string();
                            index += 1;
                            key
                        }
                    } else {
                        let key = index.to_string();
                        index += 1;
                        key
                    };
                    let value = self.value(depth + 1)?;
                    if !value.is_null() {
                        table.insert(key, value);
                    }
                    match self.peek() {
                        Some(b',' | b';') => self.pos += 1,
                        Some(b'}') => (),
                        _ => bail!(msg!(LuaSeparatorExpected, offset = self.pos)),
                    }
                }
                self.pos += 1;
                Ok(Value::Object(table))
            }
            Some(b'"' | b'\'') => self.string().map(Value::String),
            Some(b'-' | b'0'..=b'9') => {
                self.space();
                let start = self.pos;
                while self
                    .bytes
                    .get(self.pos)
                    .is_some_and(|b| b.is_ascii_digit() || b"-+.eE".contains(b))
                {
                    self.pos += 1;
                }
                let s = std::str::from_utf8(&self.bytes[start..self.pos])?;
                let n = if let Ok(n) = s.parse::<i64>() {
                    n.into()
                } else {
                    serde_json::Number::from_f64(s.parse()?)
                        .ok_or_else(|| anyhow::anyhow!(msg!(InvalidNumber)))?
                };
                Ok(Value::Number(n))
            }
            _ => match self.identifier()?.as_str() {
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                "nil" => Ok(Value::Null),
                _ => bail!(msg!(ExecutableLua)),
            },
        }
    }
    fn string(&mut self) -> Result<String> {
        let quote = self.peek().unwrap();
        self.pos += 1;
        let mut result = Vec::new();
        loop {
            let b = *self.bytes.get(self.pos).context(msg!(UnclosedLuaString))?;
            self.pos += 1;
            if b == quote {
                break;
            }
            if b != b'\\' {
                result.push(b);
                continue;
            }
            let escape = *self
                .bytes
                .get(self.pos)
                .ok_or_else(|| anyhow::anyhow!(msg!(InvalidEscape)))?;
            self.pos += 1;
            match escape {
                b'n' => result.push(b'\n'),
                b'r' => result.push(b'\r'),
                b't' => result.push(b'\t'),
                b'a' => result.push(7),
                b'b' => result.push(8),
                b'f' => result.push(12),
                b'v' => result.push(11),
                b'\n' => result.push(b'\n'),
                b'\r' => {
                    if self.bytes.get(self.pos) == Some(&b'\n') {
                        self.pos += 1;
                    }
                    result.push(b'\n');
                }
                b'z' => {
                    while self
                        .bytes
                        .get(self.pos)
                        .is_some_and(u8::is_ascii_whitespace)
                    {
                        self.pos += 1;
                    }
                }
                b'0'..=b'9' => {
                    let mut n = (escape - b'0') as u16;
                    for _ in 0..2 {
                        if let Some(b'0'..=b'9') = self.bytes.get(self.pos) {
                            n = n * 10 + (self.bytes[self.pos] - b'0') as u16;
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                    ensure!(n <= 255, msg!(LuaEscapeRange));
                    result.push(n as u8);
                }
                b'x' => {
                    let end = self.pos + 2;
                    let hex = std::str::from_utf8(
                        self.bytes
                            .get(self.pos..end)
                            .ok_or_else(|| anyhow::anyhow!(msg!(InvalidHexEscape)))?,
                    )?;
                    result.push(u8::from_str_radix(hex, 16)?);
                    self.pos = end;
                }
                b'"' | b'\'' | b'\\' => result.push(escape),
                _ => bail!(msg!(UnknownLuaEscape)),
            }
        }
        Ok(String::from_utf8(result)?)
    }
}

pub fn values(value: &Value) -> Vec<&Value> {
    if let Some(a) = value.as_array() {
        return a.iter().collect();
    }
    if let Some(o) = value.as_object() {
        let mut entries: Vec<_> = o.iter().collect();
        entries.sort_by(
            |(a, _), (b, _)| match (a.parse::<u64>(), b.parse::<u64>()) {
                (Ok(a), Ok(b)) => a.cmp(&b),
                _ => a.cmp(b),
            },
        );
        return entries.into_iter().map(|(_, v)| v).collect();
    }
    Vec::new()
}

pub fn quote(s: &str) -> String {
    let mut result = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            c if c.is_control() => {
                for byte in c.to_string().bytes() {
                    result.push_str(&format!("\\{byte:03}"));
                }
            }
            c => result.push(c),
        }
    }
    result.push('"');
    result
}

pub fn prettify(db: &Value) -> Result<Value> {
    let mut result = serde_json::json!({"realm":{},"sync":{},"factionrealm":{},"global":{},"char":{},"profile":{}});
    for (key, value) in db.as_object().context(msg!(TsmDbNotTable))? {
        if key.starts_with('_') {
            if !["_hash", "_lastModifiedVersion"].contains(&key.as_str()) {
                result[key] = value.clone();
            }
            continue;
        }
        let parts: Vec<_> = key.split('@').collect();
        ensure!(parts.len() == 4, msg!(UnknownDbKey));
        let scope = match parts[0] {
            "r" => "realm",
            "s" => "sync",
            "f" => "factionrealm",
            "g" => "global",
            "c" => "char",
            "p" => "profile",
            _ => bail!(msg!(UnknownScope)),
        };
        let object = result[scope]
            .as_object_mut()
            .unwrap()
            .entry(parts[1])
            .or_insert_with(|| serde_json::json!({}));
        let ns = object
            .as_object_mut()
            .unwrap()
            .entry(parts[2])
            .or_insert_with(|| serde_json::json!({}));
        ns.as_object_mut()
            .unwrap()
            .insert(parts[3].to_owned(), value.clone());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_literals_not_code_and_preserves_order() {
        let parsed = parse("DB = { [\"text\"] = \"Grüße\\n\\002\\\"\", [\"list\"] = { [10] = 10, [2] = 2, [1] = 1 }, enabled=true, gone=nil } -- comment\nOther = 42").unwrap();
        assert_eq!(parsed["DB"]["text"], "Grüße\n\x02\"");
        assert_eq!(
            values(&parsed["DB"]["list"]),
            vec![
                &serde_json::json!(1),
                &serde_json::json!(2),
                &serde_json::json!(10)
            ]
        );
        assert!(parse("DB = os.execute('bad')").is_err());
        assert!(parse("DB = {x = \"\\999\"}").is_err());
        assert!(parse("DB = {x = 1").is_err());
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn savedvariables_fixture_and_native_string_escapes() {
        let globals = parse(include_str!("../../tests/fixtures/savedvariables.lua")).unwrap();
        let db = &globals["TradeSkillMasterDB"];
        assert_eq!(values(&db["_scopeKeys"]["realm"])[0], "Test Realm");
        let pretty = prettify(db).unwrap();
        assert_eq!(pretty["_version"], 7);
        assert!(pretty.get("_hash").is_none());
        assert!(pretty["realm"]["Test Realm"]["internalData"]["csvSales"].is_string());
        let original = "Grüße\0\n\x027\\\"";
        assert_eq!(
            parse(&format!("DB = {}", quote(original))).unwrap()["DB"],
            original
        );
    }
}
