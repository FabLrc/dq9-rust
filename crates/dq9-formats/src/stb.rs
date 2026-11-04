//! Event scripts (`evNNNNN.stb`, magic `SB2`), the compiled scripts started by trigger
//! action 0x77 and run by the stack machine at `ov_017::021d4e38`. Layout and evidence in
//! `docs/re/stb.md`.
//!
//! [`Script::parse`] follows the code from every entry point (recursive descent through
//! jumps and calls, like the interpreter), so data embedded between functions (string
//! constants) is never decoded as code.

use std::collections::{BTreeMap, BTreeSet};

use nds_rom::{Error, Result};

const MAGIC: &[u8; 4] = b"SB2\0";
/// Size of a function record; its code follows it (`record + 0x38 == code base + code`).
pub const RECORD_SIZE: usize = 0x38;
const INSTR_SIZE: usize = 12;

/// Where a variable lives (`b` of instructions 1 and 2). The float forms retag the slot as a
/// float when it is read or addressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Slot `index` of the current frame (1).
    Local,
    /// Frame slot `index + popped int` (2).
    LocalIndexed,
    /// Element `popped int` of the array whose address is in frame slot `index` (4).
    Indirect,
    LocalFloat,
    LocalFloatIndexed,
    IndirectFloat,
    /// Script global `index` (0x40).
    Global,
    GlobalFloat,
}

impl Scope {
    fn from_raw(raw: u32) -> Option<Self> {
        Some(match raw {
            1 => Self::Local,
            2 => Self::LocalIndexed,
            4 => Self::Indirect,
            8 => Self::LocalFloat,
            0x10 => Self::LocalFloatIndexed,
            0x20 => Self::IndirectFloat,
            0x40 => Self::Global,
            0x200 => Self::GlobalFloat,
            _ => return None,
        })
    }

    /// Whether the address computation pops an index.
    fn indexed(self) -> bool {
        matches!(
            self,
            Self::LocalIndexed | Self::Indirect | Self::LocalFloatIndexed | Self::IndirectFloat
        )
    }
}

/// Comparison of instruction 0xe (`a` = 0x28 + variant).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// One decoded instruction (`[op, a, b]`, three little-endian words). Jump and call targets
/// are absolute file offsets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Instr {
    /// 0.
    Nop,
    /// 1: push the value of a variable.
    Load {
        index: u32,
        scope: Scope,
    },
    /// 2: push the address of a variable.
    Address {
        index: u32,
        scope: Scope,
    },
    /// 3: push a constant (`a` = 1 int, 2 float, 3 string).
    Int(i32),
    Float(f32),
    /// Offset of a NUL-terminated string from the code base ([`Script::string`]).
    Str(u32),
    /// 4: drop the top value.
    Pop,
    /// 5: pop a value and an address, store, push the value back.
    Assign,
    /// 6 to 0xa: binary arithmetic (int, or float if either side is a float; 0xa ints only).
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    /// 0xb.
    Neg,
    /// 0xc: int to float.
    ToFloat,
    /// 0xd: float to int.
    ToInt,
    /// 0xe: pushes 0 or 1.
    Compare(Cmp),
    /// 0xf: return the top value; ends the script at the outermost frame.
    Return,
    /// 0x10.
    Jump(usize),
    /// 0x11: pop; jump if zero, pushing the 0 back when `keep` (`&&`).
    JumpIfFalse {
        target: usize,
        keep: bool,
    },
    /// 0x12: pop; jump if non-zero, pushing the 1 back when `keep` (`||`).
    JumpIfTrue {
        target: usize,
        keep: bool,
    },
    /// 0x13: call the script function whose record is at `function`; `a` is kept raw.
    Call {
        function: usize,
        a: u32,
    },
    /// 0x14: pop `n` values and hand them to a debug output routine.
    Print(u32),
    /// 0x15: pop `args` values and call native `first value` with the others; pushes
    /// nothing.
    Native {
        args: u32,
    },
    /// 0x16: no effect at run time; `a` is a marker **[unknown]**.
    Mark(u32),
    /// 0x17: suspend until the next frame.
    Yield,
    /// 0x18, 0x19: bitwise on ints.
    And,
    Or,
    /// 0x1a: logical not of an int.
    Not,
    /// 0x1b: end the script.
    End,
    /// 0x1c: counts a step (`+0x50`) and resumes after a suspension **[unknown]**.
    Step,
    /// 0x1d, 0x1e.
    Sin,
    Cos,
}

