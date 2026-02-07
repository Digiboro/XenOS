//! mkhive - утилита для создания Windows Registry Hive файлов из INF
//!
//! Парсит INF файл и генерирует hive на основе директив AddReg.
//!
//! # Использование
//!
//! ```sh
//! mkhive --input system.inf --output SYSTEM [--hive-root SYSTEM]
//!        [--section DefaultInstall] [--addreg-section AddReg]
//! ```

use hive::{HiveBuilder, KeyHandle, KeyValueDataType};
use std::error::Error;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse(std::env::args_os().skip(1))?;

    let inf_text = fs::read_to_string(&args.input)?;
    let doc = inf::InfDoc::parse(&inf_text)?;
    let strings = doc.parse_strings();

    // Определяем, какие add-registry секции применять.
    let mut addreg_sections: Vec<String> = Vec::new();
    if !args.install_sections.is_empty() {
        for sec in &args.install_sections {
            addreg_sections.extend(doc.collect_addreg_sections(sec, &strings)?);
        }
    }
    if !args.addreg_sections.is_empty() {
        addreg_sections.extend(args.addreg_sections.clone());
    }
    if addreg_sections.is_empty() {
        addreg_sections.push("AddReg".to_string());
    }
    inf::dedup_in_place_case_insensitive(&mut addreg_sections);

    let ops = doc.parse_addreg_sections(&addreg_sections, &args.hive_root, &strings)?;

    let mut tree = model::KeyTree::new(args.hive_root.clone());
    for op in ops {
        tree.apply(op)?;
    }

    let bytes = build_hive(&tree)?;

    if let Some(parent) = args.output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(&args.output, bytes)?;

    Ok(())
}

/// Строит hive файл из дерева ключей используя библиотеку hive
fn build_hive(tree: &model::KeyTree) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut builder = HiveBuilder::new(&tree.hive_root)?;

    // Рекурсивно добавляем ключи и значения
    let root = builder.root_key();
    build_key_recursive(&mut builder, &root, &tree.root)?;

    Ok(builder.build()?)
}

/// Рекурсивно строит дерево ключей
fn build_key_recursive(
    builder: &mut HiveBuilder,
    parent: &KeyHandle,
    node: &model::KeyNode,
) -> Result<(), Box<dyn Error>> {
    // Добавляем значения
    for (name, value) in &node.values {
        add_value(builder, parent, name, value)?;
    }

    // Рекурсивно добавляем подключи
    for (name, child) in &node.subkeys {
        let child_handle = builder.create_subkey(parent, name)?;
        build_key_recursive(builder, &child_handle, child)?;
    }

    Ok(())
}

/// Добавляет значение в ключ
fn add_value(
    builder: &mut HiveBuilder,
    key: &KeyHandle,
    name: &str,
    value: &model::RegValue,
) -> Result<(), Box<dyn Error>> {
    // Типы реестра
    const REG_NONE: u32 = 0;
    const REG_SZ: u32 = 1;
    const REG_EXPAND_SZ: u32 = 2;
    const REG_BINARY: u32 = 3;
    const REG_DWORD: u32 = 4;
    const REG_MULTI_SZ: u32 = 7;
    const REG_QWORD: u32 = 11;

    match value.value_type {
        REG_DWORD => {
            if value.data.len() >= 4 {
                let dword = u32::from_le_bytes(value.data[..4].try_into()?);
                builder.set_dword(key, name, dword)?;
            }
        }
        REG_QWORD => {
            if value.data.len() >= 8 {
                let qword = u64::from_le_bytes(value.data[..8].try_into()?);
                builder.set_qword(key, name, qword)?;
            }
        }
        REG_SZ => {
            // Данные уже в UTF-16LE, передаем как есть
            builder.set_raw(key, name, KeyValueDataType::RegSZ, &value.data)?;
        }
        REG_EXPAND_SZ => {
            builder.set_raw(key, name, KeyValueDataType::RegExpandSZ, &value.data)?;
        }
        REG_MULTI_SZ => {
            builder.set_raw(key, name, KeyValueDataType::RegMultiSZ, &value.data)?;
        }
        REG_BINARY => {
            builder.set_binary(key, name, &value.data)?;
        }
        REG_NONE | _ => {
            // Для REG_NONE и неизвестных типов используем бинарные данные
            if !value.data.is_empty() {
                builder.set_binary(key, name, &value.data)?;
            }
        }
    }

    Ok(())
}

