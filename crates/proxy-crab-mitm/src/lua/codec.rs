use std::{
    collections::{BTreeMap, HashSet},
    fmt,
    io::{self, Write},
    sync::{Arc, Mutex},
};

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD},
};
use mlua::{AnyUserData, Error as LuaError, Lua, String as LuaString, Table, UserData, Value};
use serde::de::{DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor};

const CODEC_BYTE_LIMIT: usize = 16 * 1024 * 1024;
const JSON_DEPTH_LIMIT: usize = 128;
const JSON_KIND_FIELD: &str = "__proxycrab_json_kind_v1";
const I64_MAX_PLUS_ONE: f64 = 9_223_372_036_854_775_808.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct JsonWarningCounts {
    pub lossy_numbers: usize,
    pub duplicate_keys: usize,
}

impl JsonWarningCounts {
    pub fn is_empty(self) -> bool {
        self.lossy_numbers == 0 && self.duplicate_keys == 0
    }
}

#[derive(Clone, Default)]
pub(super) struct JsonWarningState(Arc<Mutex<JsonWarningCounts>>);

impl JsonWarningState {
    pub fn take(&self) -> JsonWarningCounts {
        std::mem::take(&mut *self.0.lock().expect("Lua JSON warning lock poisoned"))
    }

    fn add(&self, counts: JsonWarningCounts) {
        let mut current = self.0.lock().expect("Lua JSON warning lock poisoned");
        current.lossy_numbers += counts.lossy_numbers;
        current.duplicate_keys += counts.duplicate_keys;
    }

    fn note_lossy_number(&self) {
        self.0
            .lock()
            .expect("Lua JSON warning lock poisoned")
            .lossy_numbers += 1;
    }

    fn note_duplicate_key(&self) {
        self.0
            .lock()
            .expect("Lua JSON warning lock poisoned")
            .duplicate_keys += 1;
    }
}

#[derive(Clone, Copy)]
struct JsonNull;

impl UserData for JsonNull {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsonTableKind {
    Array,
    Object,
}

impl JsonTableKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Array => "array",
            Self::Object => "object",
        }
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(untagged)]
enum JsonValue {
    Null,
    Boolean(bool),
    Integer(i64),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}

pub(super) fn install(lua: &Lua) -> mlua::Result<JsonWarningState> {
    install_base64(lua)?;
    let warnings = JsonWarningState::default();
    install_json(lua, warnings.clone())?;
    Ok(warnings)
}

fn install_base64(lua: &Lua) -> mlua::Result<()> {
    let module = lua.create_table()?;
    module.set(
        "encode",
        lua.create_function(|lua, data: LuaString| {
            ensure_input_limit(data.as_bytes().len(), "Base64")?;
            let output_len = base64::encoded_len(data.as_bytes().len(), true)
                .ok_or_else(|| LuaError::runtime("Base64 output size overflow"))?;
            ensure_output_limit(output_len, "Base64")?;
            lua.create_string(STANDARD.encode(data.as_bytes()))
        })?,
    )?;
    module.set(
        "decode",
        lua.create_function(|lua, text: LuaString| {
            ensure_input_limit(text.as_bytes().len(), "Base64")?;
            let decoded = STANDARD
                .decode(text.as_bytes())
                .map_err(|error| LuaError::runtime(format!("invalid standard Base64: {error}")))?;
            ensure_output_limit(decoded.len(), "Base64")?;
            lua.create_string(decoded)
        })?,
    )?;
    module.set(
        "url_encode",
        lua.create_function(|lua, (data, with_padding): (LuaString, Value)| {
            let Value::Boolean(with_padding) = with_padding else {
                return Err(LuaError::runtime(
                    "base64.url_encode requires a boolean with_padding argument",
                ));
            };
            ensure_input_limit(data.as_bytes().len(), "Base64")?;
            let output_len = base64::encoded_len(data.as_bytes().len(), with_padding)
                .ok_or_else(|| LuaError::runtime("Base64 output size overflow"))?;
            ensure_output_limit(output_len, "Base64")?;
            let encoded = if with_padding {
                URL_SAFE.encode(data.as_bytes())
            } else {
                URL_SAFE_NO_PAD.encode(data.as_bytes())
            };
            lua.create_string(encoded)
        })?,
    )?;
    module.set(
        "url_decode",
        lua.create_function(|lua, text: LuaString| {
            ensure_input_limit(text.as_bytes().len(), "Base64")?;
            let engine = if text.as_bytes().contains(&b'=') {
                &URL_SAFE
            } else {
                &URL_SAFE_NO_PAD
            };
            let decoded = engine
                .decode(text.as_bytes())
                .map_err(|error| LuaError::runtime(format!("invalid URL-safe Base64: {error}")))?;
            ensure_output_limit(decoded.len(), "Base64")?;
            lua.create_string(decoded)
        })?,
    )?;
    lua.globals()
        .set("base64", read_only_module(lua, module, "base64")?)
}