impl Instr {
    fn decode(op: u32, a: u32, b: u32, code_base: usize) -> Result<Self> {
        let target = |off: u32| code_base + off as usize;
        let scope = || Scope::from_raw(b).ok_or(Error::Malformed("stb: unknown variable scope"));
        Ok(match op {
            0 => Self::Nop,
            1 => Self::Load {
                index: a,
                scope: scope()?,
            },
            2 => Self::Address {
                index: a,
                scope: scope()?,
            },
            3 => match a {
                1 => Self::Int(b as i32),
                2 => Self::Float(f32::from_bits(b)),
                3 => Self::Str(b),
                _ => return Err(Error::Malformed("stb: unknown constant type")),
            },
            4 => Self::Pop,
            5 => Self::Assign,
            6 => Self::Add,
            7 => Self::Sub,
            8 => Self::Mul,
            9 => Self::Div,
            0xa => Self::Mod,
            0xb => Self::Neg,
            0xc => Self::ToFloat,
            0xd => Self::ToInt,
            0xe => Self::Compare(match a {
                0x28 => Cmp::Eq,
                0x29 => Cmp::Ne,
                0x2a => Cmp::Lt,
                0x2b => Cmp::Le,
                0x2c => Cmp::Gt,
                0x2d => Cmp::Ge,
                _ => return Err(Error::Malformed("stb: unknown comparison")),
            }),
            0xf => Self::Return,
            0x10 => Self::Jump(target(a)),
            0x11 => Self::JumpIfFalse {
                target: target(a),
                keep: b != 0,
            },
            0x12 => Self::JumpIfTrue {
                target: target(a),
                keep: b != 0,
            },
            0x13 => Self::Call {
                function: target(b),
                a,
            },
            0x14 => Self::Print(a),
            0x15 => Self::Native { args: a },
            0x16 => Self::Mark(a),
            0x17 => Self::Yield,
            0x18 => Self::And,
            0x19 => Self::Or,
            0x1a => Self::Not,
            0x1b => Self::End,
            0x1c => Self::Step,
            0x1d => Self::Sin,
            0x1e => Self::Cos,
            _ => return Err(Error::Malformed("stb: unknown opcode")),
        })
    }

    /// Whether execution never falls through to the next instruction.
    fn terminates(&self) -> bool {
        matches!(self, Self::Jump(_) | Self::Return | Self::End)
    }

    /// `(popped, pushed)` values for straight-line code, `None` for control flow and calls
    /// whose effect depends on the callee.
    pub fn stack_effect(&self) -> Option<(u32, u32)> {
        Some(match self {
            Self::Nop | Self::Mark(_) | Self::Step => (0, 0),
            Self::Load { scope, .. } | Self::Address { scope, .. } => {
                (u32::from(scope.indexed()), 1)
            }
            Self::Int(_) | Self::Float(_) | Self::Str(_) => (0, 1),
            Self::Pop => (1, 0),
            Self::Assign
            | Self::Add
            | Self::Sub
            | Self::Mul
            | Self::Div
            | Self::Mod
            | Self::Compare(_)
            | Self::And
            | Self::Or => (2, 1),
            Self::Neg | Self::ToFloat | Self::ToInt | Self::Not | Self::Sin | Self::Cos => (1, 1),
            Self::Print(n) | Self::Native { args: n } => (*n, 0),
            _ => return None,
        })
    }
}

/// A function: its 0x38-byte record (`[code offset, ?, frame slots, parameters]`, zeros)
/// and the instructions reachable from its code.
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    /// File offset of the record.
    pub record: usize,
    /// Frame slots (parameters included).
    pub frame: u32,
    pub params: u32,
    /// Reachable instructions by file offset, in address order.
    pub code: Vec<(usize, Instr)>,
}

impl Function {
    /// Native calls: `(index in code, id)`, the id (first argument) when it is a constant
    /// expression (see [`Function::args`]).
    pub fn native_calls(&self) -> Vec<(usize, Option<i32>)> {
        let mut calls = Vec::new();
        for (i, (_, instr)) in self.code.iter().enumerate() {
            if let Instr::Native { .. } = instr {
                calls.push((i, self.args(i).and_then(|a| a.first().copied().flatten())));
            }
        }
        calls
    }

