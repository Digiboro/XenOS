use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Размер таблиц BMP: U+0000..U+FFFF.
const BMP_LEN: usize = 0x1_0000;

/// Флаги чанка PRP1.
const PRP_ALPHABETIC: u16 = 0x0001;
const PRP_UPPERCASE: u16 = 0x0002;
const PRP_LOWERCASE: u16 = 0x0004;
const PRP_DECIMAL_DIGIT: u16 = 0x0008;
const PRP_WHITE_SPACE: u16 = 0x0010;

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse(std::env::args_os().skip(1))?;

    let ucd_dir = args.ucd_dir.clone();

    // Таблицы v1: simple upper/lower + свойства для BMP.
    let mut upper: Vec<u16> = (0..BMP_LEN as u32).map(|v| v as u16).collect();
    let mut lower: Vec<u16> = (0..BMP_LEN as u32).map(|v| v as u16).collect();
    let mut props: Vec<u16> = vec![0u16; BMP_LEN];

    // 1) UnicodeData: simple mappings + Nd.
    let unicode_data = read_text(ucd_dir.join("UnicodeData.txt"))?;
    apply_unicode_data(&unicode_data, &mut upper, &mut lower, &mut props)?;

    // 2) SpecialCasing: override only unconditional + one-to-one.
    let special_casing = read_text(ucd_dir.join("SpecialCasing.txt"))?;
    apply_special_casing(&special_casing, &mut upper, &mut lower)?;

    // 3) DerivedCoreProperties: Alphabetic/Uppercase/Lowercase.
    let derived_core = read_text(ucd_dir.join("DerivedCoreProperties.txt"))?;
    apply_derived_core_properties(&derived_core, &mut props)?;

    // 4) PropList: White_Space.
    let prop_list = read_text(ucd_dir.join("PropList.txt"))?;
    apply_prop_list(&prop_list, &mut props)?;

    // Генерируем META.
    let generated_at_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let source_dir = args.ucd_dir.to_string_lossy();
    let meta = format!(
        "generator=nslgen\nsource_dir={source_dir}\ngenerated_at_unix={generated_at_unix}\n"
    );

    let out_dir = args.output_dir.unwrap_or_else(|| PathBuf::from("sysroot/XenOS/system32"));
    fs::create_dir_all(&out_dir)?;
    let out_file = out_dir.join("unicode.nls");

    let bytes = build_xnls_v1(&meta, &upper, &lower, &props, args.ucd_tag, args.ucd_revision)?;
    let mut f = fs::File::create(&out_file)?;
    f.write_all(&bytes)?;
    f.flush()?;

    Ok(())
}

struct Args {
    output_dir: Option<PathBuf>,
    ucd_dir: PathBuf,
    ucd_tag: [u8; 4],
    ucd_revision: u32,
}

impl Args {
    fn parse(mut it: impl Iterator<Item = std::ffi::OsString>) -> Result<Self, Box<dyn Error>> {
        let mut output_dir: Option<PathBuf> = None;
        let mut ucd_dir: PathBuf = PathBuf::from("build/ucd/latest/ucd");
        let mut ucd_tag: [u8; 4] = *b"UCD ";
        let mut ucd_revision: u32 = 0;

        while let Some(arg) = it.next() {
            let s = arg.to_string_lossy();
            match s.as_ref() {
                "-o" | "--output-dir" => output_dir = Some(PathBuf::from(it.next().ok_or("missing value for --output-dir")?)),
                "--ucd-dir" => ucd_dir = PathBuf::from(it.next().ok_or("missing value for --ucd-dir")?),
                "--ucd-tag" => {
                    let v = it.next().ok_or("missing value for --ucd-tag")?.to_string_lossy().into_owned();
                    ucd_tag = parse_tag4(&v)?;
                }
                "--ucd-rev" => {
                    let v = it.next().ok_or("missing value for --ucd-rev")?.to_string_lossy().into_owned();
                    ucd_revision = v.parse::<u32>()?;
                }
                "-h" | "--help" => {
                    eprintln!(
                        "nslgen (XenCore)\n\nUSAGE:\n  nslgen [--output-dir <dir>] [--ucd-dir <dir>]\n         [--ucd-tag <4cc>] [--ucd-rev <u32>]\n\nПО УМОЛЧАНИЮ:\n  --output-dir sysroot/XenOS/system32\n  --ucd-dir    build/ucd/latest/ucd\n  --ucd-tag    \"UCD \"\n  --ucd-rev    0\n\nОЖИДАЕМЫЕ ФАЙЛЫ В --ucd-dir:\n  - UnicodeData.txt\n  - SpecialCasing.txt\n  - DerivedCoreProperties.txt\n  - PropList.txt\n\nРЕЗУЛЬТАТ:\n  <output-dir>/unicode.nls (формат XNLS v1)\n\nПРИМЕЧАНИЕ:\n  Скачивание UCD выполняется makefile'ом; nslgen читает только локальные файлы.\n"
                    );
                    std::process::exit(0);
                }
                _ => return Err(format!("unknown arg: {s}").into()),
            }
        }

        Ok(Self {
            output_dir,
            ucd_dir,
            ucd_tag,
            ucd_revision,
        })
    }
}

