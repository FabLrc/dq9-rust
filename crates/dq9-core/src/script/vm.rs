//! The `.stb` stack machine, translated from the game's interpreter (`ov_017`, the
//! `Stb_*` functions of `re/symbols.txt`). Layout, instruction set and evidence in
//! `docs/re/stb.md`.
//!
//! The game keeps one memory area per script: the globals first, then the value stack made
//! of `(type, value)` pairs, with the frames of the running functions inside it. Address
//! values are pointers into it. [`Vm`] keeps the same layout in a `Vec<Slot>` and uses slot
//! indices as addresses, so a native that writes a result through an address argument
//! (`StbArg_SetInt`) behaves like the original.
//!
//! The interpreter never touches the engine: instruction 0x15 hands the native id and its
//! arguments to a [`Natives`] implementation (`Stb_CallNative`, `ov_017::021d4bc8`).
//!
//! port: ov_017::021d4980, ov_017::021d49ac, ov_017::021d4a38, ov_017::021d4a64
//! port: ov_017::021d4a78, ov_017::021d4a98, ov_017::021d4ab8, ov_017::021d4ad8
//! port: ov_017::021d4af8, ov_017::021d4b9c, ov_017::021d4bc8
//! port: ov_017::021d4ccc, ov_017::021d4ce4, ov_017::021d4e38, ov_017::021d60f4
//! port: ov_017::021d6110, ov_017::021d612c, ov_017::021d6134, ov_017::021d6148

use std::collections::BTreeMap;
use std::fmt;

use dq9_formats::stb::{self, Cmp, Instr, RECORD_SIZE, Scope};
use nds_rom::Result;

/// Type words of a stack value (`Stb_Push*`): the first word of a slot.
pub const INT: u32 = 0;
pub const FLOAT: u32 = 1;
pub const STR: u32 = 2;
pub const ADDR: u32 = 3;

/// Call stack depth (`Stb_Init` is given 0x200 frames of 12 bytes by `ov_001::021538d4`).
const MAX_CALLS: usize = 0x200;
/// Slots of the value stack, globals included: `ov_001::021538d4` gives `Stb_Init` a buffer
/// of 0x400 bytes (0x80 values). The game does not check it; a script that overflowed would
/// corrupt memory there, here it faults.
const MAX_SLOTS: usize = 0x80;

/// A value of the stack: `(type, value word)`, 8 bytes in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Slot {
    pub ty: u32,
    pub val: u32,
}

impl Slot {
    pub fn int(v: i32) -> Self {
        Self {
            ty: INT,
            val: v as u32,
        }
    }

    pub fn float(v: f32) -> Self {
        Self {
            ty: FLOAT,
            val: v.to_bits(),
        }
    }

    fn f32(self) -> f32 {
        f32::from_bits(self.val)
    }

    fn i32(self) -> i32 {
        self.val as i32
    }

    /// `Stb_Truthy` (`ov_017::021d49ac`): a float zero is true, only `(0, 0)` is false.
    fn truthy(self) -> bool {
        self.ty != 0 || self.val != 0
    }
}

/// What stopped a script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    /// A popped value had the wrong type: `FUN_0200159c(-1)`, the game's fatal error.
    Type,
    /// An integer division or remainder by zero.
    DivideByZero,
    StackUnderflow,
    StackOverflow,
    /// `FUN_0200159c(-2)`: more than 0x200 nested calls.
    CallOverflow,
    /// The program counter is not on a decoded instruction.
    BadPc,
    /// An address value (or an indexed variable) points outside the script memory.
    BadAddress,
    /// A call to a function record that is not in the script.
    BadCall,
    /// `Stb_Start` found no record for the entry number (it returns -1).
    NoEntry,
    /// The instruction budget of one resumption ran out: the script loops without yielding.
    Budget,
}

/// A fault with the offset of the instruction that raised it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault {
    pub kind: FaultKind,
    pub pc: usize,
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.kind {
            FaultKind::Type => "operand of the wrong type",
            FaultKind::DivideByZero => "division by zero",
            FaultKind::StackUnderflow => "value stack underflow",
            FaultKind::StackOverflow => "value stack overflow",
            FaultKind::CallOverflow => "call stack overflow",
            FaultKind::BadPc => "jump outside the decoded code",
            FaultKind::BadAddress => "address outside the script memory",
            FaultKind::BadCall => "call of an unknown function",
            FaultKind::NoEntry => "no such entry",
            FaultKind::Budget => "no yield within the instruction budget",
        };
        write!(f, "{what} at {:#x}", self.pc)
    }
}

impl std::error::Error for Fault {}

/// A parsed script, owning its bytes: the instructions reachable from the entries, flat and
/// sorted by file offset, and the function records.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    data: Vec<u8>,
    code_base: usize,
    globals: usize,
    default_entry: usize,
    entries: Vec<(i32, usize)>,
    code: Vec<(usize, Instr)>,
    /// Record offset → `(frame slots, parameters)`.
    records: BTreeMap<usize, (u32, u32)>,
}

