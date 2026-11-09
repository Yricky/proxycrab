//! Editable snapshots of immutable byte ranges. Only explicit submission changes a Body.
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use super::{LuaError, MEMORY_LIMIT};
use crate::storage::{BodySource, BodySourceData};
use mlua::{
    FromLua, IntoLua, Lua, LuaString, MetaMethod, MultiValue, UserData, UserDataMethods, Value,
    WeakLua,
};

const BLOCK: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub(super) enum Source {
    Memory(Arc<[u8]>),
    File(Arc<ReadFile>),
}

#[derive(Debug)]
pub(super) struct ReadFile {
    file: Mutex<File>,
    pub len: usize,
    path: std::path::PathBuf,
    temporary: Option<Arc<tempfile::TempPath>>,
}

impl Source {
    pub fn file(path: &Path) -> mlua::Result<Self> {
        Self::open_file(path, None)
    }

    fn open_file(path: &Path, temporary: Option<Arc<tempfile::TempPath>>) -> mlua::Result<Self> {
        let file = File::open(path).map_err(LuaError::external)?;
        let len = usize::try_from(file.metadata().map_err(LuaError::external)?.len())
            .map_err(LuaError::external)?;
        if len > i64::MAX as usize {
            return Err(LuaError::runtime("bytes length exceeds Lua integer range"));
        }
        Ok(Self::File(Arc::new(ReadFile {
            file: Mutex::new(file),
            len,
            path: path.to_owned(),
            temporary,
        })))
    }

    pub fn temporary(file: tempfile::NamedTempFile) -> mlua::Result<Self> {
        let path = Arc::new(file.into_temp_path());
        Self::open_file(&path, Some(path.clone()))
    }

    pub fn body(source: BodySource) -> mlua::Result<Self> {
        let mut result = match source.data {
            BodySourceData::Bytes(bytes) => Self::Memory(bytes.into()),
            BodySourceData::File(path) => Self::file(&path)?,
            BodySourceData::TemporaryFile(path) => Self::open_file(&path, Some(path.clone()))?,
        };
        if !source.content_encodings.is_empty() {
            let mut reader: Box<dyn Read> = match &result {
                Self::Memory(bytes) => Box::new(io::Cursor::new(bytes.clone())),
                Self::File(file) => Box::new(File::open(&file.path).map_err(LuaError::external)?),
            };
            for encoding in source.content_encodings.iter().rev() {
                reader = match encoding.as_str() {
                    "identity" => reader,
                    "gzip" => Box::new(flate2::read::GzDecoder::new(reader)),
                    "deflate" => Box::new(flate2::read::ZlibDecoder::new(reader)),
                    "br" => Box::new(brotli::Decompressor::new(reader, BLOCK)),
                    "zstd" => Box::new(
                        zstd::stream::read::Decoder::new(reader).map_err(LuaError::external)?,
                    ),
                    _ => return Err(LuaError::runtime("unsupported body encoding")),
                };
            }
            let mut file = tempfile::NamedTempFile::new().map_err(LuaError::external)?;
            io::copy(&mut reader, &mut file).map_err(LuaError::external)?;
            result = Self::temporary(file)?;
        }
        Ok(result)
    }

    pub fn view(&self, lua: &Lua) -> mlua::Result<BytesView> {
        match self {
            Self::Memory(bytes) => BytesView::string(lua, lua.create_string(bytes.as_ref())?),
            Self::File(file) => BytesView::pieces(
                lua,
                std::iter::once(Piece {
                    backing: Backing::File(file.clone()),
                    start: 0,
                    len: file.len,
                }),
            ),
        }
    }

    pub fn data(&self) -> BodySourceData {
        match self {
            Self::Memory(bytes) => BodySourceData::Bytes(bytes.to_vec()),
            Self::File(file) => match &file.temporary {
                Some(path) => BodySourceData::TemporaryFile(path.clone()),
                None => BodySourceData::File(file.path.clone()),
            },
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Memory(bytes) => bytes.len(),
            Self::File(file) => file.len,
        }
    }
}

