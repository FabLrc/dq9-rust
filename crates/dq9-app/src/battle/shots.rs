//! The fixed shots of the big monsters: the table `func_ov026_021d8aac` reads at
//! `data_ov026_021de87c`, and its lookup (`docs/app/battle-scene.md`, § Camera).
//!
//! Each record is seven fx12 `int`s — the monster's number, the eye `(x, y, z)`, the target
//! `(x, y, z)` — and the table ends with `-1`. `func_ov026_021d8aac(battle, counter, type)` walks
//! it in order and takes the **first** record whose kind stands in the fight
//! (`func_ov000_0215fc8c`), storing the eye at `battle +0x77f4` and the target at `+0x7800`;
//! `func_ov000_0216d600`'s `param_8` then applies them in place of the calculated shot.
//!
//! Read on the EU ROM (`build/evidence/v4-battle-intro/camera-tables.txt`): the table sits at
//! `0x021de87c` in overlay 026, its records as below. Two details the port does not carry:
//! `0x115`'s three variants are selected by the second argument modulo three (a battle counter,
//! `battle +0x8e20`), and the `0` record applies when `func_ov000_0215fc60` answers 2 (the
//! battle's own type); here the first `0x115` record is used and `0` is never selected.

/// The table's records, in its own order: `(monster number, eye, target)`, fx12.
#[rustfmt::skip]
pub(super) const KIND_SHOTS: [(u16, [i32; 6]); 47] = [
    (0x101, [0, 4341, 9297, 0, 4423, 0]),
    (0x102, [-1105, 5570, 9666, -1105, 5611, 0]),
    (0x120, [-1105, 5570, 9666, -1105, 5611, 0]),
    (0x103, [0, 4874, 6676, 0, 4874, 0]),
    (0x104, [0, 7372, 9912, 0, 7045, 0]),
    (0x105, [532, 5201, 11878, 532, 5816, 0]),
    (0x125, [532, 5201, 11878, 532, 5816, 0]),
    (0x116, [532, 5201, 11878, 532, 5816, 0]),
    (0x106, [0, 11100, 37928, 0, 13393, 0]),
    (0x0d5, [0, 11100, 37928, 0, 13393, 0]),
    (0x0ff, [0, 11100, 37928, 0, 13393, 0]),
    (0x107, [0, 5120, 10485, 0, 5611, 0]),
    (0x109, [0, 3563, 11468, 0, 5201, 0]),
    (0x108, [0, 5406, 10608, 0, 5120, 0]),
    (0x10a, [0, 3850, 9256, 0, 4669, 0]),
    (0x119, [0, 3850, 9256, 0, 4669, 0]),
    (0x10b, [0, 4997, 8273, 0, 5079, 0]),
    (0x11f, [0, 4997, 8273, 0, 5079, 0]),
    (0x10c, [0, 5283, 10649, 0, 5857, 0]),
    (0x10d, [0, 5038, 22241, 0, 6389, 0]),
    (0x10e, [-942, 6144, 12820, -942, 5816, 0]),
    (0x122, [-942, 6144, 12820, -942, 5816, 0]),
    (0x110, [0, 5365, 7454, 0, 5365, 0]),
    (0x124, [0, 5365, 7454, 0, 5365, 0]),
    (0x111, [0, 4218, 7946, 0, 4587, 0]),
    (0x118, [0, 4218, 7946, 0, 4587, 0]),
    (0x112, [-1310, 8069, 12574, -1310, 7905, 0]),
    (0x114, [0, 4136, 5488, 0, 4464, 0]),
    (0x000, [0, 4136, 5488, 0, 4464, 0]),
    (0x113, [-2252, 4136, 23838, -2252, 9994, 0]),
    (0x126, [-2252, 4136, 23838, -2252, 9994, 0]),
    (0x127, [0, 6758, 16302, 0, 7618, 0]),
    (0x128, [0, 6144, 14622, 0, 7905, 0]),
    (0x129, [0, 6062, 14581, 0, 6144, 0]),
    (0x12a, [0, 4423, 16097, 0, 6471, 0]),
    (0x12b, [0, 2539, 15032, 0, 5693, 0]),
    (0x12c, [0, 5816, 16588, 0, 7495, 0]),
    (0x12d, [0, 1802, 15278, 0, 5406, 0]),
    (0x12e, [0, 4997, 9297, 0, 4956, 0]),
    (0x12f, [0, 3563, 13762, 0, 5447, 0]),
    (0x131, [0, 8888, 19046, 0, 9461, 0]),
    (0x132, [0, 6062, 16875, 0, 7249, 0]),
    (0x133, [0, 6512, 22405, 0, 10280, 0]),
    (0x130, [0, 6184, 9584, 0, 6266, 0]),
    (0x115, [0, 3604, 11059, 0, 6062, 0]),
    (0x115, [5980, 9175, -1679, -122, 11755, -9830]),
    (0x115, [-1884, 9912, 3686, 368, 10936, -10567]),
];

/// The shot of the first record in table order whose kind is in `kinds`: `(eye, target)`, fx12.
pub(super) fn kind_shot(kinds: &[u16]) -> Option<([i32; 3], [i32; 3])> {
    KIND_SHOTS
        .iter()
        .find(|(kind, _)| *kind != 0 && kinds.contains(kind))
        .map(|&(_, values)| {
            (
                [values[0], values[1], values[2]],
                [values[3], values[4], values[5]],
            )
        })
}