impl Program {
    /// Parses a `.stb` file (`Stb_Init` checks the `SB2` magic, `ov_017::021d4c04`).
    pub fn parse(data: Vec<u8>) -> Result<Self> {
        let (code_base, globals, default_entry, entries, code, records) = {
            let script = stb::Script::parse(&data)?;
            let mut code: Vec<(usize, Instr)> = script
                .functions
                .values()
                .flat_map(|f| f.code.iter().copied())
                .collect();
            code.sort_by_key(|&(pc, _)| pc);
            code.dedup_by_key(|&mut (pc, _)| pc);
            let records = script
                .functions
                .values()
                .map(|f| (f.record, (f.frame, f.params)))
                .collect();
            (
                script.code_base,
                script.globals as usize,
                script.default_entry,
                script.entries.clone(),
                code,
                records,
            )
        };
        Ok(Self {
            data,
            code_base,
            globals,
            default_entry,
            entries,
            code,
            records,
        })
    }

    /// Entry numbers of the header table.
    pub fn entries(&self) -> impl Iterator<Item = i32> + '_ {
        self.entries.iter().map(|&(id, _)| id)
    }

    /// Number of globals (header word 6).
    pub fn globals(&self) -> usize {
        self.globals
    }

    fn instr(&self, pc: usize) -> Option<Instr> {
        let i = self.code.binary_search_by_key(&pc, |&(p, _)| p).ok()?;
        Some(self.code[i].1)
    }

    /// NUL-terminated string at file offset `at` (the value word of a type 2 slot).
    pub fn string(&self, at: u32) -> Option<&[u8]> {
        let rest = self.data.get(at as usize..)?;
        Some(&rest[..rest.iter().position(|&b| b == 0)?])
    }
}

/// The arguments of a native call, without the native number (`Stb_CallNative` passes
/// `args + 1`, `n - 1`). Reads are those of `StbArg_*`: the value word is reinterpreted, only
/// the int/float conversions look at the type.
#[derive(Debug, Clone, Copy)]
pub struct Args<'a> {
    slots: &'a [Slot],
    program: &'a Program,
}

impl<'a> Args<'a> {
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn slot(&self, i: usize) -> Option<Slot> {
        self.slots.get(i).copied()
    }

    /// `StbArg_Int` (`ov_017::021d60f4`): a float is truncated. A missing argument reads 0
    /// (the game would read whatever follows on its stack).
    pub fn int(&self, i: usize) -> i32 {
        match self.slot(i) {
            Some(s) if s.ty == FLOAT => s.f32() as i32,
            Some(s) => s.i32(),
            None => 0,
        }
    }

    /// `StbArg_Float` (`ov_017::021d6110`): an int is converted.
    pub fn float(&self, i: usize) -> f32 {
        match self.slot(i) {
            Some(s) if s.ty == INT => s.i32() as f32,
            Some(s) => s.f32(),
            None => 0.0,
        }
    }

    /// `StbArg_Str` (`ov_017::021d612c`): the string a type 2 argument points at.
    pub fn str(&self, i: usize) -> Option<&'a [u8]> {
        let s = self.slot(i).filter(|s| s.ty == STR)?;
        self.program.string(s.val)
    }

    /// Whether argument `i` is a string (`type 2`).
    pub fn is_str(&self, i: usize) -> bool {
        self.slot(i).is_some_and(|s| s.ty == STR)
    }

    /// Whether argument `i` is an address, i.e. an output.
    pub fn is_addr(&self, i: usize) -> bool {
        self.slot(i).is_some_and(|s| s.ty == ADDR)
    }

    pub fn int_or(&self, i: usize, default: i32) -> i32 {
        if i < self.len() { self.int(i) } else { default }
    }
}

/// Writes through address arguments, applied when the native returns.
#[derive(Debug, Default)]
pub struct Outs {
    writes: Vec<(u32, u32)>,
}

impl Outs {
    /// `StbArg_SetInt` (`ov_017::021d6134`): writes the value word of the slot argument `i`
    /// points at, only if it is an address; the type word is left alone.
    pub fn int(&mut self, args: &Args, i: usize, value: i32) {
        self.word(args, i, value as u32);
    }

    /// `StbArg_SetFloat` (`ov_017::021d6148`).
    pub fn float(&mut self, args: &Args, i: usize, value: f32) {
        self.word(args, i, value.to_bits());
    }

    fn word(&mut self, args: &Args, i: usize, word: u32) {
        if let Some(s) = args.slot(i).filter(|s| s.ty == ADDR) {
            self.writes.push((s.val, word));
        }
    }
}

/// The engine side of instruction 0x15.
pub trait Natives {
    /// Native `id` (`g_StbNatives`, 0 to 999); the game ignores ids without a handler.
    fn call(&mut self, id: i32, args: &Args, out: &mut Outs);
}

/// Why [`Vm::run`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Suspended by instruction 0x17 (or 0x1c after a skip); resume with [`Vm::run`].
    Yielded,
    /// Instruction 0x1b, or 0xf in the outermost frame.
    Finished,
}

