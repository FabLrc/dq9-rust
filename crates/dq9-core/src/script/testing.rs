//! Builds synthetic `.stb` files for the tests of the interpreter.

pub(crate) const RECORD: usize = 0x38;

/// One instruction, with jump and call targets given symbolically.
#[derive(Clone, Copy)]
pub(crate) enum T {
    Op(u32, u32, u32),
    /// Jump family (`0x10`, `0x11`, `0x12`) to instruction `index` of the same function.
    Jump(u32, usize, u32),
    /// Call of function `index` of the file.
    Call(usize),
}

pub(crate) const LOAD: u32 = 1;
pub(crate) const ADDRESS: u32 = 2;
pub(crate) const POP: u32 = 4;
pub(crate) const ASSIGN: u32 = 5;
pub(crate) const ADD: u32 = 6;
pub(crate) const SUB: u32 = 7;
pub(crate) const MUL: u32 = 8;
pub(crate) const DIV: u32 = 9;
pub(crate) const MOD: u32 = 0xa;
pub(crate) const NEG: u32 = 0xb;
pub(crate) const TOFLOAT: u32 = 0xc;
pub(crate) const TOINT: u32 = 0xd;
pub(crate) const CMP: u32 = 0xe;
pub(crate) const RETURN: u32 = 0xf;
pub(crate) const JUMP: u32 = 0x10;
pub(crate) const JF: u32 = 0x11;
pub(crate) const JT: u32 = 0x12;
pub(crate) const NATIVE: u32 = 0x15;
pub(crate) const YIELD: u32 = 0x17;
pub(crate) const AND: u32 = 0x18;
pub(crate) const OR: u32 = 0x19;
pub(crate) const NOT: u32 = 0x1a;
pub(crate) const END: u32 = 0x1b;
pub(crate) const STEP: u32 = 0x1c;

pub(crate) fn int(v: i32) -> T {
    T::Op(3, 1, v as u32)
}

pub(crate) fn float(v: f32) -> T {
    T::Op(3, 2, v.to_bits())
}

pub(crate) fn op(op: u32) -> T {
    T::Op(op, 0, 0)
}

pub(crate) fn addr(index: u32) -> T {
    T::Op(ADDRESS, index, 1)
}

pub(crate) fn var(index: u32) -> T {
    T::Op(LOAD, index, 1)
}

/// Values an instruction pushes minus those it pops, in expression code.
fn depth(t: &T) -> i32 {
    match *t {
        T::Op(3, ..) => 1,
        T::Op(LOAD | ADDRESS, _, scope) => i32::from(scope & 0x36 == 0),
        T::Op(ADD..=MOD | CMP | AND | OR | ASSIGN, ..) => -1,
        _ => 0,
    }
}

/// `native(id, args…)`: the code of the arguments (one value each), then the call.
pub(crate) fn native(id: i32, args: &[T]) -> Vec<T> {
    let mut code = vec![int(id)];
    code.extend_from_slice(args);
    let n = 1 + args.iter().map(depth).sum::<i32>();
    code.push(T::Op(NATIVE, n as u32, 0));
    code
}

pub(crate) struct Func {
    pub frame: u32,
    pub params: u32,
    pub code: Vec<T>,
}

pub(crate) fn func(frame: u32, params: u32, code: Vec<T>) -> Func {
    Func {
        frame,
        params,
        code,
    }
}

/// A file with the given functions (code base 0), entries `(id, function index)` and the
/// default record at function 0.
pub(crate) fn build(globals: u32, funcs: &[Func], entries: &[(i32, usize)]) -> Vec<u8> {
    let table = 0x20;
    let first = table + 8 * entries.len();
    let mut at = first;
    let mut offsets = Vec::new();
    for f in funcs {
        offsets.push(at);
        at += RECORD + 12 * f.code.len();
    }
    let mut out = Vec::new();
    let words = [
        u32::from_le_bytes(*b"SB2\0"),
        offsets[0] as u32,
        0,
        table as u32,
        entries.len() as u32,
        0,
        globals,
        0,
    ];
    for w in words {
        out.extend(w.to_le_bytes());
    }
    for &(id, f) in entries {
        out.extend((id as u32).to_le_bytes());
        out.extend((offsets[f] as u32).to_le_bytes());
    }
    for (f, &base) in funcs.iter().zip(&offsets) {
        let mut record = [0u32; RECORD / 4];
        record[0] = (base + RECORD) as u32;
        record[2] = f.frame;
        record[3] = f.params;
        for w in record {
            out.extend(w.to_le_bytes());
        }
        for t in &f.code {
            let [a, b, c] = match *t {
                T::Op(o, a, b) => [o, a, b],
                T::Jump(o, index, keep) => [o, (base + RECORD + 12 * index) as u32, keep],
                T::Call(index) => [0x13, 0, offsets[index] as u32],
            };
            for w in [a, b, c] {
                out.extend(w.to_le_bytes());
            }
        }
    }
    out
}

/// A `GPC2` archive with raw (uncompressed) members and stored index and name blocks.
pub(crate) fn gp2(members: &[(&str, &[u8])]) -> Vec<u8> {
    const HEADER: usize = 0x14;
    let stored = |body: &[u8]| {
        let mut b = ((body.len() as u32) << 3).to_le_bytes().to_vec();
        b.extend(body);
        b.resize(b.len().next_multiple_of(4), 0);
        b
    };
    let (mut names, mut blobs) = (Vec::new(), Vec::new());
    let mut index: Vec<(u32, u32, u32)> = Vec::new();
    for (name, body) in members {
        let name_offset = names.len() as u32;
        names.extend(name.as_bytes());
        names.push(0);
        let location = (blobs.len() / 4) as u32 | (name_offset & 0xff) << 24;
        let size = body.len() as u32 | (name_offset >> 8) << 24;
        index.push((crc32fast::hash(name.as_bytes()), location, size));
        blobs.extend(*body);
        blobs.resize(blobs.len().next_multiple_of(4), 0);
    }
    index.sort_by_key(|e| e.0);
    let index: Vec<u8> = index
        .iter()
        .flat_map(|(h, l, s)| [h.to_le_bytes(), l.to_le_bytes(), s.to_le_bytes()].concat())
        .collect();
    let (index, names) = (stored(&index), stored(&names));
    let names_offset = HEADER + index.len();
    let data_offset = names_offset + names.len();
    let mut out = b"GPC2".to_vec();
    out.extend((members.len() as u16).to_le_bytes());
    for words in [HEADER / 4, names_offset / 4, data_offset / 4, 0, 0] {
        out.extend((words as u16).to_le_bytes());
    }
    out.extend(((blobs.len() / 4) as u32 | 1 << 28).to_le_bytes());
    out.extend(index);
    out.extend(names);
    out.extend(blobs);
    out
}