// =============================================================================
// Аргументы командной строки
// =============================================================================

struct Args {
    input: PathBuf,
    output: PathBuf,
    hive_root: String,
    /// Секции типа [DefaultInstall]/[DDInstall], которые содержат `AddReg=...`
    install_sections: Vec<String>,
    /// Явно заданные add-registry секции: тела [Foo.AddReg] (строки reg-root,subkey,...)
    addreg_sections: Vec<String>,
}

impl Args {
    fn parse(mut it: impl Iterator<Item = std::ffi::OsString>) -> Result<Self, Box<dyn Error>> {
        let mut input: Option<PathBuf> = None;
        let mut output: Option<PathBuf> = None;
        let mut hive_root: String = "SYSTEM".to_string();
        let mut install_sections: Vec<String> = Vec::new();
        let mut addreg_sections: Vec<String> = Vec::new();

        while let Some(arg) = it.next() {
            let s = arg.to_string_lossy();
            match s.as_ref() {
                "-i" | "--input" => {
                    input = Some(PathBuf::from(
                        it.next().ok_or("missing value for --input")?,
                    ))
                }
                "-o" | "--output" => {
                    output = Some(PathBuf::from(
                        it.next().ok_or("missing value for --output")?,
                    ))
                }
                "--hive-root" => {
                    hive_root = it
                        .next()
                        .ok_or("missing value for --hive-root")?
                        .to_string_lossy()
                        .into_owned()
                }
                "--section" => install_sections.push(
                    it.next()
                        .ok_or("missing value for --section")?
                        .to_string_lossy()
                        .into_owned(),
                ),
                "--addreg-section" => addreg_sections.push(
                    it.next()
                        .ok_or("missing value for --addreg-section")?
                        .to_string_lossy()
                        .into_owned(),
                ),
                // Обратная совместимость: `--addreg <name>` (или список через запятую)
                "--addreg" => {
                    let v = it
                        .next()
                        .ok_or("missing value for --addreg")?
                        .to_string_lossy()
                        .into_owned();
                    for part in v.split(',') {
                        let p = part.trim();
                        if !p.is_empty() {
                            addreg_sections.push(p.to_string());
                        }
                    }
                }
                "-h" | "--help" => {
                    eprintln!(
                        "mkhive (XenCore)\n\n\
USAGE:\n  \
  mkhive --input <file.inf> --output <SYSTEM> [--hive-root SYSTEM]\n         \
  [--section <InstallSection> ...] [--addreg-section <AddRegSection> ...]\n\n\
DEFAULT:\n  \
  - если не задано ни --section, ни --addreg-section, применяется секция [AddReg]\n\n\
NOTE:\n  \
  В XenCore обычно задан дефолтный target (no_std), поэтому для запуска host-тулзы\n  \
  может понадобиться указать target хоста, например:\n    \
  cargo run --target $(rustc -vV | sed -n 's/^host: //p') \\\n      \
  --manifest-path xenos/tools/mkhive/Cargo.toml -- --input ... --output ...\n"
                    );
                    std::process::exit(0);
                }
                _ => return Err(format!("unknown arg: {s}").into()),
            }
        }

        Ok(Self {
            input: input.ok_or("--input is required")?,
            output: output.ok_or("--output is required")?,
            hive_root,
            install_sections,
            addreg_sections,
        })
    }
}

// =============================================================================
// Модуль парсинга INF файлов
// =============================================================================

mod inf {
    use super::model::AddRegOp;
    use std::collections::{BTreeMap, HashMap};
    use std::error::Error;

    #[derive(Clone, Debug)]
    struct InfLine {
        lineno: usize,
        text: String,
    }

    #[derive(Debug)]
    pub struct InfDoc {
        sections: BTreeMap<String, Vec<InfLine>>,
    }