fn parse_tag4(s: &str) -> Result<[u8; 4], Box<dyn Error>> {
    let b = s.as_bytes();
    if b.len() != 4 {
        return Err("--ucd-tag must be exactly 4 ASCII bytes".into());
    }
    if !b.iter().all(|c| c.is_ascii()) {
        return Err("--ucd-tag must be ASCII".into());
    }
    Ok([b[0], b[1], b[2], b[3]])
}

fn read_text(path: PathBuf) -> Result<String, Box<dyn Error>> {
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    Ok(text)
}

fn apply_unicode_data(text: &str, upper: &mut [u16], lower: &mut [u16], props: &mut [u16]) -> Result<(), Box<dyn Error>> {
    // UnicodeData.txt:
    // code;name;gc;ccc;bidi;decomp;decimal;digit;numeric;mirrored;oldname;comment;upper;lower;title
    let mut pending_range: Option<(u32, String, String)> = None; // (start_cp, gc, name_prefix)

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(';').collect();
        if fields.len() < 15 {
            return Err(format!("UnicodeData.txt:{}: too few fields", lineno + 1).into());
        }

        let cp = parse_hex_u32(fields[0]).map_err(|e| format!("UnicodeData.txt:{}: {e}", lineno + 1))?;
        let name = fields[1].trim();
        let gc = fields[2].trim().to_string();

        // Обработка First/Last диапазонов.
        if name.ends_with(", First>") {
            let prefix = name.to_string();
            pending_range = Some((cp, gc.clone(), prefix));
            continue;
        }
        if name.ends_with(", Last>") {
            if let Some((start, gc0, _prefix)) = pending_range.take() {
                if start > cp {
                    return Err(format!("UnicodeData.txt:{}: invalid range", lineno + 1).into());
                }
                for v in start..=cp {
                    if v <= 0xFFFF {
                        apply_gc_props(v as u16, &gc0, props);
                    }
                }
            }
            continue;
        }

        if cp <= 0xFFFF {
            let u = cp as u16;
            apply_gc_props(u, &gc, props);
            // Simple mappings.
            if let Some(m) = parse_optional_hex_u16(fields[12]) {
                upper[u as usize] = m;
            }
            if let Some(m) = parse_optional_hex_u16(fields[13]) {
                lower[u as usize] = m;
            }
        }
    }

    Ok(())
}

fn apply_gc_props(ch: u16, gc: &str, props: &mut [u16]) {
    // В v1 сохраняем только DecimalDigit (Nd) из UnicodeData.
    if gc == "Nd" {
        props[ch as usize] |= PRP_DECIMAL_DIGIT;
    }
}

fn apply_special_casing(text: &str, upper: &mut [u16], lower: &mut [u16]) -> Result<(), Box<dyn Error>> {
    // SpecialCasing.txt:
    // code; lower; title; upper; condition; # comment
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(';').map(|s| s.trim()).collect();
        if fields.len() < 5 {
            return Err(format!("SpecialCasing.txt:{}: too few fields", lineno + 1).into());
        }
        let cp = parse_hex_u32(fields[0]).map_err(|e| format!("SpecialCasing.txt:{}: {e}", lineno + 1))?;
        if cp > 0xFFFF {
            continue;
        }
        let cond = fields[4];
        if !cond.is_empty() {
            // Условные правила в v1 игнорируем (они требуют контекста).
            continue;
        }

        let lower_map = parse_codepoint_list_one(fields[1]);
        let upper_map = parse_codepoint_list_one(fields[3]);

        let idx = cp as usize;
        if let Some(v) = lower_map {
            if v <= 0xFFFF {
                lower[idx] = v as u16;
            }
        }
        if let Some(v) = upper_map {
            if v <= 0xFFFF {
                upper[idx] = v as u16;
            }
        }
    }
    Ok(())
}

