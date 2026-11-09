//! `dq9re functions` and `dq9re coverage`: the list of the game's functions and how much of
//! it the port reimplements. Neither needs the ROM, so both run in CI.
//!
//! - `re/functions.txt` lists every function of the European ARM9, ITCM and overlays, built
//!   from the symbol maps of the USA decompilation (DQIX/dqix-decomp, Unlicense) with
//!   addresses moved to the European release (`docs/external-resources.md`), each one
//!   classified game / platform / excluded (`docs/roadmap.md`).
//! - A function counts as ported when a comment of a port crate names it in a `port:` marker:
//!   `// port: 0x020d91ec, ov_017::021ba810, 0x0206974c..=0x02069fd4` (a range takes every
//!   function starting inside it). Every cited address must be a function start.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

/// End of the ARM9 secure area, whose BIOS call stubs are ordered differently per release.
const SECURE_END: u32 = 0x0200_0800;
const ITCM_START: u32 = 0x01FF_8000;
const ITCM_END: u32 = 0x0200_0000;

/// USA main-module address ranges that move in the European release: `(first, end, shift)`.
/// Two functions near the start (USA `0x0200f3a4`, `0x0200fb08`) are 8 bytes longer there.
const EUR_SHIFTS: [(u32, u32, u32); 2] = [
    (0x0200_F3B8, 0x0200_FB20, 0x8),
    (0x0200_FB20, 0x020E_EBD0, 0x10),
];

/// Overlays outside the game proper: debug menus, read-error screen, anti-piracy.
const EXCLUDED_OVERLAYS: [u16; 4] = [7, 19, 22, 29];
/// Overlays replaced by the platform: Nintendo Wi-Fi Connection, DS download play.
const PLATFORM_OVERLAYS: [u16; 2] = [30, 31];
/// C library functions of the main module.
const LIBC: [&str; 18] = [
    "abs", "memcpy", "memmove", "memset", "memchr", "memcmp", "sprintf", "rand", "srand", "strlen",
    "strcpy", "strncpy", "strcat", "strcmp", "strncmp", "strchr", "strrchr", "strstr",
];
/// Startup code of the SDK.
const STARTUP: [&str; 3] = ["Entry", "AutoloadCallback", "BuildInfo"];