    impl InfDoc {
        pub fn parse(text: &str) -> Result<Self, Box<dyn Error>> {
            let mut sections: BTreeMap<String, Vec<InfLine>> = BTreeMap::new();
            let mut cur: Option<String> = None;
            let mut pending: Option<(usize, String)> = None;

            for (idx, raw) in text.lines().enumerate() {
                let lineno = idx + 1;
                let mut line = raw.to_string();

                let trimmed = line.trim();
                if pending.is_none() && trimmed.starts_with('[') && trimmed.ends_with(']') {
                    let name = trimmed[1..trimmed.len() - 1].trim().to_ascii_lowercase();
                    cur = Some(name);
                    continue;
                }

                line = strip_comment(&line);

                if let Some((start_line, mut acc)) = pending.take() {
                    acc.push_str(line.trim_start());
                    if line_ends_with_continuation(&acc) {
                        let acc2 = acc.trim_end();
                        let acc3 = acc2[..acc2.len() - 1].to_string();
                        pending = Some((start_line, acc3));
                        continue;
                    }
                    if let Some(sec) = cur.as_ref() {
                        let t = acc.trim().to_string();
                        if !t.is_empty() {
                            sections.entry(sec.clone()).or_default().push(InfLine {
                                lineno: start_line,
                                text: t,
                            });
                        }
                    }
                    continue;
                }

                if line_ends_with_continuation(&line) {
                    let mut acc = line.trim_end().to_string();
                    acc.pop();
                    pending = Some((lineno, acc));
                    continue;
                }

                let t = line.trim();
                if t.is_empty() {
                    continue;
                }
                if let Some(sec) = cur.as_ref() {
                    sections.entry(sec.clone()).or_default().push(InfLine {
                        lineno,
                        text: t.to_string(),
                    });
                }
            }

            if let Some((start_line, acc)) = pending.take() {
                if let Some(sec) = cur.as_ref() {
                    let t = acc.trim().to_string();
                    if !t.is_empty() {
                        sections.entry(sec.clone()).or_default().push(InfLine {
                            lineno: start_line,
                            text: t,
                        });
                    }
                }
            }

            Ok(Self { sections })
        }

        pub fn parse_strings(&self) -> Strings {
            let mut map: HashMap<String, String> = HashMap::new();
            let Some(lines) = self.sections.get("strings") else {
                return Strings { map };
            };

            for l in lines {
                if let Some((k, v)) = parse_assignment(&l.text) {
                    let key = k.trim().to_ascii_lowercase();
                    let val = unquote(v.trim());
                    map.insert(key, val);
                }
            }

            let s = Strings { map };
            let mut resolved = HashMap::new();
            for (k, v) in &s.map {
                resolved.insert(k.clone(), s.expand(v));
            }
            Strings { map: resolved }
        }

        pub fn collect_addreg_sections(
            &self,
            install_section: &str,
            strings: &Strings,
        ) -> Result<Vec<String>, Box<dyn Error>> {
            let sec_key = install_section.trim().to_ascii_lowercase();
            let Some(lines) = self.sections.get(&sec_key) else {
                return Err(format!("INF: section [{install_section}] not found").into());
            };

            let mut out = Vec::new();
            for l in lines {
                if let Some((k, v)) = parse_assignment(&l.text) {
                    if k.trim().eq_ignore_ascii_case("AddReg") {
                        for part in v.split(',') {
                            let name = strings.expand(&unquote(part.trim()));
                            let name = name.trim();
                            if !name.is_empty() {
                                out.push(name.to_string());
                            }
                        }
                    }
                }
            }
            Ok(out)
        }

