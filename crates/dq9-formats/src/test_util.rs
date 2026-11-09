//! Builders for synthetic formats shared by the crate's tests.

use crate::table::{Record, Table, Value};

/// A record table (`table.md`) holding `records`.
pub(crate) fn encode_table(records: &[Record<'_>]) -> Vec<u8> {
    let (mut body, mut pool) = (Vec::new(), Vec::new());
    for r in records {
        body.extend(r.kind.to_le_bytes());
        body.push(r.values.len() as u8);
        let mut types = vec![0u8; r.values.len().div_ceil(4)];
        for (i, v) in r.values.iter().enumerate() {
            let t = match v {
                Value::Str(_) => 0,
                Value::Int(_) => 1,
                Value::Float(_) => 2,
            };
            types[i / 4] |= t << (2 * (i % 4));
        }
        body.extend(types);
        body.resize(body.len().next_multiple_of(4), 0xff);
        for v in &r.values {
            let raw = match *v {
                Value::Str(Some(s)) => {
                    let at = pool.len() as u32;
                    pool.extend(s);
                    pool.push(0);
                    at
                }
                Value::Str(None) => u32::MAX,
                Value::Int(i) => i,
                Value::Float(f) => f.to_bits(),
            };
            body.extend(raw.to_le_bytes());
        }
    }
    let pool_at = (0x10 + body.len()).next_multiple_of(16);
    let mut out = Vec::new();
    for word in [records.len(), pool_at, pool.len(), 0] {
        out.extend((word as u32).to_le_bytes());
    }
    out.extend(body);
    out.resize(pool_at, 0xff);
    out.extend(pool);
    out.resize(out.len().next_multiple_of(16), 0);
    out
}

/// A table parsed from its records alone (no string pool).
pub(crate) fn table<'a>(records: Vec<Record<'a>>) -> Table<'a> {
    Table {
        records,
        string_count: 0,
    }
}

/// A NARC with named members.
pub(crate) fn narc(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut btaf = (members.len() as u32).to_le_bytes().to_vec();
    let (mut image, mut names) = (Vec::new(), Vec::new());
    for (name, data) in members {
        btaf.extend((image.len() as u32).to_le_bytes());
        image.extend(data);
        btaf.extend((image.len() as u32).to_le_bytes());
        names.push(name.len() as u8);
        names.extend(name.as_bytes());
    }
    names.push(0);
    let btnf = [8u32.to_le_bytes(), [0, 0, 1, 0]].concat();
    let btnf = [btnf, names].concat();
    let mut out = b"NARC".to_vec();
    out.extend(0xFFFEu16.to_le_bytes());
    out.extend(0x0100u16.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(3u16.to_le_bytes());
    for (magic, body) in [(b"BTAF", &btaf), (b"BTNF", &btnf), (b"GMIF", &image)] {
        out.extend(magic);
        out.extend(((body.len() + 8) as u32).to_le_bytes());
        out.extend(body);
    }
    out
}