// Native range metadata and temporary buffers share the Lua sandbox's memory budget.
// Payload edits themselves are Lua strings and are already charged by its allocator.
#[derive(Clone, Default)]
struct NativeMemory(Arc<AtomicUsize>);
struct Charge {
    lua: WeakLua,
    memory: NativeMemory,
    size: usize,
}
impl Charge {
    fn new(lua: &Lua, size: usize) -> mlua::Result<Self> {
        let memory = lua
            .app_data_ref::<NativeMemory>()
            .expect("bytes budget installed")
            .clone();
        let used = memory.0.load(Ordering::Relaxed);
        let limit = MEMORY_LIMIT
            .checked_sub(used)
            .and_then(|n| n.checked_sub(size))
            .filter(|&n| n > lua.used_memory())
            .ok_or_else(|| LuaError::runtime("bytes exceeds sandbox memory budget"))?;
        lua.set_memory_limit(limit)?;
        memory.0.fetch_add(size, Ordering::Relaxed);
        Ok(Self {
            lua: lua.weak(),
            memory,
            size,
        })
    }
}
impl Drop for Charge {
    fn drop(&mut self) {
        let previous = self.memory.0.fetch_sub(self.size, Ordering::Relaxed);
        if let Some(lua) = self.lua.try_upgrade() {
            let _ = lua.set_memory_limit(MEMORY_LIMIT - (previous - self.size));
        }
    }
}

#[derive(Clone)]
enum Backing {
    String(LuaString),
    File(Arc<ReadFile>),
}
#[derive(Clone)]
struct Piece {
    backing: Backing,
    start: usize,
    len: usize,
}
struct Pieces {
    items: Vec<Piece>,
    _charge: Charge,
}
#[derive(Clone)]
pub(super) struct BytesView {
    pieces: Arc<Pieces>,
    pub len: usize,
}

impl BytesView {
    fn pieces(lua: &Lua, pieces: impl Iterator<Item = Piece> + Clone) -> mlua::Result<Self> {
        let count = pieces.clone().count();
        let charge = Charge::new(
            lua,
            count * std::mem::size_of::<Piece>()
                + std::mem::size_of::<Pieces>()
                + 2 * std::mem::size_of::<usize>(),
        )?;
        let mut items = Vec::with_capacity(count);
        items.extend(pieces);
        let len = items
            .iter()
            .try_fold(0usize, |n, p| n.checked_add(p.len))
            .filter(|&n| n <= i64::MAX as usize)
            .ok_or_else(|| LuaError::runtime("bytes length overflow"))?;
        Ok(Self {
            pieces: Arc::new(Pieces {
                items,
                _charge: charge,
            }),
            len,
        })
    }

    pub fn string(lua: &Lua, value: LuaString) -> mlua::Result<Self> {
        let len = value.as_bytes().len();
        Self::pieces(
            lua,
            std::iter::once(Piece {
                backing: Backing::String(value),
                start: 0,
                len,
            }),
        )
    }

    pub fn input(lua: &Lua, value: Value) -> mlua::Result<Self> {
        match value {
            Value::String(text) => Self::string(lua, text),
            Value::UserData(data) => Ok(data.borrow::<Self>()?.clone()),
            _ => Err(LuaError::runtime("expected a binary string or bytes view")),
        }
    }

    fn ranges(&self, start: usize, end: usize) -> impl Iterator<Item = Piece> + Clone {
        self.pieces
            .items
            .iter()
            .scan(0, move |offset, piece| {
                let lo = start.saturating_sub(*offset).min(piece.len);
                let hi = end.saturating_sub(*offset).min(piece.len);
                *offset += piece.len;
                Some((lo < hi).then(|| Piece {
                    backing: piece.backing.clone(),
                    start: piece.start + lo,
                    len: hi - lo,
                }))
            })
            .flatten()
    }

    fn slice(&self, lua: &Lua, start: usize, end: usize) -> mlua::Result<Self> {
        Self::pieces(lua, self.ranges(start, end))
    }

    fn edit(&mut self, lua: &Lua, pos: usize, delete: usize, data: Self) -> mlua::Result<()> {
        if pos > self.len || delete > self.len - pos {
            return Err(LuaError::runtime("bytes write out of bounds"));
        }
        let pieces = self
            .ranges(0, pos)
            .chain(data.pieces.items.iter().filter(|p| p.len > 0).cloned())
            .chain(self.ranges(pos + delete, self.len));
        let next = Self::pieces(lua, pieces)?;
        *self = next;
        Ok(())
    }