/// A saved caller (`Stb_Call` pushes 12 bytes: return address, frame, function).
#[derive(Debug, Clone, Copy)]
struct Call {
    ret: usize,
    frame: usize,
    func: Option<usize>,
}

/// The state of the interpreter (the `Stb` object of `ov_017`).
#[derive(Debug, Clone)]
pub struct Vm {
    /// Globals, then the value stack.
    mem: Vec<Slot>,
    /// First stack slot (`+0x10`).
    base: usize,
    /// Stack top (`+0x14`).
    sp: usize,
    /// Frame base (`+0x30`).
    frame: usize,
    /// Function record of the running frame (`+0x34`).
    func: Option<usize>,
    calls: Vec<Call>,
    /// Instruction being run (`+0x38`); `None` when stopped.
    pc: Option<usize>,
    /// `+0x3c`.
    finished: bool,
    /// `+0x40`: while set, jumps, natives and yields are skipped up to the next 0x1c.
    skip: bool,
    /// `+0x50`: instructions 0x1c run.
    steps: u32,
    /// `+0x4c`: value word returned by the outermost function.
    result: u32,
}

impl Vm {
    /// `Stb_Init` (`ov_017::021d4c04`): globals zeroed, stack empty.
    pub fn new(program: &Program) -> Self {
        Self {
            mem: vec![Slot::default(); program.globals],
            base: program.globals,
            sp: program.globals,
            frame: program.globals,
            func: None,
            calls: Vec::new(),
            pc: None,
            finished: false,
            skip: false,
            steps: 0,
            result: 0,
        }
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Whether the script is suspended or about to start.
    pub fn is_running(&self) -> bool {
        self.pc.is_some()
    }

    /// Value word returned by the outermost function (`+0x4c`).
    pub fn result(&self) -> u32 {
        self.result
    }

    /// Number of instructions 0x1c run (`+0x50`).
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// Sets `+0x40`: until the next instruction 0x1c, jumps, natives and yields do nothing.
    pub fn set_skip(&mut self, skip: bool) {
        self.skip = skip;
    }

    /// The global variable `index`, for tests and tools.
    pub fn global(&self, index: usize) -> Option<Slot> {
        self.mem.get(index).copied().filter(|_| index < self.base)
    }

    fn fault<T>(&self, kind: FaultKind, pc: usize) -> core::result::Result<T, Fault> {
        Err(Fault { kind, pc })
    }

    /// `Stb_Start` (`ov_017::021d4ce4`), without its first slice: positions the machine on
    /// the record of entry `entry` (the header's default record when negative). The globals
    /// keep their values. [`Vm::run`] then does what the rest of `Stb_Start` does.
    pub fn start(&mut self, program: &Program, entry: i32) -> core::result::Result<(), Fault> {
        let record = if entry < 0 {
            Some(program.default_entry)
        } else {
            program
                .entries
                .iter()
                .find(|&&(id, _)| id == entry)
                .map(|&(_, r)| r)
        };
        let Some(record) = record.filter(|r| program.records.contains_key(r)) else {
            return self.fault(FaultKind::NoEntry, 0);
        };
        self.mem.truncate(self.base);
        self.sp = self.base;
        self.calls.clear();
        self.func = None;
        self.enter(program, record, 0)?;
        self.finished = false;
        self.skip = false;
        self.steps = 0;
        self.pc = Some(record + RECORD_SIZE);
        Ok(())
    }

    /// The frame setup shared by `Stb_Start` and `Stb_Call`: the last `params` values of the
    /// stack become the first slots of the frame, the others are zeroed.
    fn enter(
        &mut self,
        program: &Program,
        record: usize,
        pc: usize,
    ) -> core::result::Result<(), Fault> {
        let Some(&(slots, params)) = program.records.get(&record) else {
            return self.fault(FaultKind::BadCall, pc);
        };
        let (slots, params) = (slots as usize, params as usize);
        let Some(frame) = self.sp.checked_sub(params).filter(|&f| f >= self.base) else {
            return self.fault(FaultKind::StackUnderflow, pc);
        };
        let top = frame + slots.max(params);
        if top > MAX_SLOTS {
            return self.fault(FaultKind::StackOverflow, pc);
        }
        self.mem.resize(top.max(self.mem.len()), Slot::default());
        for slot in &mut self.mem[frame + params..frame + slots] {
            *slot = Slot::default();
        }
        self.frame = frame;
        self.sp = frame + slots;
        self.func = Some(record);
        Ok(())
    }

    fn pop(&mut self, pc: usize) -> core::result::Result<Slot, Fault> {
        if self.sp <= self.base {
            return self.fault(FaultKind::StackUnderflow, pc);
        }
        self.sp -= 1;
        Ok(self.mem[self.sp])
    }

    /// `Stb_Push` (`ov_017::021d4a38`).
    fn push(&mut self, slot: Slot, pc: usize) -> core::result::Result<(), Fault> {
        if self.sp >= MAX_SLOTS {
            return self.fault(FaultKind::StackOverflow, pc);
        }
        if self.sp == self.mem.len() {
            self.mem.push(slot);
        } else {
            self.mem[self.sp] = slot;
        }
        self.sp += 1;
        Ok(())
    }

    /// `Stb_ToInt` (`ov_017::021d4980`): the value must be an int.
    fn pop_int(&mut self, pc: usize) -> core::result::Result<i32, Fault> {
        let s = self.pop(pc)?;
        if s.ty != INT {
            return self.fault(FaultKind::Type, pc);
        }
        Ok(s.i32())
    }

    /// Index of the variable `index` of `scope` (instructions 1 and 2). The indexed scopes
    /// pop their index first.
    fn address(
        &mut self,
        index: u32,
        scope: Scope,
        pc: usize,
    ) -> core::result::Result<usize, Fault> {
        let index = index as usize;
        let at = match scope {
            Scope::Local | Scope::LocalFloat => self.frame + index,
            Scope::LocalIndexed | Scope::LocalFloatIndexed => {
                let i = self.pop_int(pc)?;
                (self.frame + index).wrapping_add(i as isize as usize)
            }
            Scope::Indirect | Scope::IndirectFloat => {
                let i = self.pop_int(pc)?;
                let Some(array) = self.mem.get(self.frame + index) else {
                    return self.fault(FaultKind::BadAddress, pc);
                };
                (array.val as usize).wrapping_add(i as isize as usize)
            }
            Scope::Global | Scope::GlobalFloat => index,
        };
        if at >= self.mem.len() {
            return self.fault(FaultKind::BadAddress, pc);
        }
        Ok(at)
    }

    /// Resumes the script until it yields or ends (`Stb_Resume` and the rest of `Stb_Start`,
    /// `ov_017::021d4ccc`, `021d4ce4`). `budget` bounds the instructions of this slice.
    pub fn run(
        &mut self,
        program: &Program,
        natives: &mut dyn Natives,
        budget: u32,
    ) -> core::result::Result<Status, Fault> {
        let Some(mut pc) = self.pc else {
            return Ok(Status::Finished);
        };
        let mut left = budget;
        loop {
            // A fault stops the script where the game's fatal error would.
            let step = self.step(program, natives, pc, &mut left);
            match step {
                Ok(Flow::Next(next)) => pc = next,
                Ok(Flow::Yield(next)) => {
                    self.pc = Some(next);
                    return Ok(Status::Yielded);
                }
                Ok(Flow::End) => {
                    self.finished = true;
                    self.pc = None;
                    return Ok(Status::Finished);
                }
                Err(fault) => {
                    self.pc = None;
                    return Err(fault);
                }
            }
        }
    }

    /// One iteration of the `Stb_Run` dispatch loop (`ov_017::021d4e38`) at `pc`.
    fn step(
        &mut self,
        program: &Program,
        natives: &mut dyn Natives,
        pc: usize,
        left: &mut u32,
    ) -> core::result::Result<Flow, Fault> {
        if *left == 0 {
            return self.fault(FaultKind::Budget, pc);
        }
        *left -= 1;
        let Some(instr) = program.instr(pc) else {
            return self.fault(FaultKind::BadPc, pc);
        };
        let next = pc + 12;
        match instr {
            Instr::Nop | Instr::Mark(_) | Instr::Print(_) => {
                if let Instr::Print(n) = instr {
                    // The debug output (`FUN_02001878`) has no effect on the script.
                    for _ in 0..n {
                        self.pop(pc)?;
                    }
                }
            }
            Instr::Load { index, scope } => {
                let at = self.address(index, scope, pc)?;
                // The float forms retag the slot (`*(slot) = 1`) before pushing it.
                if matches!(
                    scope,
                    Scope::LocalFloat
                        | Scope::LocalFloatIndexed
                        | Scope::IndirectFloat
                        | Scope::GlobalFloat
                ) {
                    self.mem[at].ty = FLOAT;
                }
                let value = self.mem[at];
                self.push(value, pc)?;
            }
            Instr::Address { index, scope } => {
                let at = self.address(index, scope, pc)?;
                self.push(
                    Slot {
                        ty: ADDR,
                        val: at as u32,
                    },
                    pc,
                )?;
            }
            Instr::Int(v) => self.push(Slot::int(v), pc)?,
            Instr::Float(v) => self.push(Slot::float(v), pc)?,
            Instr::Str(offset) => self.push(
                Slot {
                    ty: STR,
                    val: (program.code_base as u32).wrapping_add(offset),
                },
                pc,
            )?,
            Instr::Pop => {
                self.pop(pc)?;
            }
            Instr::Assign => {
                let value = self.pop(pc)?;
                let target = self.pop(pc)?;
                let Some(dest) = self.mem.get_mut(target.val as usize) else {
                    return self.fault(FaultKind::BadAddress, pc);
                };
                *dest = value;
                self.push(value, pc)?;
            }
            Instr::Add | Instr::Sub | Instr::Mul | Instr::Div => self.arithmetic(instr, pc)?,
            Instr::Mod => {
                let b = self.pop_int(pc)?;
                let a = self.pop_int(pc)?;
                if b == 0 {
                    return self.fault(FaultKind::DivideByZero, pc);
                }
                self.push(Slot::int(a.wrapping_rem(b)), pc)?;
            }
            Instr::Neg => {
                let s = self.pop(pc)?;
                let r = match s.ty {
                    INT => Slot::int(s.i32().wrapping_neg()),
                    FLOAT => Slot::float(0.0 - s.f32()),
                    _ => return self.fault(FaultKind::Type, pc),
                };
                self.push(r, pc)?;
            }
            Instr::ToFloat => {
                let s = self.pop(pc)?;
                let r = match s.ty {
                    INT => Slot::float(s.i32() as f32),
                    FLOAT => s,
                    _ => return self.fault(FaultKind::Type, pc),
                };
                self.push(r, pc)?;
            }
            Instr::ToInt => {
                let s = self.pop(pc)?;
                let r = match s.ty {
                    INT => s,
                    FLOAT => Slot::int(s.f32() as i32),
                    _ => return self.fault(FaultKind::Type, pc),
                };
                self.push(r, pc)?;
            }
            Instr::Compare(cmp) => self.compare(cmp, pc)?,
            Instr::Return => {
                let value = self.pop(pc)?;
                if self.calls.is_empty() {
                    // Outermost frame: the script ends, the value word is kept (`+0x4c`).
                    self.result = value.val;
                    self.push(value, pc)?;
                    return Ok(Flow::End);
                }
                // `Stb_Return` (`ov_017::021d4b9c`).
                self.sp = self.frame;
                let call = self.calls.pop().expect("checked above");
                self.frame = call.frame;
                self.func = call.func;
                self.push(value, pc)?;
                return Ok(Flow::Next(call.ret + 12));
            }
            Instr::Jump(target) => {
                if !self.skip {
                    return Ok(Flow::Next(target));
                }
            }
            Instr::JumpIfFalse { target, keep } => {
                if !self.skip && !self.pop(pc)?.truthy() {
                    if keep {
                        self.push(Slot::int(0), pc)?;
                    }
                    return Ok(Flow::Next(target));
                }
            }
            Instr::JumpIfTrue { target, keep } => {
                if !self.skip && self.pop(pc)?.truthy() {
                    if keep {
                        self.push(Slot::int(1), pc)?;
                    }
                    return Ok(Flow::Next(target));
                }
            }
            Instr::Call { function, .. } => {
                // `Stb_Call` (`ov_017::021d4af8`).
                if self.calls.len() >= MAX_CALLS {
                    return self.fault(FaultKind::CallOverflow, pc);
                }
                let caller = Call {
                    ret: pc,
                    frame: self.frame,
                    func: self.func,
                };
                self.enter(program, function, pc)?;
                self.calls.push(caller);
                return Ok(Flow::Next(function + RECORD_SIZE));
            }
            Instr::Native { args } => {
                let n = args as usize;
                let Some(first) = self.sp.checked_sub(n).filter(|&f| f >= self.base) else {
                    return self.fault(FaultKind::StackUnderflow, pc);
                };
                self.sp = first;
                if !self.skip && n > 0 {
                    let slots: Vec<Slot> = self.mem[first..first + n].to_vec();
                    let id = slots[0].i32();
                    // `Stb_CallNative`: ids outside the table are ignored.
                    if (0..1000).contains(&id) {
                        let mut out = Outs::default();
                        natives.call(
                            id,
                            &Args {
                                slots: &slots[1..],
                                program,
                            },
                            &mut out,
                        );
                        for (at, word) in out.writes {
                            if let Some(slot) = self.mem.get_mut(at as usize) {
                                slot.val = word;
                            }
                        }
                    }
                }
            }
            Instr::Yield => {
                if !self.skip {
                    return Ok(Flow::Yield(next));
                }
            }
            Instr::And | Instr::Or => {
                let b = self.pop_int(pc)?;
                let a = self.pop_int(pc)?;
                let r = if instr == Instr::And { a & b } else { a | b };
                self.push(Slot::int(r), pc)?;
            }
            Instr::Not => {
                let s = self.pop(pc)?;
                if s.ty != INT {
                    return self.fault(FaultKind::Type, pc);
                }
                self.push(Slot::int(i32::from(s.val == 0)), pc)?;
            }
            Instr::End => return Ok(Flow::End),
            Instr::Step => {
                self.steps += 1;
                if self.skip {
                    self.skip = false;
                    return Ok(Flow::Yield(next));
                }
            }
            Instr::Sin | Instr::Cos => {
                let s = self.pop(pc)?;
                let x = match s.ty {
                    INT => s.i32() as f32,
                    FLOAT => s.f32(),
                    _ => return self.fault(FaultKind::Type, pc),
                };
                let x = f64::from(x);
                let r = if instr == Instr::Sin {
                    x.sin()
                } else {
                    x.cos()
                };
                self.push(Slot::float(r as f32), pc)?;
            }
        }
        Ok(Flow::Next(next))
    }

    /// Instructions 6 to 9: int with int is int, float with float or with int is float, any
    /// other mix is a fatal error.
    fn arithmetic(&mut self, instr: Instr, pc: usize) -> core::result::Result<(), Fault> {
        let b = self.pop(pc)?;
        let a = self.pop(pc)?;
        let result = match (a.ty, b.ty) {
            (INT, INT) => {
                let (x, y) = (a.i32(), b.i32());
                Slot::int(match instr {
                    Instr::Add => x.wrapping_add(y),
                    Instr::Sub => x.wrapping_sub(y),
                    Instr::Mul => x.wrapping_mul(y),
                    _ if y == 0 => return self.fault(FaultKind::DivideByZero, pc),
                    _ => x.wrapping_div(y),
                })
            }
            (INT | FLOAT, INT | FLOAT) => {
                let x = if a.ty == INT { a.i32() as f32 } else { a.f32() };
                let y = if b.ty == INT { b.i32() as f32 } else { b.f32() };
                Slot::float(match instr {
                    Instr::Add => x + y,
                    Instr::Sub => x - y,
                    Instr::Mul => x * y,
                    _ => x / y,
                })
            }
            _ => return self.fault(FaultKind::Type, pc),
        };
        self.push(result, pc)
    }

    /// Instruction 0xe: pushes 1 or 0.
    fn compare(&mut self, cmp: Cmp, pc: usize) -> core::result::Result<(), Fault> {
        let b = self.pop(pc)?;
        let a = self.pop(pc)?;
        let holds = match (a.ty, b.ty) {
            (INT, INT) => {
                let (x, y) = (a.i32(), b.i32());
                match cmp {
                    Cmp::Eq => x == y,
                    Cmp::Ne => x != y,
                    Cmp::Lt => x < y,
                    Cmp::Le => x <= y,
                    Cmp::Gt => x > y,
                    Cmp::Ge => x >= y,
                }
            }
            (INT | FLOAT, INT | FLOAT) => {
                let x = if a.ty == INT { a.i32() as f32 } else { a.f32() };
                let y = if b.ty == INT { b.i32() as f32 } else { b.f32() };
                match cmp {
                    Cmp::Eq => x == y,
                    Cmp::Ne => x != y,
                    Cmp::Lt => x < y,
                    Cmp::Le => x <= y,
                    Cmp::Gt => x > y,
                    Cmp::Ge => x >= y,
                }
            }
            _ => return self.fault(FaultKind::Type, pc),
        };
        self.push(Slot::int(i32::from(holds)), pc)
    }
}

/// Where the dispatch loop goes after an instruction.
enum Flow {
    Next(usize),
    Yield(usize),
    End,
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;

