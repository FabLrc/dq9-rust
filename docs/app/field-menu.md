# Field menu and facility screens

`I` on the field opens the game's menu — **Items**, **Equipment**, **Attributes**, **Spells**,
**Close** — unless a conversation, an event or a battle holds the player. A talk line carrying
`<INN=n>`, `<CHURCH=n>` or `<SHOP=n>` opens that facility when its message window closes; the
conversation ends (trigger event 1, talk counters) only after the screen closes, in the order
of the game's talk task. Movement and roaming freeze while any of these screens is open, `R`
still switches the layout, and a map change closes them. `Enter`/`Space`/`Z` confirm,
`Backspace`/`X` cancel, arrows or `WASD` move, `H` puts a bag item into a held slot, `T` takes
a held item back, `Left`/`Right` switch the shop between buying and selling.

The rules live in `dq9_core::facility` and `dq9_core::party`;
`crates/dq9-app/src/app/services.rs` adds the ROM tables, the cursors and the transfer into
`Party`, and [`re/facility.md`](../re/facility.md) records what was read. A transfer moves **one**
copy and preflights the real destination, because `Bag::add` reports success while clamping a
full stack of 99: a swap that cannot store the old copy restores the chosen one, so no copy is
ever lost. Field item use and the leader's healing spells run through
`dq9_core::battle::resolve_action` with the battle's range table, healing cap and MP cost; a
group heal (Multiheal) resolves every living member in party order and spends MP once; a dead
member is never a target and a dead leader has no enabled command.

Equipment changes rebuild only the drawn hero (`AnimatedScene::replace_hero`), in the same
scene model and instance slots, at its live pose and gait; the field's cast, talk counters,
doors, events and generators are untouched and no map trigger replays. The inn moves the day
clock (time, lighting and the trigger's `time_of_day` follow) and heals the living only; the
church's Resurrection, Purification and Benediction charge the price the core computes and name
the member, Divination is free and returns to its row, and Confession runs the church's own state 3
(`ov_003::02159d0c`): the greeting 1010 asks to record the adventure log, "Yes" shows 100 while the
live state is written into the save's current normal slot (sealed, mirrored, through a temporary
file and a rename — `--save-out` when it is given, else the `--save` container itself), then 1011
and 1013; "Yes, continue" farewells with 1090 and returns to the field, while the rest (1015, which
opens on No) ends with 1014 and 110 and **ends the session** (`App::end_session`: the window
closes — the original's 110 is its last line, with no title screen of its own). Without `--save`
there is no container: the entry says so in its own words and nothing is written. The lines, their
initial rows and their order are in [`re/facility.md`](../re/facility.md), § "The Confession".

Known limits: the window geometry, colours and title/footer are **[hypothesis]** (the field
message window's rectangle grown to the DS screen, on the screen the field does not use);
vocation restrictions on equipment are not modelled; a field item used at full HP still spends
the copy; the church's coffin line, Benediction's unequipping and the inn's guest lines are not
modelled; buying or selling is one copy per command and a partial purchase is not offered; a
sale at the gold cap is the documented **[hypothesis]**; the Confession's 1018 (another's
world) variant, its trigger 0xC and its jingle 60 are not modelled, and after 110 the session
ends rather than staying on the line.

Evidence (`build/evidence/j4-field-services/`, native windows on the European ROM, sha256
prefixes): menu `field-menu.png` (`6669e1ad98e4`), held-item swap and use `held-swap.png`,
`held-use.log` (`298cdf4a7336`), equipment and attributes `equipment.png` (`f0d25fe2a8aa`),
`attributes.png` (`0379109e4882`), paid shop `shop-paid.png` (`cecbfc5c446c`, gold 100 → 92, one
herb), inn `inn-rested.png` (`94a10396507b`, the living healed, the dead untouched),
resurrection `revival-done.png` (`39dc17c14e4d`, 0 → 30 HP for 60 gold), Divination
`divination-result.png` (`044932510558`), Multiheal `multiheal.log` (`a691caa6479f`), Fullheal
`fullheal-after.png` (`c24ecb26688e`, HP 20 → 80 for 24 MP) and a dead actor's visible refusal
`dead-actor-refusal.png` (`f9f2c74e00c2`). The same transitions are covered without a window in
`app/services/tests.rs`, including the bounded hero replacement (alternating outfits keep the
model, instance and texture counts and the texel storage stable, and a failed resource leaves
the old rig drawn). The Confession has no window capture: its flow log
`build/evidence/v3-save-menu/confession.log` (`dc81ea6db300`) prints the line sequence the run
walked and names the file written with `--save` untouched, its byte-level shape is
`confession-diff.txt` (`ddfd2b93169a`), and the drawing itself is the compiled
`Screen::Confession` window, not captured.

Controls here absorb the field's: `T` is "take a held item back" while a screen is open and the
next time of day otherwise.