    pub fn read(&self, pos: usize, output: &mut [u8]) -> mlua::Result<()> {
        if pos > self.len || output.len() > self.len - pos {
            return Err(LuaError::runtime("bytes read out of bounds"));
        }
        let mut offset = 0;
        let mut copied = 0;
        for piece in &self.pieces.items {
            let lo = pos.saturating_sub(offset).min(piece.len);
            let count = (piece.len - lo).min(output.len() - copied);
            if count > 0 {
                let target = &mut output[copied..copied + count];
                match &piece.backing {
                    Backing::String(text) => target.copy_from_slice(
                        &text.as_bytes()[piece.start + lo..piece.start + lo + count],
                    ),
                    Backing::File(source) => {
                        let mut file = source.file.lock().expect("bytes file lock");
                        file.seek(SeekFrom::Start((piece.start + lo) as u64))
                            .map_err(LuaError::external)?;
                        file.read_exact(target).map_err(LuaError::external)?;
                    }
                }
                copied += count;
            }
            offset += piece.len;
            if copied == output.len() {
                break;
            }
        }
        Ok(())
    }

    pub fn to_string(&self, lua: &Lua) -> mlua::Result<LuaString> {
        if self.len > MEMORY_LIMIT {
            return Err(LuaError::runtime(
                "bytes result exceeds sandbox memory budget; slice it first",
            ));
        }
        let _charge = Charge::new(lua, self.len)?;
        let mut bytes = vec![0; self.len];
        self.read(0, &mut bytes)?;
        lua.create_string(bytes)
    }

    pub fn copy_to(&self, writer: &mut dyn Write) -> mlua::Result<()> {
        let mut buffer = vec![0; BLOCK.min(self.len)];
        let mut pos = 0;
        while pos < self.len {
            let count = buffer.len().min(self.len - pos);
            self.read(pos, &mut buffer[..count])?;
            writer
                .write_all(&buffer[..count])
                .map_err(LuaError::external)?;
            pos += count;
        }
        Ok(())
    }

    pub fn temporary(&self) -> mlua::Result<Source> {
        let mut file = tempfile::NamedTempFile::new().map_err(LuaError::external)?;
        self.copy_to(&mut file)?;
        Source::temporary(file)
    }

    fn find(&self, lua: &Lua, needle: Self, init: i64) -> mlua::Result<Option<(usize, usize)>> {
        let start = relative(init, self.len).max(1) as usize - 1;
        if start > self.len {
            return Ok(None);
        }
        if needle.len == 0 {
            return Ok(Some((start + 1, start)));
        }
        if needle.len > self.len - start {
            return Ok(None);
        }
        let pattern = needle.to_string(lua)?;
        let pattern = pattern.as_bytes();
        let _charge = Charge::new(
            lua,
            needle
                .len
                .checked_mul(std::mem::size_of::<usize>())
                .and_then(|n| n.checked_add(BLOCK))
                .ok_or_else(|| LuaError::runtime("search size overflow"))?,
        )?;
        let mut prefix = vec![0; pattern.len()];
        let mut matched = 0;
        for i in 1..pattern.len() {
            while matched > 0 && pattern[i] != pattern[matched] {
                matched = prefix[matched - 1];
            }
            if pattern[i] == pattern[matched] {
                matched += 1;
            }
            prefix[i] = matched;
        }
        let mut buffer = vec![0; BLOCK];
        let mut pos = start;
        matched = 0;
        while pos < self.len {
            let count = buffer.len().min(self.len - pos);
            self.read(pos, &mut buffer[..count])?;
            for (i, &byte) in buffer[..count].iter().enumerate() {
                while matched > 0 && byte != pattern[matched] {
                    matched = prefix[matched - 1];
                }
                if byte == pattern[matched] {
                    matched += 1;
                }
                if matched == pattern.len() {
                    return Ok(Some((pos + i + 2 - matched, pos + i + 1)));
                }
            }
            pos += count;
        }
        Ok(None)
    }