    /// Records native calls (id, arguments); native 900 writes 77 through its address.
    #[derive(Default)]
    struct Rec {
        calls: Vec<(i32, Vec<Slot>)>,
    }

    impl Natives for Rec {
        fn call(&mut self, id: i32, args: &Args, out: &mut Outs) {
            self.calls
                .push((id, (0..args.len()).filter_map(|i| args.slot(i)).collect()));
            if id == 900 {
                out.int(args, 0, 77);
            }
        }
    }

    fn program(globals: u32, funcs: &[Func]) -> Program {
        Program::parse(build(globals, funcs, &[(5, 0)])).unwrap()
    }

    /// Runs entry 5 to its end (or fault), returning the native calls.
    fn run(globals: u32, funcs: &[Func]) -> (Result<Status, Fault>, Rec, Vm) {
        let p = program(globals, funcs);
        let (mut vm, mut rec) = (Vm::new(&p), Rec::default());
        vm.start(&p, 5).unwrap();
        let r = loop {
            match vm.run(&p, &mut rec, 10_000) {
                Ok(Status::Yielded) => {}
                done => break done,
            }
        };
        (r, rec, vm)
    }

    fn args(rec: &Rec, call: usize) -> Vec<Slot> {
        rec.calls[call].1.clone()
    }