fn install_json(lua: &Lua, warnings: JsonWarningState) -> mlua::Result<()> {
    let module = lua.create_table()?;
    let null = lua.create_userdata(JsonNull)?;
    module.set("null", null.clone())?;
    module.set(
        "encode",
        lua.create_function(move |lua, value: Value| json_encode(lua, value))?,
    )?;

    let decode_null = null.clone();
    let decode_warnings = warnings.clone();
    module.set(
        "decode",
        lua.create_function(move |lua, text: LuaString| {
            json_decode(lua, text, &decode_null, &decode_warnings)
        })?,
    )?;
    module.set(
        "array",
        lua.create_function(|lua, table: Table| {
            mark_table(lua, &table, JsonTableKind::Array, true)?;
            Ok(table)
        })?,
    )?;
    module.set(
        "object",
        lua.create_function(|lua, table: Table| {
            mark_table(lua, &table, JsonTableKind::Object, true)?;
            Ok(table)
        })?,
    )?;
    lua.globals()
        .set("json", read_only_module(lua, module, "json")?)?;
    Ok(())
}

fn read_only_module(lua: &Lua, backing: Table, name: &'static str) -> mlua::Result<Table> {
    let proxy = lua.create_table()?;
    let metatable = lua.create_table()?;
    metatable.set("__index", backing)?;
    metatable.set(
        "__newindex",
        lua.create_function(move |_, (_table, _key, _value): (Value, Value, Value)| {
            Err::<(), _>(LuaError::runtime(format!(
                "attempt to modify read-only {name} module"
            )))
        })?,
    )?;
    metatable.set("__metatable", false)?;
    proxy.set_metatable(Some(metatable))?;
    Ok(proxy)
}

fn ensure_input_limit(size: usize, label: &str) -> mlua::Result<()> {
    if size > CODEC_BYTE_LIMIT {
        return Err(LuaError::runtime(format!(
            "{label} input exceeds the 16 MiB limit"
        )));
    }
    Ok(())
}

fn ensure_output_limit(size: usize, label: &str) -> mlua::Result<()> {
    if size > CODEC_BYTE_LIMIT {
        return Err(LuaError::runtime(format!(
            "{label} output exceeds the 16 MiB limit"
        )));
    }
    Ok(())
}

fn json_decode(
    lua: &Lua,
    text: LuaString,
    null: &AnyUserData,
    warnings: &JsonWarningState,
) -> mlua::Result<Value> {
    ensure_input_limit(text.as_bytes().len(), "JSON")?;
    let text = text
        .to_str()
        .map_err(|_| LuaError::runtime("JSON input must be valid UTF-8"))?;
    let mut deserializer = serde_json::Deserializer::from_str(&text);
    let decode_warnings = JsonWarningState::default();
    let value = JsonSeed {
        depth: 0,
        warnings: decode_warnings.clone(),
    }
    .deserialize(&mut deserializer)
    .map_err(|error| LuaError::runtime(format!("invalid JSON: {error}")))?;
    deserializer
        .end()
        .map_err(|error| LuaError::runtime(format!("invalid JSON: {error}")))?;
    let value = json_value_to_lua(lua, value, null)?;
    warnings.add(decode_warnings.take());
    Ok(value)
}

fn json_encode(lua: &Lua, value: Value) -> mlua::Result<LuaString> {
    let value = lua_value_to_json(
        value,
        0,
        &mut HashSet::new(),
        &mut JsonEncodeBudget::default(),
    )?;
    let mut writer = LimitedWriter::new(CODEC_BYTE_LIMIT);
    serde_json::to_writer(&mut writer, &value)
        .map_err(|error| LuaError::runtime(format!("failed to encode JSON: {error}")))?;
    lua.create_string(writer.into_inner())
}