    // Delegate Lua's format/overflow rules. A growing bounded window also handles
    // z/s formats without materializing the whole file. Max Lua alignment is 16.
    fn unpack(&self, lua: &Lua, format: LuaString, pos: i64) -> mlua::Result<MultiValue> {
        let pos = relative(pos, self.len);
        if pos < 1 || pos as usize > self.len + 1 {
            return Err(LuaError::runtime("initial position out of string"));
        }
        let start = (pos as usize - 1) & !15;
        let local_pos = pos as usize - start;
        let available = self.len - start;
        let mut length = available.min(256);
        let unpack: mlua::Function = lua.named_registry_value("bytes.unpack")?;
        loop {
            let window = self.slice(lua, start, start + length)?.to_string(lua)?;
            match unpack.call::<MultiValue>((format.clone(), window, local_pos)) {
                Ok(mut values) => {
                    let next = values.pop_back().expect("unpack next position");
                    values.push_back((i64::from_lua(next, lua)? + start as i64).into_lua(lua)?);
                    return Ok(values);
                }
                Err(error) => {
                    let message = error.to_string();
                    if length == available
                        || !(message.contains("data string too short")
                            || message.contains("unfinished string for format 'z'"))
                    {
                        return Err(error);
                    }
                    length = length.saturating_mul(2).max(1).min(available);
                    if length > MEMORY_LIMIT {
                        return Err(LuaError::runtime(
                            "unpack result exceeds sandbox memory budget",
                        ));
                    }
                }
            }
        }
    }
}

fn relative(index: i64, len: usize) -> i64 {
    if index >= 0 {
        index
    } else {
        (len as i64).saturating_add(index).saturating_add(1)
    }
}
fn write_pos(pos: i64) -> mlua::Result<usize> {
    pos.checked_sub(1)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| LuaError::runtime("bytes write position must be positive"))
}

impl UserData for BytesView {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Len, |_, this, ()| Ok(this.len));
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("bytes({})", this.len))
        });
        methods.add_meta_method(MetaMethod::Index, |_, this, i: i64| {
            if i <= 0 || i as usize > this.len {
                return Ok(None);
            }
            let mut byte = [0];
            this.read(i as usize - 1, &mut byte)?;
            Ok(Some(byte[0]))
        });
        methods.add_meta_method_mut(MetaMethod::NewIndex, |lua, this, (i, byte): (i64, u8)| {
            this.edit(
                lua,
                write_pos(i)?,
                1,
                Self::string(lua, lua.create_string([byte])?)?,
            )
        });
        methods.add_method("to_string", |lua, this, ()| this.to_string(lua));
        methods.add_method("clone", |_, this, ()| Ok(this.clone()));
        methods.add_method("sub", |lua, this, (i, j): (i64, Option<i64>)| {
            let start = relative(i, this.len).max(1) as usize - 1;
            let end = relative(j.unwrap_or(-1), this.len).max(0) as usize;
            this.slice(
                lua,
                start.min(this.len),
                end.min(this.len).max(start.min(this.len)),
            )
        });
        methods.add_method("byte", |lua, this, (i, j): (Option<i64>, Option<i64>)| {
            let i = i.unwrap_or(1);
            let start = relative(i, this.len).max(1) as usize - 1;
            let end = (relative(j.unwrap_or(i), this.len).max(0) as usize).min(this.len);
            let count = end.saturating_sub(start);
            let _charge = Charge::new(
                lua,
                count
                    .checked_mul(std::mem::size_of::<Value>() + 1)
                    .ok_or_else(|| LuaError::runtime("too many byte results"))?,
            )?;
            let mut data = vec![0; count];
            this.read(start.min(this.len), &mut data)?;
            Ok(MultiValue::from_vec(
                data.into_iter().map(|b| Value::Integer(b as i64)).collect(),
            ))
        });
        methods.add_method("find", |lua, this, (needle, init): (Value, Option<i64>)| {
            Ok(
                match this.find(lua, Self::input(lua, needle)?, init.unwrap_or(1))? {
                    Some((start, end)) => MultiValue::from_vec(vec![
                        Value::Integer(start as i64),
                        Value::Integer(end as i64),
                    ]),
                    None => MultiValue::from_vec(vec![Value::Nil]),
                },
            )
        });
        methods.add_method(
            "unpack",
            |lua, this, (format, pos): (LuaString, Option<i64>)| {
                this.unpack(lua, format, pos.unwrap_or(1))
            },
        );
        methods.add_method_mut("set", |lua, this, (pos, byte): (i64, u8)| {
            this.edit(
                lua,
                write_pos(pos)?,
                1,
                Self::string(lua, lua.create_string([byte])?)?,
            )
        });
        methods.add_function(
            "write",
            |lua, (this, pos, data): (mlua::AnyUserData, i64, Value)| {
                let data = Self::input(lua, data)?;
                this.borrow_mut::<Self>()?
                    .edit(lua, write_pos(pos)?, data.len, data)
            },
        );
        methods.add_function(
            "splice",
            |lua, (this, pos, count, data): (mlua::AnyUserData, i64, usize, Value)| {
                let data = Self::input(lua, data)?;
                this.borrow_mut::<Self>()?
                    .edit(lua, write_pos(pos)?, count, data)
            },
        );
        methods.add_method_mut("pack", |lua, this, mut args: MultiValue| {
            let pos = i64::from_lua(args.pop_front().unwrap_or(Value::Nil), lua)?;
            let pack: mlua::Function = lua.named_registry_value("bytes.pack")?;
            let data = Self::string(lua, pack.call::<LuaString>(args)?)?;
            this.edit(lua, write_pos(pos)?, data.len, data)
        });
    }
}