    fn cat(parts: Vec<Vec<T>>) -> Vec<T> {
        parts.concat()
    }

    #[test]
    fn arithmetic_keeps_ints_and_widens_mixed_operands() {
        let code = cat(vec![
            native(500, &[int(6), int(7), op(MUL)]),
            native(501, &[int(7), int(2), op(DIV)]),
            native(502, &[int(-7), int(3), op(MOD)]),
            native(503, &[int(1), float(0.5), op(ADD)]),
            native(504, &[float(3.0), int(2), op(SUB)]),
            native(505, &[int(5), op(NEG)]),
            native(506, &[int(3), op(TOFLOAT)]),
            native(507, &[float(2.75), op(TOINT)]),
            native(508, &[float(1.0), float(4.0), op(DIV)]),
            vec![op(END)],
        ]);
        let (r, rec, _) = run(0, &[func(0, 0, code)]);
        assert_eq!(r, Ok(Status::Finished));
        let value = |i: usize| args(&rec, i)[0];
        assert_eq!(value(0), Slot::int(42));
        assert_eq!(value(1), Slot::int(3));
        assert_eq!(
            value(2),
            Slot::int(-1),
            "remainder takes the dividend's sign"
        );
        assert_eq!(value(3), Slot::float(1.5));
        assert_eq!(value(4), Slot::float(1.0));
        assert_eq!(value(5), Slot::int(-5));
        assert_eq!(value(6), Slot::float(3.0));
        assert_eq!(value(7), Slot::int(2));
        assert_eq!(value(8), Slot::float(0.25));
    }