        pub fn parse_addreg_sections(
            &self,
            addreg_sections: &[String],
            _hive_root: &str,
            strings: &Strings,
        ) -> Result<Vec<AddRegOp>, Box<dyn Error>> {
            let mut ops = Vec::new();
            for sec in addreg_sections {
                let sec_key = sec.trim().to_ascii_lowercase();
                let Some(lines) = self.sections.get(&sec_key) else {
                    return Err(format!("INF: section [{sec}] not found").into());
                };
                for l in lines {
                    let raw = l.text.trim();
                    if raw.is_empty() {
                        continue;
                    }
                    let fields =
                        split_csv_like(raw).map_err(|e| format!("INF:{}: {e}", l.lineno))?;
                    if fields.len() < 2 {
                        return Err(format!(
                            "INF:{}: AddReg entry has too few fields: {raw}",
                            l.lineno
                        )
                        .into());
                    }
                    let root = strings.expand(&unquote(fields[0].trim()));
                    if root.eq_ignore_ascii_case("HKR") {
                        return Err(
                            format!("INF:{}: HKR is not supported in mkhive", l.lineno).into()
                        );
                    }
                    if !matches_root(&root) {
                        return Err(
                            format!("INF:{}: unsupported reg-root: {root}", l.lineno).into()
                        );
                    }

                    let key_path = strings.expand(&unquote(fields[1].trim()));
                    let value_name = if fields.len() >= 3 {
                        strings.expand(&unquote(fields[2].trim()))
                    } else {
                        String::new()
                    };

                    let (flags, data_start) = if fields.len() >= 4 {
                        let f3 = fields[3].trim();
                        if f3.is_empty() {
                            (0u32, 4)
                        } else {
                            match parse_u32_auto(f3) {
                                Ok(v) => (v, 4),
                                Err(_) => (0u32, 3),
                            }
                        }
                    } else {
                        (0u32, 4)
                    };

                    let value_type = decode_value_type(flags);
                    let data_fields = if data_start <= fields.len() {
                        &fields[data_start..]
                    } else {
                        &[]
                    };
                    let data = encode_data(value_type, flags, data_fields, strings)
                        .map_err(|e| format!("INF:{}: {e}", l.lineno))?;

                    ops.push(AddRegOp {
                        key_path,
                        value_name,
                        flags,
                        value_type,
                        data,
                    });
                }
            }
            Ok(ops)
        }
    }

    #[derive(Clone, Debug)]
    pub struct Strings {
        map: HashMap<String, String>,
    }

    impl Strings {
        pub fn expand(&self, s: &str) -> String {
            let mut cur = s.to_string();
            for _ in 0..16 {
                let next = expand_once(&cur, &self.map);
                if next == cur {
                    return next;
                }
                cur = next;
            }
            cur
        }
    }

    pub fn dedup_in_place_case_insensitive(v: &mut Vec<String>) {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        v.retain(|s| {
            let k = s.trim().to_ascii_lowercase();
            if k.is_empty() {
                return false;
            }
            if seen.contains(&k) {
                false
            } else {
                seen.insert(k);
                true
            }
        });
    }

    fn matches_root(root: &str) -> bool {
        root.eq_ignore_ascii_case("HKLM")
            || root.eq_ignore_ascii_case("HKU")
            || root.eq_ignore_ascii_case("HKCU")
            || root.eq_ignore_ascii_case("HKCR")
    }

    fn parse_assignment(line: &str) -> Option<(&str, &str)> {
        let mut in_quotes = false;
        for (i, ch) in line.char_indices() {
            match ch {
                '"' => in_quotes = !in_quotes,
                '=' if !in_quotes => {
                    let (k, v) = line.split_at(i);
                    return Some((k, &v[1..]));
                }
                _ => {}
            }
        }
        None
    }

    fn strip_comment(line: &str) -> String {
        let mut out = String::new();
        let mut in_quotes = false;
        for ch in line.chars() {
            match ch {
                '"' => {
                    in_quotes = !in_quotes;
                    out.push(ch);
                }
                ';' if !in_quotes => break,
                _ => out.push(ch),
            }
        }
        out
    }

    fn line_ends_with_continuation(line: &str) -> bool {
        let t = line.trim_end();
        if !t.ends_with('\\') {
            return false;
        }
        let mut in_quotes = false;
        for ch in t.chars() {
            if ch == '"' {
                in_quotes = !in_quotes;
            }
        }
        !in_quotes
    }