/// Individually verified EUR tuples, sorted by address; see `docs/re/nitro-sdk.md`.
/// Names and full symbol sizes must still match; proximity and prefixes prove nothing.
const VERIFIED_SDK: [(u32, u32, &str); 86] = [
    (0x0202_0978, 0x34, "func_02020968"),
    (0x0202_09ac, 0x34, "func_0202099c"),
    (0x0208_c9dc, 0x34, "func_0208c9cc"),
    (0x0209_38dc, 0x24, "func_020938cc"),
    (0x020c_112c, 0xc, "Mat2x2_WriteRotation"),
    (0x020c_1138, 0x58, "Mat2x2_ApplyScale"),
    (0x020c_1190, 0x24, "Mat3x3_WriteIdentity"),
    (0x020c_11b4, 0xc0, "Mat3x3_ApplyScale"),
    (0x020c_1274, 0x1c, "Mat3x3_WriteRotationX"),
    (0x020c_1290, 0x1c, "Mat3x3_WriteRotationY"),
    (0x020c_12ac, 0x18, "Mat3x3_WriteRotationZ"),
    (0x020c_12c4, 0x2f0, "Mat3x3_Invert"),
    (0x020c_15b4, 0x220, "Mat3x3_Multiply"),
    (0x020c_17d4, 0x7c, "Mat3x3_ApplyToVector"),
    (0x020c_1850, 0x28, "Mat4x3_WriteIdentity"),
    (0x020c_1878, 0x34, "Mat4x3_ConvertTo4x4"),
    (0x020c_18ac, 0xac, "Mat4x3_ApplyTranslation"),
    (0x020c_1958, 0x34, "Mat4x3_ApplyScale"),
    (0x020c_198c, 0x20, "Mat4x3_WriteRotationX"),
    (0x020c_19ac, 0x1c, "Mat4x3_WriteRotationY"),
    (0x020c_19c8, 0x1c, "Mat4x3_WriteRotationZ"),
    (0x020c_19e4, 0x38c, "Mat4x3_Invert"),
    (0x020c_1d70, 0x2d4, "Mat4x3_Multiply"),
    (0x020c_2044, 0xa0, "Mat4x3_ApplyToVector"),
    (0x020c_20e4, 0x108, "Mat4x3_WriteViewMatrix"),
    (0x020c_21ec, 0x2c, "Mat4x4_WriteIdentity"),
    (0x020c_2218, 0x34, "Mat4x4_ConvertTo4x3"),
    (0x020c_224c, 0x664, "Mat4x4_Multiply"),
    (0x020c_28b0, 0x14c, "Mat4x4_MaybeWriteFrustum"),
    (0x020c_29fc, 0x208, "Mat4x4_WriteProjectionUnknown"),
    (0x020c_2c04, 0x10, "fix32_Divide"),
    (0x020c_2c14, 0x34, "fix32_Sqrt"),
    (0x020c_2c48, 0x24, "GetHardwareDividerResult"),
    (0x020c_2c6c, 0x38, "fix32_GetDivisionResult"),
    (0x020c_2ca4, 0x30, "fix32_QueueComputeReciprocal"),
    (0x020c_2cd4, 0x2c, "fix32_GetSqrtResult"),
    (0x020c_2d00, 0x28, "fix32_QueueComputeQuotient"),
    (0x020c_2d28, 0x3c, "FastIntDivide"),
    (0x020c_2d64, 0x3c, "FastIntModulus"),
    (0x020c_2da0, 0x34, "Vector3fix_Add"),
    (0x020c_2dd4, 0x34, "Vector3fix_Subtract"),
    (0x020c_2e08, 0x3c, "Vector3fix_InnerProduct"),
    (0x020c_2e44, 0x84, "Vector3fix_CrossProduct"),
    (0x020c_2ec8, 0x60, "Vector3fix_Length"),
    (0x020c_2f28, 0x118, "Vector3fix_Normalize"),
    (0x020c_3040, 0x7c, "Vector3fix_Distance"),
    (0x020c_339c, 0x1b8, "fix32_Atan2"),
    (0x020c_384c, 0x28, "func_020c383c"),
    (0x020c_3874, 0x34, "func_020c3864"),
    (0x020c_3994, 0x1c, "func_020c3984"),
    (0x020c_5598, 0x28, "func_020c5588"),
    (0x020c_5660, 0x30, "func_020c5650"),
    (0x020c_5690, 0x30, "func_020c5680"),
    (0x020c_56c0, 0x2c, "func_020c56b0"),
    (0x020c_7d44, 0x4c, "SaveContext"),
    (0x020c_7d90, 0x44, "RestoreContext"),
    (0x020c_89d4, 0x10, "func_020c89c4"),
    (0x020c_89e4, 0x10, "func_020c89d4"),
    (0x020c_89f4, 0x14, "func_020c89e4"),
    (0x020c_8a08, 0x10, "func_020c89f8"),
    (0x020c_8a18, 0x10, "func_020c8a08"),
    (0x020c_8a28, 0x14, "func_020c8a18"),
    (0x020c_8a3c, 0x08, "func_020c8a2c"),
    (0x020c_8a44, 0x08, "func_020c8a34"),
    (0x020c_9c00, 0x0c, "func_020c9bf0"),
    (0x020c_a5a4, 0xbc, "VectorizedMemset"),
    (0x020c_a660, 0x180, "VectorizedInvertedMemcpy"),
    (0x020c_a96c, 0xb4, "DecompressC"),
    (0x020c_aa20, 0x184, "DecompressA"),
    (0x020c_aba4, 0x16c, "DecompressB"),
    (0x020c_edf0, 0x1c, "func_020cede0"),
    (0x020d_af30, 0x1c, "func_020daf20"),
    (0x020d_af64, 0x18, "func_020daf54"),
    (0x020d_c018, 0x18, "func_020dc008"),
    (0x020e_29c8, 0x34, "func_020e29b8"),
    (0x020e_29fc, 0x34, "func_020e29ec"),
    (0x020e_2a30, 0x34, "func_020e2a20"),
    (0x020e_2a64, 0x34, "func_020e2a54"),
    (0x020e_2a98, 0x28, "func_020e2a88"),
    (0x020e_2ac0, 0x28, "func_020e2ab0"),
    (0x020e_2ae8, 0x28, "func_020e2ad8"),
    (0x020e_2b10, 0x28, "func_020e2b00"),
    (0x020e_2b38, 0x28, "func_020e2b28"),
    (0x020e_2b60, 0x28, "func_020e2b50"),
    (0x020e_2b88, 0x1c, "func_020e2b78"),
    (0x020e_2bbc, 0x18, "func_020e2bac"),
];