fn parse_codepoint_list_one(s: &str) -> Option<u32> {
    // Поле — список hex кодовых точек через пробел. Берём только one-to-one.
    let mut it = s.split_whitespace();
    let first = it.next()?;
    if it.next().is_some() {
        return None;
    }
    parse_hex_u32(first).ok()
}

fn apply_derived_core_properties(text: &str, props: &mut [u16]) -> Result<(), Box<dyn Error>> {
    // DerivedCoreProperties.txt: "XXXX..YYYY ; Property # ..."
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (range, prop) = split_prop_line(line).ok_or_else(|| format!("DerivedCoreProperties.txt:{}: bad line", lineno + 1))?;
        let (start, end) = parse_range(range).map_err(|e| format!("DerivedCoreProperties.txt:{}: {e}", lineno + 1))?;

        let bit = match prop {
            "Alphabetic" => PRP_ALPHABETIC,
            "Uppercase" => PRP_UPPERCASE,
            "Lowercase" => PRP_LOWERCASE,
            _ => continue,
        };
        let s = start.min(0xFFFF);
        let e = end.min(0xFFFF);
        for cp in s..=e {
            props[cp as usize] |= bit;
        }
    }
    Ok(())
}

fn apply_prop_list(text: &str, props: &mut [u16]) -> Result<(), Box<dyn Error>> {
    // PropList.txt: "XXXX..YYYY ; White_Space # ..."
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (range, prop) = split_prop_line(line).ok_or_else(|| format!("PropList.txt:{}: bad line", lineno + 1))?;
        if prop != "White_Space" {
            continue;
        }
        let (start, end) = parse_range(range).map_err(|e| format!("PropList.txt:{}: {e}", lineno + 1))?;
        let s = start.min(0xFFFF);
        let e = end.min(0xFFFF);
        for cp in s..=e {
            props[cp as usize] |= PRP_WHITE_SPACE;
        }
    }
    Ok(())
}

fn split_prop_line(line: &str) -> Option<(&str, &str)> {
    // Ищем ';' как разделитель.
    let mut it = line.split(';');
    let left = it.next()?.trim();
    let right = it.next()?.trim();
    Some((left, right))
}

fn parse_range(s: &str) -> Result<(u32, u32), Box<dyn Error>> {
    let t = s.trim();
    if let Some((a, b)) = t.split_once("..") {
        let start = parse_hex_u32(a.trim())?;
        let end = parse_hex_u32(b.trim())?;
        Ok((start, end))
    } else {
        let v = parse_hex_u32(t)?;
        Ok((v, v))
    }
}

fn parse_hex_u32(s: &str) -> Result<u32, Box<dyn Error>> {
    Ok(u32::from_str_radix(s.trim(), 16)?)
}

fn parse_optional_hex_u16(s: &str) -> Option<u16> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    u16::from_str_radix(t, 16).ok()
}