    fn split_csv_like(line: &str) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut in_quotes = false;
        for ch in line.chars() {
            match ch {
                '"' => {
                    in_quotes = !in_quotes;
                    cur.push(ch);
                }
                ',' if !in_quotes => {
                    out.push(cur.trim().to_string());
                    cur.clear();
                }
                _ => cur.push(ch),
            }
        }
        if in_quotes {
            return Err(format!("unterminated quote in line: {line}"));
        }
        out.push(cur.trim().to_string());
        Ok(out)
    }

    fn unquote(s: &str) -> String {
        let s = s.trim();
        if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
            let inner = &s[1..s.len() - 1];
            inner.replace("\\\"", "\"").replace("\\\\", "\\")
        } else {
            s.to_string()
        }
    }

    fn expand_once(s: &str, dict: &HashMap<String, String>) -> String {
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '%' {
                out.push(ch);
                continue;
            }
            match chars.peek().copied() {
                Some('%') => {
                    chars.next();
                    out.push('%');
                }
                Some(_) => {
                    let mut name = String::new();
                    while let Some(c) = chars.next() {
                        if c == '%' {
                            break;
                        }
                        name.push(c);
                    }
                    let key = name.to_ascii_lowercase();
                    if let Some(val) = dict.get(&key) {
                        out.push_str(val);
                    } else {
                        out.push('%');
                        out.push_str(&name);
                        out.push('%');
                    }
                }
                None => out.push('%'),
            }
        }
        out
    }

    fn parse_u32_auto(s: &str) -> Result<u32, Box<dyn Error>> {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix("0x") {
            Ok(u32::from_str_radix(hex, 16)?)
        } else {
            Ok(s.parse::<u32>()?)
        }
    }

    fn decode_value_type(flags: u32) -> u32 {
        const REG_NONE: u32 = 0;
        const REG_SZ: u32 = 1;
        const REG_EXPAND_SZ: u32 = 2;
        const REG_BINARY: u32 = 3;
        const REG_DWORD: u32 = 4;
        const REG_MULTI_SZ: u32 = 7;
        const REG_QWORD: u32 = 11;

        let f = flags & !(0x0000_1000 | 0x0000_4000);

        match f {
            0x0000_0000 => REG_SZ,
            0x0000_0001 => REG_BINARY,
            0x0001_0000 => REG_MULTI_SZ,
            0x0002_0000 => REG_EXPAND_SZ,
            0x0001_0001 => REG_DWORD,
            0x0002_0001 => REG_NONE,
            0x000B_0001 => REG_QWORD,
            other if (other & 0xFFFF_0000) != 0 && (other & 0x0000_0001) != 0 => {
                (other >> 16) & 0xFFFF
            }
            other if (other & 0xFFFF_0000) != 0 && (other & 0x0000_0001) == 0 => REG_BINARY,
            _ => REG_SZ,
        }
    }

    fn encode_data(
        value_type: u32,
        flags: u32,
        fields: &[String],
        strings: &Strings,
    ) -> Result<Vec<u8>, Box<dyn Error>> {
        const REG_NONE: u32 = 0;
        const REG_SZ: u32 = 1;
        const REG_EXPAND_SZ: u32 = 2;
        const REG_BINARY: u32 = 3;
        const REG_DWORD: u32 = 4;
        const REG_MULTI_SZ: u32 = 7;
        const REG_QWORD: u32 = 11;

        let op_flags = flags & !(0x0000_1000 | 0x0000_4000);
        if (op_flags & 0x0000_0010) != 0 || (op_flags & 0x0000_0004) != 0 {
            return Ok(Vec::new());
        }

        match value_type {
            REG_SZ | REG_EXPAND_SZ => {
                let s = fields
                    .first()
                    .map(|s| strings.expand(&unquote(s.trim())))
                    .unwrap_or_default();
                Ok(utf16z(&s))
            }
            REG_MULTI_SZ => {
                let mut list = Vec::new();
                for f in fields {
                    let t = f.trim();
                    if t.is_empty() {
                        continue;
                    }
                    list.push(strings.expand(&unquote(t)));
                }
                Ok(utf16_multi_sz(&list))
            }
            REG_DWORD => {
                let s = fields.first().ok_or("missing DWORD value")?;
                let v = parse_u32_auto(&strings.expand(&unquote(s.trim())))?;
                Ok(v.to_le_bytes().to_vec())
            }
            REG_QWORD => {
                let s = fields.first().ok_or("missing QWORD value")?;
                let txt = strings.expand(&unquote(s.trim()));
                let v = parse_u64_auto(&txt)?;
                Ok(v.to_le_bytes().to_vec())
            }
            REG_NONE => {
                if fields.is_empty() {
                    Ok(Vec::new())
                } else {
                    Ok(parse_byte_list(fields, strings)?)
                }
            }
            REG_BINARY => {
                if fields.is_empty() {
                    Ok(Vec::new())
                } else {
                    let first = fields[0].trim();
                    if first.starts_with('"') {
                        Ok(strings.expand(&unquote(first)).into_bytes())
                    } else {
                        Ok(parse_byte_list(fields, strings)?)
                    }
                }
            }
            _custom => Ok(parse_byte_list(fields, strings)?),
        }
    }

    fn parse_u64_auto(s: &str) -> Result<u64, Box<dyn Error>> {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix("0x") {
            Ok(u64::from_str_radix(hex, 16)?)
        } else {
            Ok(s.parse::<u64>()?)
        }
    }

    fn parse_byte_list(fields: &[String], strings: &Strings) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut out = Vec::new();
        for f in fields {
            let t = strings.expand(&unquote(f.trim()));
            let t = t.trim();
            if t.is_empty() {
                continue;
            }
            let b = parse_u8_auto(t)?;
            out.push(b);
        }
        Ok(out)
    }

    fn parse_u8_auto(s: &str) -> Result<u8, Box<dyn Error>> {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix("0x") {
            return Ok(u8::from_str_radix(hex, 16)?);
        }
        let is_hexish = s.chars().all(|c| c.is_ascii_hexdigit())
            && s.chars().any(|c| c.is_ascii_alphabetic())
            && s.len() <= 2;
        if is_hexish {
            return Ok(u8::from_str_radix(s, 16)?);
        }
        Ok(s.parse::<u8>()?)
    }

    fn utf16z(s: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for u in s.encode_utf16() {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out.extend_from_slice(&[0u8, 0u8]);
        out
    }

    fn utf16_multi_sz(list: &[String]) -> Vec<u8> {
        let mut out = Vec::new();
        for s in list {
            out.extend_from_slice(&utf16z(s));
        }
        out.extend_from_slice(&[0u8, 0u8]);
        out
    }
}