pub(super) fn install(lua: &Lua) -> mlua::Result<()> {
    lua.set_app_data(NativeMemory::default());
    let string: mlua::Table = lua.globals().get("string")?;
    for method in ["pack", "unpack"] {
        lua.set_named_registry_value(
            &format!("bytes.{method}"),
            string.get::<mlua::Function>(method)?,
        )?;
    }
    let module = lua.create_table()?;
    module.set(
        "view",
        lua.create_function(|lua, text: LuaString| BytesView::string(lua, text))?,
    )?;
    module.set(
        "new",
        lua.create_function(|lua, (size, fill): (usize, Option<u8>)| {
            let _charge = Charge::new(lua, size)?;
            BytesView::string(lua, lua.create_string(vec![fill.unwrap_or(0); size])?)
        })?,
    )?;
    lua.globals().set(
        "bytes",
        super::codec::read_only_module(lua, module, "bytes")?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_slices_and_clones_are_independent_and_atomic() {
        let (lua, _) = super::super::safe_lua().unwrap();
        lua.load(
            r#"
            local b = bytes.view('abcdef')
            local sub, copy = b:sub(2, 4), b:clone()
            b:set(2, 255)
            sub[1] = 0
            assert(b:byte(2) == 255 and sub[1] == 0 and copy:to_string() == 'abcdef')
            b:splice(3, 2, 'XYZ')
            assert(b:to_string() == 'a' .. string.char(255) .. 'XYZef')
            b:write(3, bytes.view('12'))
            assert(b:sub(3, 5):to_string() == '12Z')
            local before = b:to_string()
            assert(not pcall(function() b:write(#b, 'too long') end))
            assert(b:to_string() == before)
            assert(bytes.new(3, 42):to_string() == '***')
            assert(tostring(b) == 'bytes(7)')
            assert(not pcall(function() b:set(1, 256) end))
            local self_copy = bytes.view('ab')
            self_copy:splice(3, 0, self_copy)
            assert(self_copy:to_string() == 'abab')
        "#,
        )
        .exec()
        .unwrap();
    }
    #[test]
    fn string_operations_match_lua() {
        let (lua, _) = super::super::safe_lua().unwrap();
        lua.load(r#"
            local text = 'abc\0defabc'
            local b = bytes.view(text)
            for i = -12, 12 do
                for j = -12, 12 do
                    assert(b:sub(i, j):to_string() == text:sub(i, j))
                    assert(table.concat({b:byte(i,j)}, ',') == table.concat({text:byte(i,j)}, ','))
                end
                for _, needle in ipairs({'', 'abc', '\0d', '.', 'missing'}) do
                    local s, e = b:find(needle, i)
                    local x, y = text:find(needle, i, true)
                    assert(s == x and e == y)
                end
            end
            local text = string.pack('!8 b x I4 z s2', 7, 1000, 'hello', 'world')
            local b = bytes.view(text)
            local a = {b:unpack('!8 b x I4 z s2')}
            local expected = {string.unpack('!8 b x I4 z s2', text)}
            assert(table.concat(a, ',') == table.concat(expected, ','))
            b:pack(1, '>I4', 12345)
            assert(b:unpack('>I4') == 12345)
            for pos = 1, 17 do
                local format = '!8 I4 z s2'
                local text = string.pack('c' .. pos .. format, string.rep('p', pos), 1000, string.rep('z', 1024), 'tail')
                local actual = {bytes.view(text):unpack(format, pos + 1)}
                local expected = {string.unpack(format, text, pos + 1)}
                assert(table.concat(actual, ',') == table.concat(expected, ','))
            end
        "#).exec().unwrap();
    }
    #[test]
    fn compressed_binary_source_decodes_to_an_immutable_file_without_the_text_limit() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let block = [255; BLOCK];
        for _ in 0..320 {
            encoder.write_all(&block).unwrap();
        }
        let source = Source::body(BodySource {
            data: BodySourceData::Bytes(encoder.finish().unwrap()),
            path: None,
            stored_size: 0,
            content_type: Some("application/octet-stream".into()),
            content_encodings: vec!["gzip".into()],
        })
        .unwrap();
        assert_eq!(source.len(), 20 * 1024 * 1024);
        let Source::File(file) = &source else {
            panic!("large decoded content must be file backed");
        };
        let path = file.path.clone();
        let (lua, _) = super::super::safe_lua().unwrap();
        lua.globals()
            .set("data", source.view(&lua).unwrap())
            .unwrap();
        drop(source);
        lua.load("assert(data[1] == 255 and data[#data] == 255); local part = data:sub(-4); part[1] = 0; assert(data[#data-3] == 255)").exec().unwrap();
        assert!(lua.used_memory() < 1024 * 1024);
        drop(lua);
        assert!(!path.exists());
    }

    #[test]
    fn allocations_obey_the_shared_budget_without_changing_failed_edits() {
        let (lua, _) = super::super::safe_lua().unwrap();
        lua.load(
            r#"
            local data = bytes.new(4 * 1024 * 1024, 42)
            local copies = {}
            for i = 1, 30 do copies[i] = data:clone() end
            assert(not pcall(function() return bytes.new(16 * 1024 * 1024) end))
            assert(data[1] == 42 and copies[30][1] == 42)
            data[1] = 7
            assert(copies[30][1] == 42)
        "#,
        )
        .exec()
        .unwrap();
        assert!(lua.used_memory() < 5 * 1024 * 1024);
    }

    #[test]
    fn large_file_uses_ranges_and_does_not_change_source() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.as_file().set_len(40 * 1024 * 1024).unwrap();
        file.seek(SeekFrom::Start((BLOCK - 2) as u64)).unwrap();
        file.write_all(b"needle").unwrap();
        let (lua, _) = super::super::safe_lua().unwrap();
        lua.globals()
            .set(
                "data",
                Source::file(file.path()).unwrap().view(&lua).unwrap(),
            )
            .unwrap();
        lua.load(
            r#"
            assert(#data == 40 * 1024 * 1024)
            assert(data:find('needle') == 65535)
            local b = data:clone()
            b:write(65535, 'edited')
            assert(data:sub(65535, 65540):to_string() == 'needle')
            assert(b:sub(65535, 65540):to_string() == 'edited')
            assert(b:unpack('>I4', #b - 3) == 0)
            assert(not pcall(function() return b:to_string() end))
        "#,
        )
        .exec()
        .unwrap();
        assert!(lua.used_memory() < 1024 * 1024);
        let mut original = [0; 6];
        file.seek(SeekFrom::Start((BLOCK - 2) as u64)).unwrap();
        file.read_exact(&mut original).unwrap();
        assert_eq!(&original, b"needle");
    }
}