    #[test]
    fn comparisons_and_logic_push_zero_or_one() {
        let cmp = |a: u32| T::Op(CMP, a, 0);
        let code = cat(vec![
            native(500, &[int(3), int(3), cmp(0x28)]),
            native(501, &[int(3), int(4), cmp(0x2a)]),
            native(502, &[int(3), float(2.5), cmp(0x2c)]),
            native(503, &[float(1.0), int(1), cmp(0x2b)]),
            native(504, &[int(6), int(3), op(AND)]),
            native(505, &[int(6), int(3), op(OR)]),
            native(506, &[int(0), op(NOT)]),
            native(507, &[int(9), op(NOT)]),
            vec![op(END)],
        ]);
        let (r, rec, _) = run(0, &[func(0, 0, code)]);
        assert_eq!(r, Ok(Status::Finished));
        let got: Vec<_> = (0..8).map(|i| args(&rec, i)[0].val).collect();
        assert_eq!(got, [1, 1, 1, 1, 2, 7, 1, 0]);
    }

    #[test]
    fn short_circuit_jumps_keep_their_value_only_when_asked() {
        // 0 && x: the jump leaves 0 on the stack; 5 falls through with the 5 popped.
        let skip = |value: i32, keep: u32| {
            vec![
                int(500),
                int(value),
                T::Jump(JF, 4, keep),
                int(99),
                T::Op(NATIVE, 2, 0),
                op(END),
            ]
        };
        let (_, rec, _) = run(0, &[func(0, 0, skip(0, 1))]);
        assert_eq!(args(&rec, 0), [Slot::int(0)]);
        let (_, rec, _) = run(0, &[func(0, 0, skip(5, 1))]);
        assert_eq!(args(&rec, 0), [Slot::int(99)]);
        // Without `keep` the jump leaves nothing: the native then sees only its id... and
        // underflows the arguments it was told, which is a fault, not a panic.
        let (r, _, _) = run(0, &[func(0, 0, skip(0, 0))]);
        assert_eq!(r.unwrap_err().kind, FaultKind::StackUnderflow);
        // A float zero is true: only the pair (int, 0) is false.
        let code = vec![
            int(500),
            float(0.0),
            T::Jump(JT, 4, 1),
            int(99),
            T::Op(NATIVE, 2, 0),
            op(END),
        ];
        let (_, rec, _) = run(0, &[func(0, 0, code)]);
        assert_eq!(args(&rec, 0), [Slot::int(1)]);
    }