// =============================================================================
// Модель дерева ключей
// =============================================================================

mod model {
    use std::collections::BTreeMap;
    use std::error::Error;

    #[derive(Clone, Debug)]
    pub struct AddRegOp {
        pub key_path: String,
        pub value_name: String,
        pub flags: u32,
        pub value_type: u32,
        pub data: Vec<u8>,
    }

    #[derive(Default, Debug)]
    pub struct KeyNode {
        pub subkeys: BTreeMap<String, KeyNode>,
        pub values: BTreeMap<String, RegValue>,
    }

    #[derive(Clone, Debug)]
    pub struct RegValue {
        pub value_type: u32,
        pub data: Vec<u8>,
    }

    #[derive(Debug)]
    pub struct KeyTree {
        pub hive_root: String,
        pub root: KeyNode,
    }

    impl KeyTree {
        pub fn new(hive_root: String) -> Self {
            Self {
                hive_root,
                root: KeyNode::default(),
            }
        }

        pub fn apply(&mut self, op: AddRegOp) -> Result<(), Box<dyn Error>> {
            let flags = op.flags & !(0x0000_1000 | 0x0000_4000);

            let mut parts = op
                .key_path
                .split('\\')
                .filter(|p| !p.is_empty())
                .map(|s| s.to_string())
                .collect::<Vec<_>>();

            if parts
                .first()
                .is_some_and(|s| s.eq_ignore_ascii_case(&self.hive_root))
            {
                parts.remove(0);
            }

            const FLG_ADDREG_NOCLOBBER: u32 = 0x0000_0002;
            const FLG_ADDREG_DELVAL: u32 = 0x0000_0004;
            const FLG_ADDREG_APPEND: u32 = 0x0000_0008;
            const FLG_ADDREG_KEYONLY: u32 = 0x0000_0010;
            const FLG_ADDREG_OVERWRITEONLY: u32 = 0x0000_0020;

            let is_del = (flags & FLG_ADDREG_DELVAL) != 0;
            let is_keyonly = (flags & FLG_ADDREG_KEYONLY) != 0;
            let is_append = (flags & FLG_ADDREG_APPEND) != 0;
            let is_noclobber = (flags & FLG_ADDREG_NOCLOBBER) != 0;
            let is_overwriteonly = (flags & FLG_ADDREG_OVERWRITEONLY) != 0;

            if is_del {
                if op.value_name.is_empty() {
                    self.delete_key(&parts);
                } else if let Some(key) = self.get_key_mut(&parts) {
                    key.values.remove(&op.value_name);
                }
                return Ok(());
            }

            if is_keyonly {
                let _ = self.get_or_create_key_mut(&parts);
                return Ok(());
            }

            if is_overwriteonly {
                let Some(key) = self.get_key_mut(&parts) else {
                    return Ok(());
                };
                if !key.values.contains_key(&op.value_name) {
                    return Ok(());
                }
            }

            let key = self.get_or_create_key_mut(&parts);

            if is_noclobber && key.values.contains_key(&op.value_name) {
                return Ok(());
            }

            if is_append {
                const REG_MULTI_SZ: u32 = 7;
                if op.value_type != REG_MULTI_SZ {
                    return Err("APPEND is only supported for REG_MULTI_SZ".into());
                }

                let mut cur_list = match key.values.get(&op.value_name) {
                    Some(v) if v.value_type == REG_MULTI_SZ => parse_multi_sz(&v.data),
                    Some(_) => return Err("APPEND target exists but is not REG_MULTI_SZ".into()),
                    None => Vec::new(),
                };
                let add_list = parse_multi_sz(&op.data);
                for s in add_list {
                    if !cur_list.iter().any(|x| x == &s) {
                        cur_list.push(s);
                    }
                }
                let merged = encode_multi_sz(&cur_list);
                key.values.insert(
                    op.value_name,
                    RegValue {
                        value_type: REG_MULTI_SZ,
                        data: merged,
                    },
                );
                return Ok(());
            }

            key.values.insert(
                op.value_name,
                RegValue {
                    value_type: op.value_type,
                    data: op.data,
                },
            );
            Ok(())
        }