    /// Values consumed by instruction `at` (a native call or print), bottom first, each
    /// `Some` when it is a constant expression of `Int`, `Add` and `Sub`; `None` when the
    /// arguments are not computed by straight-line code just before the call.
    pub fn args(&self, at: usize) -> Option<Vec<Option<i32>>> {
        let (args, _) = self.code.get(at)?.1.stack_effect()?;
        // Walk back over straight-line code until the `args` values are accounted for.
        let mut need = i64::from(args);
        let mut start = at;
        while need > 0 {
            let j = start.checked_sub(1)?;
            let (pc, instr) = self.code[j];
            if pc + INSTR_SIZE != self.code[j + 1].0 {
                return None;
            }
            let (pops, pushes) = instr.stack_effect()?;
            need -= i64::from(pushes) - i64::from(pops);
            start = j;
            if need < 0 {
                return None;
            }
        }
        // Evaluate forward; the first argument is the bottom of what was pushed.
        let mut stack: Vec<Option<i32>> = Vec::new();
        for (_, instr) in &self.code[start..at] {
            match *instr {
                Instr::Int(v) => stack.push(Some(v)),
                Instr::Add | Instr::Sub => {
                    let (b, a) = (stack.pop()?, stack.pop()?);
                    stack.push(match (a, b, *instr) {
                        (Some(a), Some(b), Instr::Add) => Some(a.wrapping_add(b)),
                        (Some(a), Some(b), _) => Some(a.wrapping_sub(b)),
                        _ => None,
                    });
                }
                other => {
                    let (pops, pushes) = other.stack_effect()?;
                    for _ in 0..pops {
                        stack.pop()?;
                    }
                    stack.extend(std::iter::repeat_n(None, pushes as usize));
                }
            }
        }
        (stack.len() == args as usize).then_some(stack)
    }
}

/// A parsed `.stb` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Script<'a> {
    data: &'a [u8],
    /// Header word 2: offsets of code and constants are relative to it.
    pub code_base: usize,
    /// Header word 6: number of script globals.
    pub globals: u32,
    /// Header word 1: record run when no entry id is given (`ov_017::021d4ce4`, id < 0).
    pub default_entry: usize,
    /// Header words 3/4: `(id, record offset)` pairs.
    pub entries: Vec<(i32, usize)>,
    /// Functions reachable from the entries, by record offset.
    pub functions: BTreeMap<usize, Function>,
}

fn word(data: &[u8], offset: usize) -> Result<u32> {
    data.get(offset..offset + 4)
        .map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")))
        .ok_or(Error::OutOfBounds {
            offset,
            len: 4,
            size: data.len(),
        })
}