pub const README_START: &str = "<!-- coverage:start -->";
pub const README_END: &str = "<!-- coverage:end -->";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Module {
    Main,
    Itcm,
    Overlay(u16),
}

impl fmt::Display for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Module::Main => f.write_str("ARM9"),
            Module::Itcm => f.write_str("ITCM"),
            Module::Overlay(n) => write!(f, "ov_{n:03}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Level-5 game logic: to be translated.
    Game,
    /// Nintendo libraries and runtime: replaced, not translated.
    Platform,
    /// Not part of the game (debug, anti-piracy).
    Excluded,
}

impl Category {
    fn as_str(self) -> &'static str {
        match self {
            Category::Game => "game",
            Category::Platform => "platform",
            Category::Excluded => "excluded",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "game" => Some(Category::Game),
            "platform" => Some(Category::Platform),
            "excluded" => Some(Category::Excluded),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub module: Module,
    pub addr: u32,
    pub size: u32,
    pub category: Category,
    pub name: String,
}

/// Address as written in our sources and `re/symbols.txt`.
fn format_addr(module: Module, addr: u32) -> String {
    match module {
        Module::Overlay(n) => format!("ov_{n:03}::{addr:08x}"),
        Module::Main | Module::Itcm => format!("{addr:#010x}"),
    }
}

/// Parses `0x020d91ec`, `FUN_020d91ec` or `ov_017::0218c0f4`.
fn parse_addr(token: &str) -> Option<(Module, u32)> {
    if let Some(rest) = token.strip_prefix("ov_") {
        let (n, addr) = rest.split_once("::")?;
        let n = n.parse().ok()?;
        return Some((Module::Overlay(n), u32::from_str_radix(addr, 16).ok()?));
    }
    let hex = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("FUN_"))?;
    let addr = u32::from_str_radix(hex, 16).ok()?;
    let module = if (ITCM_START..ITCM_END).contains(&addr) {
        Module::Itcm
    } else {
        Module::Main
    };
    Some((module, addr))
}

/// USA main-module address → European one.
fn usa_to_eur(addr: u32) -> u32 {
    EUR_SHIFTS
        .iter()
        .find(|(first, end, _)| (*first..*end).contains(&addr))
        .map_or(addr, |(_, _, shift)| addr + shift)
}

fn hex_field(line: &str, key: &str) -> Option<u32> {
    let start = line.find(key)? + key.len();
    let digits: String = line[start..]
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect();
    u32::from_str_radix(&digits, 16).ok()
}

/// Functions of one dsd `symbols.txt`: `(name, USA address, size)`.
fn dsd_functions(text: &str) -> Vec<(String, u32, u32)> {
    text.lines()
        .filter(|l| l.contains("kind:function("))
        .filter_map(|l| {
            let name = l.split_whitespace().next()?.to_string();
            Some((name, hex_field(l, "addr:0x")?, hex_field(l, "size=0x")?))
        })
        .collect()
}

/// `.text` ranges of the files under `src/NitroSDK/` in one dsd `delinks.txt`.
fn sdk_ranges(text: &str) -> Vec<(u32, u32)> {
    let mut sdk = false;
    let mut ranges = Vec::new();
    for line in text.lines() {
        if !line.starts_with(char::is_whitespace) && line.ends_with(':') {
            sdk = line.starts_with("src/NitroSDK/");
        } else if sdk
            && line.trim_start().starts_with(".text")
            && let (Some(start), Some(end)) =
                (hex_field(line, "start:0x"), hex_field(line, "end:0x"))
        {
            ranges.push((start, end));
        }
    }
    ranges
}

