use super::*;
use crate::script::testing::*;

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