fn json_value_to_lua(lua: &Lua, value: JsonValue, null: &AnyUserData) -> mlua::Result<Value> {
    match value {
        JsonValue::Null => Ok(Value::UserData(null.clone())),
        JsonValue::Boolean(value) => Ok(Value::Boolean(value)),
        JsonValue::Integer(value) => Ok(Value::Integer(value)),
        JsonValue::Number(value) => Ok(Value::Number(value)),
        JsonValue::String(value) => Ok(Value::String(lua.create_string(value)?)),
        JsonValue::Array(values) => {
            let table = lua.create_table_with_capacity(values.len(), 0)?;
            for (index, value) in values.into_iter().enumerate() {
                table.raw_set(index + 1, json_value_to_lua(lua, value, null)?)?;
            }
            mark_table(lua, &table, JsonTableKind::Array, false)?;
            Ok(Value::Table(table))
        }
        JsonValue::Object(values) => {
            let table = lua.create_table_with_capacity(0, values.len())?;
            for (key, value) in values {
                table.raw_set(key, json_value_to_lua(lua, value, null)?)?;
            }
            mark_table(lua, &table, JsonTableKind::Object, false)?;
            Ok(Value::Table(table))
        }
    }
}

fn lua_value_to_json(
    value: Value,
    depth: usize,
    active_tables: &mut HashSet<usize>,
    budget: &mut JsonEncodeBudget,
) -> mlua::Result<JsonValue> {
    match value {
        Value::Nil => {
            budget.charge(4)?;
            Ok(JsonValue::Null)
        }
        Value::Boolean(value) => {
            budget.charge(4)?;
            Ok(JsonValue::Boolean(value))
        }
        Value::Integer(value) => {
            budget.charge(1)?;
            Ok(JsonValue::Integer(value))
        }
        Value::Number(value) if value.is_finite() => {
            budget.charge(1)?;
            Ok(JsonValue::Number(value))
        }
        Value::Number(_) => Err(LuaError::runtime(
            "JSON cannot encode NaN or infinite numbers",
        )),
        Value::String(value) => {
            let value = value
                .to_str()
                .map_err(|_| LuaError::runtime("JSON strings must be valid UTF-8"))?;
            budget.charge(value.len().saturating_add(2))?;
            Ok(JsonValue::String(value.to_string()))
        }
        Value::Table(table) => table_to_json(table, depth, active_tables, budget),
        Value::UserData(value) if value.is::<JsonNull>() => {
            budget.charge(4)?;
            Ok(JsonValue::Null)
        }
        other => Err(LuaError::runtime(format!(
            "JSON cannot encode Lua {} values",
            other.type_name()
        ))),
    }
}

fn table_to_json(
    table: Table,
    depth: usize,
    active_tables: &mut HashSet<usize>,
    budget: &mut JsonEncodeBudget,
) -> mlua::Result<JsonValue> {
    if depth >= JSON_DEPTH_LIMIT {
        return Err(LuaError::runtime(
            "JSON value exceeds the maximum nesting depth of 128",
        ));
    }
    let pointer = table.to_pointer() as usize;
    if !active_tables.insert(pointer) {
        return Err(LuaError::runtime("JSON cannot encode cyclic tables"));
    }
    let result = table_to_json_inner(table, depth, active_tables, budget);
    active_tables.remove(&pointer);
    result
}

fn table_to_json_inner(
    table: Table,
    depth: usize,
    active_tables: &mut HashSet<usize>,
    budget: &mut JsonEncodeBudget,
) -> mlua::Result<JsonValue> {
    let marked_kind = table_kind(&table)?;
    let mut integer_values = BTreeMap::<usize, Value>::new();
    let mut string_values = BTreeMap::<String, Value>::new();

    for pair in table.clone().pairs::<Value, Value>() {
        let (key, value) = pair?;
        match key {
            Value::Integer(key) if key > 0 => {
                let key = usize::try_from(key)
                    .map_err(|_| LuaError::runtime("JSON array index is too large"))?;
                integer_values.insert(key, value);
            }
            Value::String(key) => {
                let key = key
                    .to_str()
                    .map_err(|_| LuaError::runtime("JSON object keys must be valid UTF-8"))?;
                budget.charge(key.len().saturating_add(3))?;
                let key = key.to_string();
                string_values.insert(key, value);
            }
            _ => {
                return Err(LuaError::runtime(
                    "JSON table keys must be positive integers or strings",
                ));
            }
        }
    }

    let kind = match marked_kind {
        Some(kind) => kind,
        None if integer_values.is_empty() && string_values.is_empty() => JsonTableKind::Object,
        None if string_values.is_empty() => JsonTableKind::Array,
        None if integer_values.is_empty() => JsonTableKind::Object,
        None => {
            return Err(LuaError::runtime(
                "JSON cannot encode tables with mixed integer and string keys",
            ));
        }
    };

    match kind {
        JsonTableKind::Array => {
            budget.charge(2usize.saturating_add(integer_values.len()))?;
            if !string_values.is_empty() {
                return Err(LuaError::runtime("JSON arrays cannot contain string keys"));
            }
            let length = integer_values.len();
            if integer_values.keys().copied().ne(1..=length) {
                return Err(LuaError::runtime(
                    "JSON arrays must use consecutive indexes starting at 1",
                ));
            }
            let mut values = Vec::with_capacity(length);
            for (_, value) in integer_values {
                values.push(lua_value_to_json(value, depth + 1, active_tables, budget)?);
            }
            Ok(JsonValue::Array(values))
        }
        JsonTableKind::Object => {
            budget.charge(2usize.saturating_add(string_values.len()))?;
            if !integer_values.is_empty() {
                return Err(LuaError::runtime(
                    "JSON objects cannot contain integer keys",
                ));
            }
            let mut values = BTreeMap::new();
            for (key, value) in string_values {
                values.insert(
                    key,
                    lua_value_to_json(value, depth + 1, active_tables, budget)?,
                );
            }
            Ok(JsonValue::Object(values))
        }
    }
}