    #[test]
    fn calls_pass_parameters_and_return_one_value() {
        // f(a, b) { local2 = a + b; return local2 * 2 } ; local2 starts at zero.
        let f = func(
            3,
            2,
            cat(vec![
                vec![addr(2), var(0), var(1), op(ADD), op(ASSIGN), op(POP)],
                vec![var(2), int(2), op(MUL), op(RETURN)],
            ]),
        );
        let main = func(
            0,
            0,
            cat(vec![
                vec![int(500), int(5), int(7), T::Call(1), T::Op(NATIVE, 2, 0)],
                // The frame is gone: another call sees zeroed locals.
                vec![int(501), int(1), int(1), T::Call(1), T::Op(NATIVE, 2, 0)],
                vec![op(END)],
            ]),
        );
        let (r, rec, vm) = run(0, &[main, f]);
        assert_eq!(r, Ok(Status::Finished));
        assert_eq!(args(&rec, 0), [Slot::int(24)]);
        assert_eq!(args(&rec, 1), [Slot::int(4)]);
        assert!(vm.is_finished());
    }

    #[test]
    fn returning_from_the_outermost_function_ends_the_script() {
        let (r, rec, vm) = run(0, &[func(0, 0, vec![int(5), op(RETURN), int(500)])]);
        assert_eq!(r, Ok(Status::Finished));
        assert!(rec.calls.is_empty());
        assert_eq!(vm.result(), 5);
    }

    #[test]
    fn yield_suspends_until_the_next_run() {
        let code = cat(vec![
            native(1, &[]),
            vec![op(YIELD)],
            native(2, &[]),
            vec![op(END)],
        ]);
        let p = program(0, &[func(0, 0, code)]);
        let (mut vm, mut rec) = (Vm::new(&p), Rec::default());
        vm.start(&p, 5).unwrap();
        assert_eq!(vm.run(&p, &mut rec, 100), Ok(Status::Yielded));
        assert_eq!(rec.calls.len(), 1);
        assert_eq!(vm.run(&p, &mut rec, 100), Ok(Status::Finished));
        assert_eq!(rec.calls.len(), 2);
        // A finished script stays finished.
        assert_eq!(vm.run(&p, &mut rec, 100), Ok(Status::Finished));
        assert_eq!(rec.calls.len(), 2);
    }