impl<'a> Script<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        if data.get(..4) != Some(MAGIC) {
            let mut found = [0; 4];
            let n = data.len().min(4);
            found[..n].copy_from_slice(&data[..n]);
            return Err(Error::BadMagic { what: "SB2", found });
        }
        let header = |i: usize| word(data, i * 4);
        let code_base = header(2)? as usize;
        let default_entry = header(1)? as usize;
        let (table, count) = (header(3)? as usize, header(4)? as usize);
        let entries = (0..count)
            .map(|i| {
                Ok((
                    word(data, table + 8 * i)? as i32,
                    word(data, table + 8 * i + 4)? as usize,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut script = Script {
            data,
            code_base,
            globals: header(6)?,
            default_entry,
            entries,
            functions: BTreeMap::new(),
        };
        let mut pending: Vec<usize> = script.entries.iter().map(|&(_, r)| r).collect();
        pending.push(default_entry);
        while let Some(record) = pending.pop() {
            if script.functions.contains_key(&record) {
                continue;
            }
            let function = script.decode_function(record)?;
            for (_, instr) in &function.code {
                if let Instr::Call { function, .. } = *instr {
                    pending.push(function);
                }
            }
            script.functions.insert(record, function);
        }
        Ok(script)
    }

    fn decode_function(&self, record: usize) -> Result<Function> {
        let code = word(self.data, record)? as usize;
        if self.code_base + code != record + RECORD_SIZE {
            return Err(Error::Malformed("stb: code does not follow its record"));
        }
        let mut seen = BTreeMap::new();
        let mut pending = vec![record + RECORD_SIZE];
        let mut targets = BTreeSet::new();
        while let Some(mut pc) = pending.pop() {
            while !seen.contains_key(&pc) {
                let [op, a, b] = [0, 4, 8].map(|i| word(self.data, pc + i));
                let instr = Instr::decode(op?, a?, b?, self.code_base)?;
                seen.insert(pc, instr);
                match instr {
                    Instr::Jump(t)
                    | Instr::JumpIfFalse { target: t, .. }
                    | Instr::JumpIfTrue { target: t, .. } => {
                        targets.insert(t);
                        pending.push(t);
                    }
                    _ => {}
                }
                if instr.terminates() {
                    break;
                }
                pc += INSTR_SIZE;
            }
        }
        if targets
            .iter()
            .any(|t| !(t - self.code_base).is_multiple_of(4))
        {
            return Err(Error::Malformed("stb: misaligned jump"));
        }
        Ok(Function {
            record,
            frame: word(self.data, record + 8)?,
            params: word(self.data, record + 12)?,
            code: seen.into_iter().collect(),
        })
    }

    /// String constant at `offset` from the code base (without its NUL).
    pub fn string(&self, offset: u32) -> Option<&'a [u8]> {
        let start = self.code_base.checked_add(offset as usize)?;
        let rest = self.data.get(start..)?;
        Some(&rest[..rest.iter().position(|&b| b == 0)?])
    }
}

/// Natives whose role is established (`docs/re/stb.md`); the others are numbered only.
pub fn native_name(id: i32) -> Option<&'static str> {
    Some(match id {
        400 => "message",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A script with one entry (id 7) whose function calls a second one.
    fn sample() -> Vec<u8> {
        let mut words: Vec<u32> = vec![u32::from_le_bytes(*MAGIC), 0, 0, 0, 1, 0, 2, 0];
        // Code base = 0x40 (entry table at 0x20, one (id, record) pair, padding).
        words[2] = 0x40;
        words[3] = 0x20;
        words.extend([7, 0x40, 0, 0, 0, 0, 0, 0]);
        let record = |code: usize, frame: u32, params: u32, out: &mut Vec<u32>| {
            out.extend([code as u32, 0, frame, params]);
            out.extend([0; 10]); // record = 0x38 bytes
        };
        // Function 1 at 0x40: native 400 + 2 with a string argument, call function 2, end.
        record(0x38, 1, 0, &mut words);
        let f2 = 0x40 + 0x38 + 8 * 12;
        words[1] = f2 as u32; // default entry = function 2
        let code1 = [
            [3, 1, 400],
            [3, 1, 2],
            [6, 0, 0],
            [3, 3, 0], // "hi", placed after function 2
            [0x15, 2, 0],
            [0x11, 0x38 + 7 * 12, 0], // skips the call (forward jump)
            [0x13, 0, (f2 - 0x40) as u32],
            [0x1b, 0, 0],
        ];
        words.extend(code1.iter().flatten());
        // Function 2: local 0 = 1.5; return it.
        record(f2 + 0x38 - 0x40, 1, 1, &mut words);
        words.extend(
            [[2, 0, 1], [3, 2, 1.5f32.to_bits()], [5, 0, 0], [0xf, 0, 0]]
                .iter()
                .flatten(),
        );
        let mut bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let strings = bytes.len() - 0x40;
        bytes.extend(b"hi\0\0");
        // Patch the string offset.
        let at = 0x40 + 0x38 + 3 * 12 + 8;
        bytes[at..at + 4].copy_from_slice(&(strings as u32).to_le_bytes());
        bytes
    }

    #[test]
    fn follows_entries_calls_and_jumps() {
        let data = sample();
        let script = Script::parse(&data).unwrap();
        assert_eq!(script.entries, [(7, 0x40)]);
        assert_eq!(script.globals, 2);
        assert_eq!(script.functions.len(), 2);
        let f1 = &script.functions[&0x40];
        assert_eq!(f1.code.len(), 8);
        let Instr::Str(s) = f1.code[3].1 else {
            panic!("{:?}", f1.code[3])
        };
        assert_eq!(script.string(s), Some(&b"hi"[..]));
        // The native id is folded from `400 + 2`.
        assert_eq!(f1.native_calls(), [(4, Some(402))]);
        let f2 = &script.functions[&script.default_entry];
        assert_eq!((f2.frame, f2.params), (1, 1));
        assert_eq!(f2.code[1].1, Instr::Float(1.5));
        assert_eq!(f2.code.last().unwrap().1, Instr::Return);
    }

    #[test]
    fn rejects_unknown_opcodes_and_bad_magic() {
        let mut data = sample();
        let at = 0x40 + 0x38;
        data[at..at + 4].copy_from_slice(&0x1fu32.to_le_bytes());
        assert!(Script::parse(&data).is_err());
        assert!(matches!(
            Script::parse(b"SB1\0"),
            Err(Error::BadMagic { .. })
        ));
    }
}
