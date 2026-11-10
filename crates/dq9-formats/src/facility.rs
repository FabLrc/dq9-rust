//! Facilities of a town: a shop's stock and an item's two prices, an inn's and a church's
//! system strings, the treasure containers of a map and the random tables they draw from.
//!
//! Layouts, the counts checked over the whole cartridge and the open questions are in
//! `docs/re/facility.md`; the pages of the DQIX wiki the numbers come from are *Items*
//! (`shopdata1.bin`, the two prices), *Inns-and-Churches* (`str_in<k>`/`str_ch<k>`, the inn's
//! price line 10000, the church's services) and *Treasure* (`treasure.nsarc`).
//!
//! No I/O: the caller decompresses the members and hands the bytes over. The rules that use
//! these readers are in `dq9_core::facility`.

pub mod inn_church;
pub mod shop;
pub mod treasure;