    #[test]
    fn natives_return_through_address_arguments() {
        let code = cat(vec![
            native(900, &[addr(0)]),
            native(500, &[var(0)]),
            // A value that is not an address is left alone.
            native(900, &[int(3)]),
            vec![op(END)],
        ]);
        let (r, rec, _) = run(0, &[func(1, 0, code)]);
        assert_eq!(r, Ok(Status::Finished));
        assert_eq!(args(&rec, 1), [Slot::int(77)]);
    }

    #[test]
    fn globals_and_indexed_variables() {
        // Global 1 = 8; local array: element 1 of the array at local0 is frame slot 2.
        let code = cat(vec![
            vec![T::Op(ADDRESS, 1, 0x40), int(8), op(ASSIGN), op(POP)],
            vec![addr(0), addr(1), op(ASSIGN), op(POP)],
            vec![int(0), T::Op(ADDRESS, 0, 4), int(11), op(ASSIGN), op(POP)],
            vec![int(1), T::Op(ADDRESS, 1, 2), int(22), op(ASSIGN), op(POP)],
            native(500, &[T::Op(LOAD, 1, 0x40), var(1), var(2)]),
            vec![op(END)],
        ]);
        let (r, rec, vm) = run(2, &[func(4, 0, code)]);
        assert_eq!(r, Ok(Status::Finished));
        assert_eq!(args(&rec, 0), [Slot::int(8), Slot::int(11), Slot::int(22)]);
        assert_eq!(vm.global(1), Some(Slot::int(8)));
    }

    #[test]
    fn float_scopes_retag_the_slot() {
        let code = cat(vec![
            vec![addr(0), int(0x3f80_0000), op(ASSIGN), op(POP)],
            native(500, &[T::Op(LOAD, 0, 8)]),
            vec![op(END)],
        ]);
        let (_, rec, _) = run(0, &[func(1, 0, code)]);
        assert_eq!(args(&rec, 0), [Slot::float(1.0)]);
    }

    #[test]
    fn invalid_programs_fault_instead_of_panicking() {
        let fault = |code: Vec<T>| run(0, &[func(2, 0, code)]).0.unwrap_err().kind;
        // A string is no number.
        assert_eq!(
            fault(vec![T::Op(3, 3, 0), int(1), op(ADD), op(END)]),
            FaultKind::Type
        );
        assert_eq!(
            fault(vec![int(1), int(0), op(DIV), op(END)]),
            FaultKind::DivideByZero
        );
        assert_eq!(
            fault(vec![int(1), int(0), op(MOD), op(END)]),
            FaultKind::DivideByZero
        );
        // The frame's own slots are on the stack too: only an empty one underflows.
        let empty = run(0, &[func(0, 0, vec![op(POP), op(END)])]).0;
        assert_eq!(empty.unwrap_err().kind, FaultKind::StackUnderflow);
        assert_eq!(
            fault(vec![int(9), T::Op(LOAD, 0, 2), op(END)]),
            FaultKind::BadAddress
        );
        assert_eq!(fault(vec![float(1.0), op(NOT), op(END)]), FaultKind::Type);
        // A loop that never yields runs out of budget.
        assert_eq!(fault(vec![T::Jump(JUMP, 0, 0)]), FaultKind::Budget);
        // Unbounded recursion.
        let recursive = func(0, 0, vec![T::Call(0), op(END)]);
        assert_eq!(
            run(0, &[recursive]).0.unwrap_err().kind,
            FaultKind::CallOverflow
        );
        // Unbounded pushes.
        assert_eq!(
            fault(vec![int(1), T::Jump(JUMP, 0, 0)]),
            FaultKind::StackOverflow
        );
    }

    #[test]
    fn unknown_entries_are_reported() {
        let p = program(0, &[func(0, 0, vec![op(END)])]);
        let mut vm = Vm::new(&p);
        assert_eq!(vm.start(&p, 6).unwrap_err().kind, FaultKind::NoEntry);
        assert_eq!(vm.start(&p, -1), Ok(()), "negative: the default record");
    }

    #[test]
    fn skipping_runs_to_the_next_step_marker() {
        // `+0x40` set: natives, yields and jumps do nothing until instruction 0x1c, which
        // clears it and suspends.
        let code = cat(vec![
            native(1, &[]),
            vec![op(YIELD)],
            native(2, &[]),
            vec![op(STEP)],
            native(3, &[]),
            vec![op(END)],
        ]);
        let p = program(0, &[func(0, 0, code)]);
        let (mut vm, mut rec) = (Vm::new(&p), Rec::default());
        vm.start(&p, 5).unwrap();
        vm.set_skip(true);
        assert_eq!(vm.run(&p, &mut rec, 100), Ok(Status::Yielded));
        assert!(rec.calls.is_empty());
        assert_eq!(vm.steps(), 1);
        assert_eq!(vm.run(&p, &mut rec, 100), Ok(Status::Finished));
        assert_eq!(rec.calls.len(), 1);
        assert_eq!(rec.calls[0].0, 3);
    }
}