fn classify(module: Module, usa_addr: u32, name: &str, sdk: &[(u32, u32)]) -> Category {
    if let Module::Overlay(n) = module {
        if EXCLUDED_OVERLAYS.contains(&n) {
            return Category::Excluded;
        }
        if PLATFORM_OVERLAYS.contains(&n) {
            return Category::Platform;
        }
    }
    let runtime = name.starts_with('_') && !name.starts_with("_Z") && !name.starts_with("__sinit_");
    if (module == Module::Main && usa_addr < SECURE_END)
        || sdk.iter().any(|&(s, e)| (s..e).contains(&usa_addr))
        || LIBC.contains(&name)
        || STARTUP.contains(&name)
        || runtime
    {
        Category::Platform
    } else {
        Category::Game
    }
}

fn verified_sdk(module: Module, eur_addr: u32, eur_size: u32, name: &str) -> bool {
    module == Module::Main
        && VERIFIED_SDK
            .binary_search_by_key(&eur_addr, |&(addr, _, _)| addr)
            .is_ok_and(|index| {
                let (_, size, source_name) = VERIFIED_SDK[index];
                size == eur_size && source_name == name
            })
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

/// Commit checked out in a git working tree, if it can be read without running git.
fn git_head(root: &Path) -> Option<String> {
    let head = fs::read_to_string(root.join(".git/HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: ") {
        Some(r) => fs::read_to_string(root.join(".git").join(r))
            .ok()
            .map(|s| s.trim().to_string()),
        None => Some(head.to_string()),
    }
}

/// Builds the function list from a dqix-decomp checkout.
pub fn from_decomp(root: &Path) -> Result<Vec<Function>> {
    let config = root.join("config/usa/arm9");
    let mut modules = vec![
        (Module::Main, config.clone()),
        (Module::Itcm, config.join("itcm")),
    ];
    let overlays = config.join("overlays");
    let mut entries: Vec<_> = fs::read_dir(&overlays)
        .with_context(|| format!("reading {}", overlays.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for dir in entries {
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if let Some(n) = name.strip_prefix("ov").and_then(|n| n.parse().ok()) {
            modules.push((Module::Overlay(n), dir));
        }
    }

    let mut functions = Vec::new();
    for (module, dir) in modules {
        let symbols = read(&dir.join("symbols.txt"))?;
        let delinks = dir.join("delinks.txt");
        let sdk = if delinks.exists() {
            sdk_ranges(&read(&delinks)?)
        } else {
            Vec::new()
        };
        for (name, addr, size) in dsd_functions(&symbols) {
            let (eur, eur_size) = if module == Module::Main && addr >= SECURE_END {
                let eur = usa_to_eur(addr);
                (eur, usa_to_eur(addr + size.max(1) - 1) + 1 - eur)
            } else {
                (addr, size)
            };
            let category = if verified_sdk(module, eur, eur_size, &name) {
                Category::Platform
            } else {
                classify(module, addr, &name, &sdk)
            };
            functions.push(Function {
                module,
                addr: eur,
                size: eur_size,
                category,
                name,
            });
        }
    }
    functions.sort_by_key(|f| (f.module, f.addr));
    ensure!(
        !functions.is_empty(),
        "no function found under {}",
        config.display()
    );
    Ok(functions)
}

pub fn write_functions(functions: &[Function], commit: Option<&str>) -> String {
    let mut out = format!(
        "# Functions of Dragon Quest IX (European release YDQP), generated by\n\
         # `cargo dq9re functions <dqix-decomp checkout>` from the USA symbol maps of\n\
         # https://github.com/DQIX/dqix-decomp (commit {}, Unlicense), addresses moved to the\n\
         # European release. The secure area (below 0x02000800) keeps the USA order.\n\
         # <address> <size> <game|platform|excluded> <name>\n",
        commit.unwrap_or("unknown")
    );
    for f in functions {
        out += &format!(
            "{} {:#x} {} {}\n",
            format_addr(f.module, f.addr),
            f.size,
            f.category.as_str(),
            f.name
        );
    }
    out
}

pub fn parse_functions(text: &str) -> Result<Vec<Function>> {
    let mut functions = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        let parsed = match fields.as_slice() {
            [addr, size, category, name] => parse_addr(addr).and_then(|(module, addr)| {
                Some(Function {
                    module,
                    addr,
                    size: u32::from_str_radix(size.strip_prefix("0x")?, 16).ok()?,
                    category: Category::parse(category)?,
                    name: (*name).to_string(),
                })
            }),
            _ => None,
        };
        functions.push(parsed.with_context(|| format!("functions line {}: {line:?}", i + 1))?);
    }
    Ok(functions)
}

/// One `port:` citation: a single function or an inclusive range of starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Citation {
    One(Module, u32),
    Range(Module, u32, u32),
}

/// Position of a `port:` marker that is a word of its own (not `import:` or `report:`).
fn marker(comment: &str) -> Option<usize> {
    comment.match_indices("port:").map(|(p, _)| p).find(|&p| {
        comment[..p]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_')
    })
}

/// Citations of the `port:` markers in one source file's comments.
pub fn citations(source: &str) -> Result<Vec<(usize, Citation)>> {
    let mut found = Vec::new();
    for (i, line) in source.lines().enumerate() {
        let Some(comment) = line.find("//").map(|p| &line[p + 2..]) else {
            continue;
        };
        let Some(pos) = marker(comment) else {
            continue;
        };
        for token in comment[pos + 5..]
            .split(|c: char| c == ',' || c == '`' || c.is_whitespace())
            .filter(|t| !t.is_empty())
        {
            let citation = match token.split_once("..=") {
                Some((a, b)) => match (parse_addr(a), parse_addr(b)) {
                    (Some((m, a)), Some((n, b))) if m == n && a <= b => {
                        Some(Citation::Range(m, a, b))
                    }
                    _ => None,
                },
                None => parse_addr(token).map(|(m, a)| Citation::One(m, a)),
            };
            match citation {
                Some(c) => found.push((i + 1, c)),
                None => bail!("line {}: `port:` expects addresses, got {token:?}", i + 1),
            }
        }
    }
    Ok(found)
}

/// Rust sources of the port crates (every crate except the tools).
fn port_sources(crates: &Path) -> Result<Vec<PathBuf>> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                walk(&path, out)?;
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(crates).with_context(|| format!("reading {}", crates.display()))? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() && !matches!(name, "dq9re" | "testrom") {
            walk(&path, &mut out)?;
        }
    }
    out.sort();
    Ok(out)
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Tally {
    pub functions: usize,
    pub bytes: u64,
    pub ported_functions: usize,
    pub ported_bytes: u64,
}

impl Tally {
    fn add(&mut self, size: u32, ported: bool) {
        self.functions += 1;
        self.bytes += u64::from(size);
        if ported {
            self.ported_functions += 1;
            self.ported_bytes += u64::from(size);
        }
    }
}

/// Game-function progress per module, from the functions and the ported set.
pub fn tally(functions: &[Function], ported: &[bool]) -> BTreeMap<Module, Tally> {
    let mut modules = BTreeMap::new();
    for (f, &p) in functions.iter().zip(ported) {
        if f.category == Category::Game {
            modules
                .entry(f.module)
                .or_insert_with(Tally::default)
                .add(f.size, p);
        }
    }
    modules
}

/// Marks the functions cited by the sources; errors name every citation that is not a
/// function start.
pub fn mark_ported(functions: &[Function], sources: &[(String, String)]) -> Result<Vec<bool>> {
    let index: HashMap<(Module, u32), usize> = functions
        .iter()
        .enumerate()
        .map(|(i, f)| ((f.module, f.addr), i))
        .collect();
    let mut ported = vec![false; functions.len()];
    let mut errors = Vec::new();
    for (file, text) in sources {
        for (line, citation) in citations(text).with_context(|| file.clone())? {
            match citation {
                Citation::One(m, a) => match index.get(&(m, a)) {
                    Some(&i) => ported[i] = true,
                    None => errors.push(format!(
                        "{file}:{line}: {} is not a function start",
                        format_addr(m, a)
                    )),
                },
                Citation::Range(m, a, b) => {
                    for (i, f) in functions.iter().enumerate() {
                        if f.module == m && (a..=b).contains(&f.addr) {
                            ported[i] = true;
                        }
                    }
                }
            }
        }
    }
    if !errors.is_empty() {
        bail!("invalid `port:` citations:\n{}", errors.join("\n"));
    }
    Ok(ported)
}

fn percent(part: u64, whole: u64) -> String {
    let tenths = (part * 1000).checked_div(whole).unwrap_or(0);
    format!("{}.{}%", tenths / 10, tenths % 10)
}

fn kib(bytes: u64) -> String {
    format!("{} KB", group(bytes.div_ceil(1024) as usize))
}

/// Thousands separator: `12142` → `12,142`.
fn group(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// shields.io static badge for the ported share of game functions, colored by progress.
fn badge(part: u64, whole: u64) -> String {
    let tenths = (part * 1000).checked_div(whole).unwrap_or(0);
    let color = match tenths {
        0..250 => "orange",
        250..750 => "yellow",
        750..1000 => "yellowgreen",
        _ => "brightgreen",
    };
    let label = percent(part, whole).replace('%', "%25");
    format!(
        "[![Ported](https://img.shields.io/badge/ported-{label}%20of%20functions-{color})]\
         (docs/roadmap.md)"
    )
}

/// Markdown block shown in the README, between `README_START` and `README_END`.
pub fn report(modules: &BTreeMap<Module, Tally>) -> String {
    let mut total = Tally::default();
    for t in modules.values() {
        total.functions += t.functions;
        total.bytes += t.bytes;
        total.ported_functions += t.ported_functions;
        total.ported_bytes += t.ported_bytes;
    }
    let mut out = format!(
        "{}\n\n\
         **Progress: {}** of game functions ported ({} / {}), {} of the code ({} / {}).\n\
         Computed by `cargo dq9re coverage` from the `port:` markers in the code \
         (definition in `docs/roadmap.md`).\n\n\
         <details><summary>Per-module detail</summary>\n\n\
         | Module | Functions ported | Code ported |\n|---|---:|---:|\n",
        badge(total.ported_functions as u64, total.functions as u64),
        percent(total.ported_functions as u64, total.functions as u64),
        group(total.ported_functions),
        group(total.functions),
        percent(total.ported_bytes, total.bytes),
        kib(total.ported_bytes),
        kib(total.bytes),
    );
    for (module, t) in modules {
        out += &format!(
            "| {module} | {} / {} ({}) | {} / {} ({}) |\n",
            group(t.ported_functions),
            group(t.functions),
            percent(t.ported_functions as u64, t.functions as u64),
            kib(t.ported_bytes),
            kib(t.bytes),
            percent(t.ported_bytes, t.bytes),
        );
    }
    out + "\n</details>\n"
}

/// Replaces the text between the README markers (exclusive) with `block`.
pub fn splice_readme(readme: &str, block: &str) -> Result<String> {
    let start = readme
        .find(README_START)
        .with_context(|| format!("{README_START} not found"))?
        + README_START.len();
    let end = readme[start..]
        .find(README_END)
        .with_context(|| format!("{README_END} not found"))?
        + start;
    Ok(format!("{}\n{block}{}", &readme[..start], &readme[end..]))
}

pub fn run_functions(decomp: &Path, out: &Path) -> Result<()> {
    let functions = from_decomp(decomp)?;
    let commit = git_head(decomp);
    fs::write(out, write_functions(&functions, commit.as_deref()))
        .with_context(|| format!("writing {}", out.display()))?;
    let count = |c| functions.iter().filter(|f| f.category == c).count();
    println!(
        "{} functions ({} game, {} platform, {} excluded) -> {}",
        functions.len(),
        count(Category::Game),
        count(Category::Platform),
        count(Category::Excluded),
        out.display()
    );
    Ok(())
}

pub fn run_coverage(
    functions: &Path,
    crates: &Path,
    readme: Option<&Path>,
    check: bool,
) -> Result<()> {
    let functions = parse_functions(&read(functions)?)?;
    let sources = port_sources(crates)?
        .into_iter()
        .map(|p| Ok((p.display().to_string(), read(&p)?)))
        .collect::<Result<Vec<_>>>()?;
    let ported = mark_ported(&functions, &sources)?;
    let block = report(&tally(&functions, &ported));
    print!("{block}");
    if let Some(path) = readme {
        let current = read(path)?;
        let updated = splice_readme(&current, &block)?;
        if check {
            ensure!(
                current == updated,
                "{} is out of date: run `cargo dq9re coverage --readme {}`",
                path.display(),
                path.display()
            );
        } else if current != updated {
            fs::write(path, updated).with_context(|| format!("writing {}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_sdk_requires_the_exact_main_module_tuple() {
        assert!(verified_sdk(
            Module::Main,
            0x020c_112c,
            0xc,
            "Mat2x2_WriteRotation"
        ));
        for (module, addr, size, name) in [
            (Module::Main, 0x020c_1130, 0xc, "Mat2x2_WriteRotation"),
            (Module::Main, 0x020c_112c, 0x10, "Mat2x2_WriteRotation"),
            (Module::Main, 0x020c_112c, 0xc, "Mat2x2_Unverified"),
            (Module::Overlay(0), 0x020c_112c, 0xc, "Mat2x2_WriteRotation"),
            (Module::Itcm, 0x020c_112c, 0xc, "Mat2x2_WriteRotation"),
            (Module::Main, 0x020c_0eac, 0x294, "func_020c0e9c"),
            (
                Module::Main,
                0x0203_0e3c,
                0x5c,
                "_Z24Vector3fixMultiplyScalarPK8Vector3iiPS_",
            ),
        ] {
            assert!(!verified_sdk(module, addr, size, name));
            assert_eq!(classify(module, addr, name, &[]), Category::Game);
        }
    }

    #[test]
    fn eur_addresses_follow_the_two_inserted_blocks() {
        assert_eq!(usa_to_eur(0x0200_F3A4), 0x0200_F3A4);
        assert_eq!(usa_to_eur(0x0200_F3B8), 0x0200_F3C0);
        assert_eq!(usa_to_eur(0x020D_91DC), 0x020D_91EC);
        assert_eq!(usa_to_eur(0x020F_27C8), 0x020F_27C8);
    }

    #[test]
    fn dsd_symbols_are_classified_and_moved() {
        let symbols = "\
Div kind:function(thumb,size=0x4) addr:0x02000100
memcpy kind:function(arm,size=0x20) addr:0x02001000
func_0200f3a4 kind:function(arm,size=0x20) addr:0x0200f3a4
OS_Init kind:function(arm,size=0x10) addr:0x02090000
_ZN7GPCFile12GetFileByCRCEj kind:function(arm,size=0xb4) addr:0x020d901c
data_020f27c8 kind:data(any) addr:0x020f27c8
";
        let delinks = "    .text start:0x02000000 end:0x020e5920 kind:code align:32\n\n\
src/NitroSDK/OS.cpp:\n    complete\n    .text start:0x02090000 end:0x02090010\n";
        let sdk = sdk_ranges(delinks);
        assert_eq!(sdk, [(0x0209_0000, 0x0209_0010)]);
        let got: Vec<_> = dsd_functions(symbols)
            .into_iter()
            .map(|(name, addr, _)| classify(Module::Main, addr, &name, &sdk))
            .collect();
        use Category::*;
        assert_eq!(got, [Platform, Platform, Game, Platform, Game]);
        assert_eq!(
            classify(Module::Overlay(31), 0x0215_0000, "func", &[]),
            Platform
        );
        assert_eq!(
            classify(Module::Overlay(7), 0x0218_0000, "func", &[]),
            Excluded
        );
    }

    #[test]
    fn markers_cite_functions_and_ranges() {
        let source = "\
//! port: `0x020d91ec`, ov_017::021ba810
fn f() {} // port: FUN_02069ffc 0x0206974c..=0x02069fd4
/// Calls `Trig_Fire` (0x020649c0); not a marker.
";
        let got: Vec<_> = citations(source)
            .unwrap()
            .into_iter()
            .map(|(_, c)| c)
            .collect();
        assert_eq!(
            got,
            [
                Citation::One(Module::Main, 0x020d_91ec),
                Citation::One(Module::Overlay(17), 0x021b_a810),
                Citation::One(Module::Main, 0x0206_9ffc),
                Citation::Range(Module::Main, 0x0206_974c, 0x0206_9fd4),
            ]
        );
        assert!(citations("// port: Trig_Fire").is_err());
        assert!(
            citations("// import: report: not markers")
                .unwrap()
                .is_empty()
        );
    }

    fn function(module: Module, addr: u32, size: u32, category: Category) -> Function {
        Function {
            module,
            addr,
            size,
            category,
            name: "f".into(),
        }
    }

    #[test]
    fn tally_counts_game_functions_only_and_rejects_unknown_starts() {
        let functions = [
            function(Module::Main, 0x100, 0x10, Category::Game),
            function(Module::Main, 0x110, 0x30, Category::Game),
            function(Module::Main, 0x140, 0x40, Category::Platform),
            function(Module::Overlay(17), 0x100, 0x20, Category::Game),
        ];
        let text = write_functions(&functions, None);
        assert_eq!(parse_functions(&text).unwrap(), functions);

        let sources = [(
            "a.rs".to_string(),
            "// port: 0x00000100..=0x00000140".to_string(),
        )];
        let ported = mark_ported(&functions, &sources).unwrap();
        assert_eq!(ported, [true, true, true, false]);
        let modules = tally(&functions, &ported);
        assert_eq!(
            modules[&Module::Main],
            Tally {
                functions: 2,
                bytes: 0x40,
                ported_functions: 2,
                ported_bytes: 0x40
            }
        );
        assert_eq!(modules[&Module::Overlay(17)].ported_functions, 0);

        let bad = [("a.rs".to_string(), "// port: 0x00000104".to_string())];
        let err = mark_ported(&functions, &bad).unwrap_err().to_string();
        assert!(
            err.contains("a.rs:1: 0x00000104 is not a function start"),
            "{err}"
        );
    }

    #[test]
    fn readme_block_is_replaced_between_markers() {
        let readme = format!("intro\n{README_START}\nold\n{README_END}\nrest\n");
        let updated = splice_readme(&readme, "new\n").unwrap();
        assert_eq!(
            updated,
            format!("intro\n{README_START}\nnew\n{README_END}\nrest\n")
        );
        assert_eq!(splice_readme(&updated, "new\n").unwrap(), updated);
        assert_eq!(percent(1, 3), "33.3%");
        assert_eq!(percent(0, 0), "0.0%");
        assert_eq!(group(12142), "12,142");
        assert_eq!(group(999), "999");
        assert!(badge(21, 1000).contains("ported-2.1%25%20of%20functions-orange"));
        assert!(badge(500, 1000).ends_with("-yellow)](docs/roadmap.md)"));
    }

    /// Every named function of `re/symbols.txt` must start a function of the list: checks the
    /// USA → Europe address conversion against our own findings.
    #[test]
    fn our_symbols_start_listed_functions() {
        // Run-time CARGO_MANIFEST_DIR: a shared target directory can reuse a test binary
        // compiled in another worktree, whose compile-time path may no longer exist.
        let manifest = std::env::var_os("CARGO_MANIFEST_DIR").map_or_else(
            || std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            std::path::PathBuf::from,
        );
        let root = manifest.join("../..");
        let functions = parse_functions(&read(&root.join("re/functions.txt")).unwrap()).unwrap();
        let starts: std::collections::HashSet<_> =
            functions.iter().map(|f| (f.module, f.addr)).collect();
        let symbols = read(&root.join("re/symbols.txt")).unwrap();
        let mut checked = 0;
        for line in symbols.lines() {
            let mut fields = line.split_whitespace();
            let (Some(addr), Some(name)) = (fields.next(), fields.next()) else {
                continue;
            };
            // Data symbols (`g_…`) are not functions.
            if line.starts_with('#') || name.starts_with("g_") {
                continue;
            }
            let (module, addr) = parse_addr(addr).unwrap();
            assert!(
                starts.contains(&(module, addr)),
                "{name} at {} is not listed",
                format_addr(module, addr)
            );
            checked += 1;
        }
        assert!(checked > 100, "only {checked} symbols checked");
    }
}