#[derive(Default)]
struct JsonEncodeBudget {
    estimated_output_bytes: usize,
}

impl JsonEncodeBudget {
    fn charge(&mut self, size: usize) -> mlua::Result<()> {
        self.estimated_output_bytes = self
            .estimated_output_bytes
            .checked_add(size)
            .ok_or_else(|| LuaError::runtime("JSON output size overflow"))?;
        ensure_output_limit(self.estimated_output_bytes, "JSON")
    }
}

fn mark_table(
    lua: &Lua,
    table: &Table,
    kind: JsonTableKind,
    reject_existing: bool,
) -> mlua::Result<()> {
    if reject_existing && table.metatable().is_some() {
        return Err(LuaError::runtime(
            "json.array/json.object cannot mark a table that already has a metatable",
        ));
    }
    let metatable = lua.create_table()?;
    metatable.raw_set(JSON_KIND_FIELD, kind.as_str())?;
    metatable.raw_set("__metatable", false)?;
    table.set_metatable(Some(metatable))
}

fn table_kind(table: &Table) -> mlua::Result<Option<JsonTableKind>> {
    let Some(metatable) = table.metatable() else {
        return Ok(None);
    };
    match metatable
        .raw_get::<Option<String>>(JSON_KIND_FIELD)?
        .as_deref()
    {
        Some("array") => Ok(Some(JsonTableKind::Array)),
        Some("object") => Ok(Some(JsonTableKind::Object)),
        _ => Ok(None),
    }
}

struct JsonSeed {
    depth: usize,
    warnings: JsonWarningState,
}

impl<'de> DeserializeSeed<'de> for JsonSeed {
    type Value = JsonValue;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(JsonVisitor {
            depth: self.depth,
            warnings: self.warnings,
        })
    }
}

struct JsonVisitor {
    depth: usize,
    warnings: JsonWarningState,
}

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = JsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(JsonValue::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(JsonValue::Boolean(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(JsonValue::Integer(value))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if let Ok(value) = i64::try_from(value) {
            return Ok(JsonValue::Integer(value));
        }
        self.warnings.note_lossy_number();
        Ok(JsonValue::Number(value as f64))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if !value.is_finite() {
            return Err(E::custom("JSON number is outside the finite f64 range"));
        }
        if value.fract() == 0.0 && (value < i64::MIN as f64 || value >= I64_MAX_PLUS_ONE) {
            self.warnings.note_lossy_number();
        }
        Ok(JsonValue::Number(value))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(JsonValue::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(JsonValue::String(value))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        if self.depth >= JSON_DEPTH_LIMIT {
            return Err(A::Error::custom(
                "JSON value exceeds the maximum nesting depth of 128",
            ));
        }
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(JsonSeed {
            depth: self.depth + 1,
            warnings: self.warnings.clone(),
        })? {
            values.push(value);
        }
        Ok(JsonValue::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        if self.depth >= JSON_DEPTH_LIMIT {
            return Err(A::Error::custom(
                "JSON value exceeds the maximum nesting depth of 128",
            ));
        }
        let mut values = BTreeMap::new();
        while let Some(key) = map.next_key::<String>()? {
            let value = map.next_value_seed(JsonSeed {
                depth: self.depth + 1,
                warnings: self.warnings.clone(),
            })?;
            if values.insert(key, value).is_some() {
                self.warnings.note_duplicate_key();
            }
        }
        Ok(JsonValue::Object(values))
    }
}

struct LimitedWriter {
    bytes: Vec<u8>,
    limit: usize,
}

impl LimitedWriter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for LimitedWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let next_len = self
            .bytes
            .len()
            .checked_add(buffer.len())
            .ok_or_else(|| io::Error::other("JSON output size overflow"))?;
        if next_len > self.limit {
            return Err(io::Error::other("JSON output exceeds the 16 MiB limit"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