        fn get_key_mut(&mut self, parts: &[String]) -> Option<&mut KeyNode> {
            let mut cur = &mut self.root;
            for p in parts {
                let next = cur.subkeys.get_mut(p)?;
                cur = next;
            }
            Some(cur)
        }

        fn get_or_create_key_mut(&mut self, parts: &[String]) -> &mut KeyNode {
            let mut cur = &mut self.root;
            for p in parts {
                cur = cur.subkeys.entry(p.clone()).or_default();
            }
            cur
        }

        fn delete_key(&mut self, parts: &[String]) {
            if parts.is_empty() {
                return;
            }
            let (parent, last) = parts.split_at(parts.len() - 1);
            if let Some(p) = self.get_key_mut(parent) {
                p.subkeys.remove(&last[0]);
            }
        }
    }

    fn parse_multi_sz(data: &[u8]) -> Vec<String> {
        if data.len() % 2 != 0 {
            return Vec::new();
        }
        let mut u16s = Vec::with_capacity(data.len() / 2);
        for chunk in data.chunks_exact(2) {
            u16s.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        let mut out = Vec::new();
        let mut cur = Vec::new();
        let mut i = 0;
        while i < u16s.len() {
            let w = u16s[i];
            if w == 0 {
                if cur.is_empty() {
                    break;
                }
                out.push(String::from_utf16_lossy(&cur));
                cur.clear();
            } else {
                cur.push(w);
            }
            i += 1;
        }
        out
    }

    fn encode_multi_sz(list: &[String]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        for s in list {
            for w in s.encode_utf16() {
                out.extend_from_slice(&w.to_le_bytes());
            }
            out.extend_from_slice(&[0u8, 0u8]);
        }
        out.extend_from_slice(&[0u8, 0u8]);
        out
    }
}