fn build_xnls_v1(
    meta: &str,
    upper: &[u16],
    lower: &[u16],
    props: &[u16],
    ucd_tag: [u8; 4],
    ucd_revision: u32,
) -> Result<Vec<u8>, Box<dyn Error>> {
    if upper.len() != BMP_LEN || lower.len() != BMP_LEN || props.len() != BMP_LEN {
        return Err("invalid table sizes".into());
    }

    let mut chunks: Vec<Chunk> = Vec::new();
    chunks.push(Chunk::new("META", meta.as_bytes().to_vec(), 0, 0, 0));
    chunks.push(Chunk::new("UUP1", u16_table_bytes(upper), 0, 2, BMP_LEN as u32));
    chunks.push(Chunk::new("ULO1", u16_table_bytes(lower), 0, 2, BMP_LEN as u32));
    chunks.push(Chunk::new("PRP1", u16_table_bytes(props), 0, 2, BMP_LEN as u32));

    // Layout: header(48) + toc + chunk-data.
    let header_size = 48u64;
    let toc_entry_size = 32u64;
    let toc_offset = header_size;
    let toc_len = (chunks.len() as u64) * toc_entry_size;
    let mut cur = align8(toc_offset + toc_len);

    // Проставляем offsets и собираем TOC.
    for ch in &mut chunks {
        cur = align8(cur);
        ch.offset = cur;
        ch.length = ch.data.len() as u64;
        cur = cur.checked_add(align8(ch.length)).ok_or("file too large")?;
    }

    let file_size = cur;

    let file_size_usize: usize = file_size.try_into().map_err(|_| "file too large for this host")?;
    let header_size_usize: usize = header_size.try_into().unwrap();
    let mut out: Vec<u8> = Vec::with_capacity(file_size_usize);
    out.resize(header_size_usize, 0u8);

    // TOC.
    let toc_len_usize: usize = toc_len.try_into().map_err(|_| "toc too large for this host")?;
    let mut toc: Vec<u8> = Vec::with_capacity(toc_len_usize);
    for ch in &chunks {
        toc.extend_from_slice(&ch.id);
        toc.extend_from_slice(&ch.offset.to_le_bytes()); // u64
        toc.extend_from_slice(&ch.length.to_le_bytes()); // u64
        toc.extend_from_slice(&ch.flags.to_le_bytes());
        toc.extend_from_slice(&ch.elem_size.to_le_bytes());
        toc.extend_from_slice(&ch.elem_count.to_le_bytes());
    }
    out.extend_from_slice(&toc);
    while out.len() % 8 != 0 {
        out.push(0);
    }

    // Chunk data.
    for ch in &chunks {
        let want: usize = ch.offset.try_into().map_err(|_| "file too large for this host")?;
        if out.len() > want {
            return Err("internal layout error: overlap".into());
        }
        while out.len() < want {
            out.push(0);
        }
        out.extend_from_slice(&ch.data);
        while out.len() % 8 != 0 {
            out.push(0);
        }
    }

    // Заполняем заголовок.
    if out.len() != file_size_usize {
        return Err("internal size mismatch".into());
    }
    write_header_xnls(
        &mut out,
        file_size,
        toc_offset,
        chunks.len() as u32,
        toc_entry_size as u32,
        ucd_tag,
        ucd_revision,
    );

    Ok(out)
}

fn write_header_xnls(
    buf: &mut [u8],
    file_size: u64,
    toc_offset: u64,
    chunk_count: u32,
    toc_entry_size: u32,
    ucd_tag: [u8; 4],
    ucd_revision: u32,
) {
    // magic
    buf[0x00..0x04].copy_from_slice(b"XNLS");
    // version
    buf[0x04..0x06].copy_from_slice(&1u16.to_le_bytes());
    // endian
    buf[0x06] = 1;
    // header_size
    buf[0x07] = 48;
    // file_size
    buf[0x08..0x10].copy_from_slice(&file_size.to_le_bytes());
    // toc_offset
    buf[0x10..0x18].copy_from_slice(&toc_offset.to_le_bytes());
    // chunk_count (u32)
    buf[0x18..0x1C].copy_from_slice(&chunk_count.to_le_bytes());
    // toc_entry_size (u32)
    buf[0x1C..0x20].copy_from_slice(&toc_entry_size.to_le_bytes());
    // ucd_tag
    buf[0x20..0x24].copy_from_slice(&ucd_tag);
    // ucd_revision
    buf[0x24..0x28].copy_from_slice(&ucd_revision.to_le_bytes());
    // header_crc32 (0 = не используем)
    buf[0x28..0x2C].copy_from_slice(&0u32.to_le_bytes());
    // reserved
    buf[0x2C..0x30].copy_from_slice(&0u32.to_le_bytes());
}

fn u16_table_bytes(t: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(t.len() * 2);
    for &w in t {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out
}

fn align8(v: u64) -> u64 {
    (v + 7) & !7
}

struct Chunk {
    id: [u8; 4],
    offset: u64,
    length: u64,
    flags: u32,
    elem_size: u32,
    elem_count: u32,
    data: Vec<u8>,
}

impl Chunk {
    fn new(id4: &str, data: Vec<u8>, flags: u32, elem_size: u32, elem_count: u32) -> Self {
        let mut id = [0u8; 4];
        let b = id4.as_bytes();
        id.copy_from_slice(&b[0..4]);
        Self {
            id,
            offset: 0,
            length: 0,
            flags,
            elem_size,
            elem_count,
            data,
        }
    }
}

// Примечание: эти импорты сейчас не используются напрямую, но оставлены на будущее расширение формата.
#[allow(dead_code)]
fn _is_under_repo_root(_p: &Path) -> bool {
    // Заглушка под будущие опции “писать в sysroot относительно репо”.
    true
}

