use dq9_formats::cast::TalkZone;
use dq9_formats::font::{Font, FontId};
use dq9_formats::message::{LeadingTags, compile};
use glam::Vec3;

use super::*;

/// Font of 4-pixel glyphs `a b c d - A` (indices 0 to 5, `A` is a vowel and lowercase is
/// not tracked): advance 5, spaces advance 4.
fn font_files() -> (Vec<u8>, Vec<u8>) {
    let glyphs: [&[u8]; 6] = [b"a", b"b", b"c", b"d", b"-", b"A"];
    let table = 0x18;
    let strings = table + glyphs.len() * 8;
    let mut info = b"1.1\0".to_vec();
    for v in [glyphs.len(), 0, table, table, strings] {
        info.extend((v as u32).to_le_bytes());
    }
    let mut pool: Vec<u8> = Vec::new();
    for text in glyphs {
        info.extend(((strings + pool.len()) as u32).to_le_bytes());
        info.extend([4, text.len() as u8, 0, 0]);
        pool.extend(text);
    }
    info.extend(pool);
    let mut data = b"1.0\0".to_vec();
    for v in [8u16, 1] {
        data.extend(v.to_le_bytes());
    }
    data.extend(1u32.to_le_bytes());
    data.extend(16u32.to_le_bytes());
    data.push(0);
    (info, data)
}

fn script(text: &[u8], width: i32) -> Script {
    let (info, data) = font_files();
    let font = Font::parse(&info, &data, FontId::Message).unwrap();
    layout(&compile(text), LeadingTags::default(), &font, width)
}

/// The glyph indices of each line of a script, from its ops.
fn lines(script: &Script) -> Vec<Vec<(u8, i32)>> {
    let mut out = vec![Vec::new()];
    for op in &script.ops {
        match op {
            Op::Glyph { glyph, x } => out.last_mut().unwrap().push((*glyph, *x)),
            Op::Line => out.push(Vec::new()),
            _ => {}
        }
    }
    out
}

fn press() -> Input {
    Input {
        confirm: true,
        advance: true,
        ..Input::default()
    }
}

/// The action key on a wait for the end, then the steps the window takes to close.
fn press_and_close(dialogue: &mut Dialogue) {
    dialogue.step(press());
    play(dialogue, CLOSE_STEPS as usize);
}

fn play(dialogue: &mut Dialogue, steps: usize) {
    for _ in 0..steps {
        dialogue.step(Input::default());
    }
}

#[test]
fn long_lines_break_at_the_last_space_and_drop_it() {
    // Glyphs advance 5 px, a space 4: "aa bb cc" at 20 px wraps before each word.
    let s = script(b"aa bb cc", 20);
    assert_eq!(
        lines(&s),
        [
            vec![(0, 0), (0, 5)],
            vec![(1, 0), (1, 5)],
            vec![(2, 0), (2, 5)]
        ]
    );
    // A word without break point overflows instead of splitting.
    let s = script(b"aaaaaa", 12);
    assert_eq!(lines(&s).len(), 1);
    // A hyphen is a break point too.
    let s = script(b"aa-bb", 16);
    assert_eq!(
        lines(&s),
        [vec![(0, 0), (0, 5), (4, 10)], vec![(1, 0), (1, 5)]]
    );
}

#[test]
fn explicit_breaks_and_names_are_kept() {
    let s = script(b"//aA// ab\\ncd", 100);
    let Op::Name(plate) = &s.ops[0] else {
        panic!("the page opens with its speaker");
    };
    assert_eq!(plate.text, b"aA");
    assert_eq!(plate.glyphs, [(0, 0), (5, 5)]);
    assert_eq!(plate.advance, 10);
    assert_eq!(lines(&s), [vec![(0, 0), (1, 5)], vec![(2, 0), (3, 5)]]);
}

#[test]
fn typing_is_one_unit_per_step_and_pages_wait() {
    let mut d = Dialogue::new(script(b"ab<PAGE>cd", 100));
    play(&mut d, 1);
    assert_eq!(d.view().lines[0].len(), 1);
    play(&mut d, 1);
    assert_eq!(d.view().lines[0].len(), 2);
    assert!(!d.view().arrow);
    play(&mut d, 1);
    assert!(d.waiting(), "the page waits for the action key");
    assert!(
        !d.view().arrow,
        "the arrow is hidden on the wait's first frames"
    );
    play(&mut d, 5);
    assert_eq!(d.view().lines[0].len(), 2, "nothing moves without input");
    d.step(press());
    assert!(d.view().lines[0].is_empty(), "the box is cleared");
    play(&mut d, 2);
    assert_eq!(d.view().lines[0].len(), 2);
    play(&mut d, 1);
    assert!(d.waiting(), "the text ended: it waits for the key");
    assert!(!d.view().arrow, "state 0 (`<END>`) draws no arrow");
}

#[test]
fn the_action_key_does_not_skip_the_typing() {
    // `func_020440cc` has no case for the typing state: pressing A while the text types is
    // ignored, the page finishes at its own rate (measured on the original, 400 frames).
    let text = b"abcdabcd<PAGE>a";
    let mut typed = Dialogue::new(script(text, 100));
    let mut pressed = Dialogue::new(script(text, 100));
    for step in 0..8 {
        typed.step(Input::default());
        pressed.step(press());
        assert_eq!(
            typed.view().lines,
            pressed.view().lines,
            "step {step}: the key changes nothing while typing"
        );
    }
    assert!(!pressed.view().arrow);
}

#[test]
fn spaces_and_line_breaks_cost_a_unit_like_the_original_stream() {
    // The original's code stream (read from RAM) holds a `0xFF19` per space and a `0xFF18`
    // per break, each spent by the update like a glyph: "a b\nc" is five units.
    let mut d = Dialogue::new(script(b"a b\\nc<PAGE>", 100));
    let mut seen = Vec::new();
    for _ in 0..5 {
        d.step(Input::default());
        seen.push(d.view().lines.iter().map(Vec::len).collect::<Vec<_>>());
    }
    assert_eq!(
        seen,
        [vec![1], vec![1], vec![2], vec![2, 0], vec![2, 1]],
        "glyph, space, glyph, break, glyph"
    );
    d.step(Input::default());
    assert!(d.waiting(), "the page waits for the key");
}

#[test]
fn a_wrapped_space_is_the_break_not_an_extra_unit() {
    // Width 12: "ab cd" wraps at the space; the stream holds one break where the space was.
    let mut d = Dialogue::new(script(b"ab cd<PAGE>", 12));
    play(&mut d, 2);
    assert_eq!(d.view().lines, [[(0, 0), (1, 5)]]);
    play(&mut d, 1);
    assert_eq!(
        d.view().lines.iter().map(Vec::len).collect::<Vec<_>>(),
        [2, 0]
    );
    play(&mut d, 2);
    assert_eq!(d.view().lines[1], [(2, 0), (3, 5)]);
}

#[test]
fn text_speed_scales_the_units_per_step() {
    let units = |speed: TextSpeed, steps: u32| {
        let mut carry = 0;
        (0..steps).map(|_| speed.units(&mut carry)).sum::<usize>()
    };
    // Normal is the original's rate: 2 units per 33 ms update, 60 per second.
    assert_eq!(units(TextSpeed::Normal, 60), 60);
    assert_eq!(units(TextSpeed::Slow, 60), 30);
    assert_eq!(units(TextSpeed::Fast, 60), 120);
    let mut carry = 0;
    let slow: Vec<_> = (0..4).map(|_| TextSpeed::Slow.units(&mut carry)).collect();
    assert_eq!(slow, [0, 1, 0, 1], "a unit every other step");
    assert_eq!(TextSpeed::default(), TextSpeed::Normal);
    assert_eq!(TextSpeed::parse("fast"), Ok(TextSpeed::Fast));
    assert!(TextSpeed::parse("quick").is_err());
}

#[test]
fn the_dialogue_types_at_its_speed() {
    let typed = |speed: TextSpeed, steps: usize| {
        let mut d = Dialogue::new(script(b"abcdabcdabcdabcd<PAGE>", 100));
        d.set_speed(speed);
        play(&mut d, steps);
        d.view().lines[0].len()
    };
    assert_eq!(typed(TextSpeed::Slow, 8), 4);
    assert_eq!(typed(TextSpeed::Normal, 8), 8);
    assert_eq!(typed(TextSpeed::Fast, 8), 16);
}

#[test]
fn time_pauses_count_window_updates_of_two_steps() {
    // `<TIME=n>`: the counter drops once per `MsgWin_Update` call, 33 ms apart.
    let mut d = Dialogue::new(script(b"a<TIME=3>b<PAGE>", 100));
    play(&mut d, 1);
    let steps_until_b = (1..40)
        .find(|_| {
            d.step(Input::default());
            d.view().lines[0].len() == 2
        })
        .unwrap();
    // "a" at step 1, the pause op at step 2, six steps of counting, "b" at step 9.
    assert_eq!(steps_until_b, 3 * STEPS_PER_UPDATE + 2);
}

#[test]
fn timed_waits_end_by_themselves() {
    // `<PAGE_T=n>` counts `n` ticks (one per step) while the page waits, then acts as A.
    let mut d = Dialogue::new(script(b"a<PAGE_T=5>b<PAGE>", 100));
    play(&mut d, 2);
    assert!(d.waiting());
    play(&mut d, 4);
    assert!(d.waiting(), "still waiting after 4 steps");
    play(&mut d, 1);
    assert!(d.view().lines[0].is_empty(), "the 5th step turned the page");
    // `<PAD_T=n>` counts `n` updates (2 steps each).
    let mut d = Dialogue::new(script(b"a<PAD_T=2>b<PAGE>", 100));
    play(&mut d, 2);
    assert!(d.waiting());
    play(&mut d, 3);
    assert!(d.waiting());
    play(&mut d, 1);
    assert!(!d.waiting(), "4 steps after the wait began");
    // Without the limit a page never turns by itself.
    let mut d = Dialogue::new(script(b"a<PAGE>b<PAGE>", 100));
    play(&mut d, 500);
    assert!(d.waiting());
}

#[test]
fn end_waits_for_the_key_then_closes() {
    let mut d = Dialogue::new(script(b"a", 100));
    play(&mut d, 3);
    assert!(d.waiting());
    assert!(!d.view().arrow, "state 0 (`<END>`) draws no arrow");
    assert_eq!(d.outcome(), None);
    // The window closes `CLOSE_STEPS` after the key, no sooner (melonDS: key 61, closed 67).
    d.step(press());
    play(&mut d, CLOSE_STEPS as usize - 1);
    assert_eq!(d.outcome(), None);
    assert!(!d.view().arrow, "the arrow is gone once the key was taken");
    play(&mut d, 1);
    assert_eq!(
        d.outcome(),
        Some(Outcome {
            end: EndKind::Wait,
            answer: None
        })
    );
    let mut d = Dialogue::new(script(b"a<CLOSE>", 100));
    play(&mut d, 3);
    assert_eq!(d.outcome().map(|o| o.end), Some(EndKind::Close));
}

/// `dq9_formats::message` compiles the two ends apart: `<END>` waits and keeps the heading,
/// `<END_R_TURN>` waits and gives it back, and `<R_TURN>` is a code of its own in the text.
#[test]
fn the_turn_tags_compile_to_their_own_ops() {
    // `Msg_Compile` appends an `<END>` to every message; the tag's own end comes first and
    // is the one the window reaches.
    let s = script(b"a<END_R_TURN>", 100);
    let first_end = s.ops.iter().find(|op| matches!(op, Op::End(_)));
    assert_eq!(first_end, Some(&Op::End(EndKind::Turn)));
    let s = script(b"a<R_TURN>b<END>", 100);
    assert!(s.ops.contains(&Op::ReleaseTurn));
    let first_end = s.ops.iter().find(|op| matches!(op, Op::End(_)));
    assert_eq!(first_end, Some(&Op::End(EndKind::Wait)));
}

/// `<R_TURN>` (`0xFF2C`) gives the partner the conversation's saved heading back **mid-line**:
/// the report comes on the step the code is met, the page still shows, the rest of the text
/// types on, and the `<END>` that follows keeps that heading (`0x020659a0` sets
/// `+0x1844 = +0x183c` and `+0x19b6 = 1`, then goes on).
#[test]
fn r_turn_releases_the_heading_while_the_page_still_shows() {
    let mut d = Dialogue::new(script(b"ab<R_TURN>cd<END>", 100));
    for _ in 0..2 {
        d.step(Input::default());
        assert!(!d.take_turn_release(), "nothing before the code");
    }
    assert_eq!(d.view().lines[0].len(), 2);
    d.step(Input::default());
    assert!(d.take_turn_release(), "the code reports on its own step");
    assert!(!d.take_turn_release(), "the report is taken once");
    assert_eq!(d.outcome(), None, "the window stays open");
    play(&mut d, 2);
    assert_eq!(d.view().lines[0].len(), 4, "the rest of the page typed on");
    play(&mut d, 1);
    assert!(d.waiting(), "the line reaches its `<END>`");
    d.step(press());
    assert!(!d.take_turn_release(), "a plain `<END>` gives nothing back");
    play(&mut d, CLOSE_STEPS as usize);
    assert_eq!(
        d.outcome(),
        Some(Outcome {
            end: EndKind::Wait,
            answer: None
        })
    );
    assert!(!d.take_turn_release());
}

/// `<END_R_TURN>` (`0xFF02`) gives the heading back at the close: nothing while the text
/// stays, the report on the key that leaves the end wait (`func_020440cc` case 0 compares the
/// code with `0xFF02`), then the window closes `CLOSE_STEPS` later.
#[test]
fn end_r_turn_releases_at_the_key_and_a_plain_end_keeps_the_heading() {
    let mut d = Dialogue::new(script(b"ab<END_R_TURN>", 100));
    play(&mut d, 20);
    assert!(d.waiting());
    assert!(
        !d.take_turn_release(),
        "nothing gives back while the text stays"
    );
    d.step(press());
    assert!(
        d.take_turn_release(),
        "the key that leaves the wait reports"
    );
    assert!(!d.take_turn_release());
    play(&mut d, CLOSE_STEPS as usize - 1);
    assert_eq!(d.outcome(), None);
    play(&mut d, 1);
    assert_eq!(
        d.outcome(),
        Some(Outcome {
            end: EndKind::Turn,
            answer: None
        })
    );

    let mut d = Dialogue::new(script(b"ab<END>", 100));
    play(&mut d, 20);
    d.step(press());
    assert!(!d.take_turn_release(), "a plain `<END>` keeps the heading");
    play(&mut d, CLOSE_STEPS as usize);
    assert_eq!(d.outcome().map(|o| o.end), Some(EndKind::Wait));
    assert!(!d.take_turn_release());
}

/// Waits that draw the arrow: page (1), pad (2) and `<ADD>` (3); states 0 (`<END>`) and 4
/// (`<AUTO>`, not modelled) draw none. The triangle is hidden for the wait's first 10 frames,
/// then 32 frames on and 32 off (measured: visible on 10-41, hidden 42-73, visible from 74,
/// frame counted from the state's start; `docs/re/dialogue.md` § "Message pacing").
#[test]
fn the_wait_arrow_blinks_32_on_32_off_in_states_1_2_and_3() {
    // The measured table for frames 0..=80: first visible frame 10, then 32 on / 32 off.
    let on = |frame: u32| frame >= 10 && (frame - 10) % 64 < 32;
    for text in [
        &b"a<ADD>"[..],
        &b"a<PAGE>b"[..],
        &b"a<PAD_WAIT>b"[..],
        &b"a<PAD_WAIT_NOCUR>b"[..],
    ] {
        let mut d = Dialogue::for_event(script(text, 100));
        let mut steps = 0;
        while !d.waiting() {
            d.step(Input::default());
            steps += 1;
            assert!(steps < 50, "{text:?}: the text never waits");
        }
        for frame in 0..=80 {
            assert_eq!(d.view().arrow, on(frame), "{text:?}: arrow, frame {frame}");
            d.step(Input::default());
        }
        assert!(d.waiting(), "{text:?}: still waiting after 81 frames");
    }
}

/// `<ADD>` (state 3) waits for the key however long the player takes, draws the blinking
/// arrow, keeps its text, and the key closes it at once (`W+0x19bd` clears), so the scene's
/// next message follows.
#[test]
fn add_waits_for_the_key_with_its_arrow_and_closes_on_it() {
    let mut d = Dialogue::for_event(script(b"a<ADD>", 100));
    while !d.waiting() {
        d.step(Input::default());
    }
    play(&mut d, 3_000);
    assert_eq!(d.outcome(), None, "nothing but the key ends it");
    assert_eq!(d.view().lines[0], [(0, 0)], "the text stays on screen");
    d.step(press());
    assert_eq!(
        d.outcome(),
        Some(Outcome {
            end: EndKind::Add,
            answer: None
        })
    );
}

/// A wait leaves on the `advance` edge only: the application sends it on the step a key goes
/// down (`TestKeysPressed` `0x02012454`: `cur & m && !(prev & m)`), so a key still held does
/// not touch the next wait.
#[test]
fn a_wait_leaves_on_the_advance_edge_and_a_held_key_does_not_touch_the_next_one() {
    for text in [
        &b"a<ADD>"[..],
        &b"a<PAGE>b"[..],
        &b"a<PAD_WAIT>b"[..],
        &b"a<PAD_WAIT_NOCUR>b"[..],
        &b"a"[..],
    ] {
        let mut d = Dialogue::new(script(text, 100));
        let mut steps = 0;
        while !d.waiting() {
            d.step(Input::default());
            steps += 1;
            assert!(steps < 50, "{text:?}: the text never waits");
        }
        d.step(Input {
            advance: true,
            ..Input::default()
        });
        assert!(!d.waiting(), "{text:?}: the advance edge leaves the wait");
    }
    // The `<PAGE>` turned on its edge; the held key (no edge) types on and the second page
    // still waits for a new press.
    let mut d = Dialogue::new(script(b"a<PAGE>b<PAGE>c", 100));
    while !d.waiting() {
        d.step(Input::default());
    }
    d.step(Input {
        advance: true,
        ..Input::default()
    });
    play(&mut d, 10);
    assert!(d.waiting(), "the second page still waits");
    assert_eq!(d.view().lines[0], [(1, 0)], "only `b` typed");
}

/// Prints the wait arrow's visibility over a wait's first 81 frames for the states that draw
/// it and for `<END>`: the table of `docs/re/dialogue.md` § "Message pacing"
/// (`cargo test -p dq9-core print_arrow_phase -- --ignored --nocapture`).
#[test]
#[ignore = "prints the measured table, asserts nothing"]
fn print_arrow_phase() {
    for text in [
        &b"a<ADD>"[..],
        &b"a<PAGE>b"[..],
        &b"a<PAD_WAIT>b"[..],
        &b"a"[..],
    ] {
        let mut d = Dialogue::for_event(script(text, 100));
        while !d.waiting() {
            d.step(Input::default());
        }
        let frames: String = (0..=80)
            .map(|_| {
                let shown = d.view().arrow;
                d.step(Input::default());
                if shown { '#' } else { '.' }
            })
            .collect();
        println!("{:<16} {frames}", String::from_utf8_lossy(text));
    }
}

#[test]
fn a_fourth_line_scrolls_the_box() {
    let mut d = Dialogue::new(script(b"a\\nb\\nc\\nd", 100));
    play(&mut d, 5);
    assert_eq!(d.view().lines.len(), 3);
    // The fourth line's break starts the scroll (one pixel per step), then `d` types.
    play(&mut d, usize::try_from(SCROLL_STEPS).unwrap() + 2);
    let view = d.view();
    assert_eq!(view.lines.len(), 3);
    assert_eq!(view.lines[0], [(1, 0)]);
    assert_eq!(view.lines[2], [(3, 0)]);
}

#[test]
fn prompts_jump_to_the_answer_branch() {
    let text = b"a<YESNO><YES>b<END><NO>c<END>";
    let mut d = Dialogue::new(script(text, 100));
    play(&mut d, 2);
    let prompt = d.view().prompt.unwrap();
    assert_eq!((prompt.kind, prompt.cursor), (PromptKind::YesNo, 0));
    // Down then confirm in the same step picks the second answer.
    d.step(Input {
        down: true,
        confirm: true,
        ..Input::default()
    });
    play(&mut d, 3);
    assert_eq!(
        d.view().lines[0],
        [(2, 0)],
        "the No branch replaces the question"
    );
    press_and_close(&mut d);
    assert_eq!(d.outcome().and_then(|o| o.answer), Some(1));

    // B answers No, and the first answer is Yes.
    let mut d = Dialogue::new(script(text, 100));
    play(&mut d, 2);
    d.step(Input {
        cancel: true,
        ..Input::default()
    });
    play(&mut d, 1);
    assert_eq!(d.view().lines[0], [(2, 0)]);
    let mut d = Dialogue::new(script(text, 100));
    play(&mut d, 2);
    d.step(press());
    play(&mut d, 1);
    assert_eq!(d.view().lines[0], [(1, 0)]);
}

/// A question without `<YES>`/`<NO>` markers (the scripts of events read the answer with
/// native 558): the answer callback (`0x020678cc`) finds no branch (`0x0206b2d8`) and closes the
/// window at once, so one press answers and the scene goes on. It used to wait on an arrow for a
/// second press.
#[test]
fn a_prompt_without_branches_closes_on_its_answer_with_no_second_key() {
    for hold in [false, true] {
        let text = b"a<NOYES>";
        let mut d = if hold {
            Dialogue::for_event(script(text, 100))
        } else {
            Dialogue::new(script(text, 100))
        };
        play(&mut d, 2);
        assert_eq!(d.view().prompt.unwrap().cursor, 1, "NOYES starts on No");
        d.step(press());
        assert_eq!(
            d.outcome(),
            Some(Outcome {
                end: EndKind::Wait,
                answer: Some(1)
            }),
            "the answer closed the window, hold_at_end {hold}"
        );
    }
}

/// An answer whose branch is only `<END>` (NPC 130's "Yes" in `X01M07`): the window closes with
/// the answer instead of waiting on an empty box for a second press. A branch with text still
/// waits for its key.
#[test]
fn an_empty_answer_branch_closes_with_the_answer() {
    let text = b"a<YESNO><NO>b<END><YES><END>";
    let mut d = Dialogue::new(script(text, 100));
    play(&mut d, 2);
    d.step(press());
    play(&mut d, 2);
    assert_eq!(
        d.outcome(),
        Some(Outcome {
            end: EndKind::Wait,
            answer: Some(0)
        }),
        "Yes closed the window"
    );
    let mut d = Dialogue::new(script(text, 100));
    play(&mut d, 2);
    d.step(Input {
        down: true,
        confirm: true,
        ..Input::default()
    });
    play(&mut d, 3);
    assert!(d.waiting(), "No has a text: it waits for its key");
    assert!(
        !d.view().arrow,
        "a typed branch ends on state 0, which draws none"
    );
    assert_eq!(d.outcome(), None);
}

/// Opens `tag` at the start of a message and checks its opening sound was emitted once.
fn open_prompt(tag: &[u8]) -> Dialogue {
    let mut text = tag.to_vec();
    text.extend_from_slice(b"<YES><END><NO><END>");
    let mut d = Dialogue::new(script(&text, 100));
    d.step(Input::default());
    assert!(d.view().prompt.is_some(), "the prompt is on screen");
    assert_eq!(d.take_sounds(), [Sound::Choice], "one opening sound");
    d
}

/// One step on a prompt, returning the sounds it made.
fn answer(d: &mut Dialogue, input: Input) -> Vec<Sound> {
    d.step(input);
    d.take_sounds()
}

fn choose_second() -> Input {
    Input {
        down: true,
        confirm: true,
        ..Input::default()
    }
}

#[test]
fn choice_answers_play_effect_one_except_notse_variants() {
    // <YESNO>, <NOYES> and <UKEYAME> sound on both answers (0xFF04, 0xFF05, 0xFF08).
    for tag in [&b"<YESNO>"[..], &b"<NOYES>"[..], &b"<UKEYAME>"[..]] {
        let mut d = open_prompt(tag);
        assert_eq!(answer(&mut d, press()), [Sound::ChoiceConfirm], "{tag:?}");
        let mut d = open_prompt(tag);
        assert_eq!(
            answer(&mut d, choose_second()),
            [Sound::ChoiceConfirm],
            "{tag:?}"
        );
    }
    // <YESNO_NOTSE> (0xFF06) never sounds.
    for input in [press(), choose_second()] {
        let mut d = open_prompt(b"<YESNO_NOTSE>");
        assert!(answer(&mut d, input).is_empty());
    }
    // <YESNO_NOTSE_IIE> (0xFF07) sounds on the first answer only.
    let mut d = open_prompt(b"<YESNO_NOTSE_IIE>");
    assert_eq!(answer(&mut d, press()), [Sound::ChoiceConfirm]);
    let mut d = open_prompt(b"<YESNO_NOTSE_IIE>");
    assert!(answer(&mut d, choose_second()).is_empty());
}

#[test]
fn the_confirmation_sound_is_emitted_once_per_answer() {
    let mut d = open_prompt(b"<YESNO>");
    assert_eq!(answer(&mut d, press()), [Sound::ChoiceConfirm]);
    for _ in 0..3 {
        d.step(Input::default());
        assert!(d.take_sounds().is_empty(), "the answer is not repeated");
    }
}

#[test]
fn cancel_answers_the_second_choice_silently_and_wins_over_confirm() {
    for tag in [&b"<YESNO>"[..], &b"<NOYES>"[..], &b"<UKEYAME>"[..]] {
        let mut d = open_prompt(tag);
        assert!(
            answer(
                &mut d,
                Input {
                    cancel: true,
                    ..Input::default()
                }
            )
            .is_empty()
        );
        play(&mut d, 1);
        press_and_close(&mut d);
        assert_eq!(d.outcome().and_then(|o| o.answer), Some(1), "{tag:?}");

        // A and B in the same step: B wins and the confirmation effect is not played.
        let mut d = open_prompt(tag);
        let both = Input {
            cancel: true,
            confirm: true,
            ..Input::default()
        };
        assert!(answer(&mut d, both).is_empty(), "{tag:?}");
        play(&mut d, 1);
        press_and_close(&mut d);
        assert_eq!(d.outcome().and_then(|o| o.answer), Some(1), "{tag:?}");
    }
}

#[test]
fn the_choice_cursor_toggles_silently_between_its_two_positions() {
    // <YESNO> starts on the first answer; down, up, up and down all toggle without sound.
    let mut d = open_prompt(b"<YESNO>");
    for (input, cursor) in [
        (
            Input {
                down: true,
                ..Input::default()
            },
            1,
        ),
        (
            Input {
                up: true,
                ..Input::default()
            },
            0,
        ),
        (
            Input {
                up: true,
                ..Input::default()
            },
            1,
        ),
        (
            Input {
                down: true,
                ..Input::default()
            },
            0,
        ),
    ] {
        assert!(answer(&mut d, input).is_empty(), "moving is silent");
        assert_eq!(d.view().prompt.unwrap().cursor, cursor);
    }
    // <NOYES> starts on the second answer.
    let mut d = open_prompt(b"<NOYES>");
    assert_eq!(d.view().prompt.unwrap().cursor, 1);
    assert!(
        answer(
            &mut d,
            Input {
                up: true,
                ..Input::default()
            }
        )
        .is_empty()
    );
    assert_eq!(d.view().prompt.unwrap().cursor, 0);
    assert!(
        answer(
            &mut d,
            Input {
                down: true,
                ..Input::default()
            }
        )
        .is_empty()
    );
    assert_eq!(d.view().prompt.unwrap().cursor, 1);
    // Nothing happens without a key.
    for _ in 0..3 {
        d.step(Input::default());
        assert!(d.take_sounds().is_empty());
    }
    assert_eq!(d.view().prompt.unwrap().cursor, 1);
}

fn npc(id: u16, kind: u8, position: [i32; 3], zones: Vec<TalkZone>) -> Talkable {
    Talkable {
        id,
        kind,
        position,
        zones,
        extra: None,
    }
}

fn fx(units: f32) -> i32 {
    (units * 4096.0) as i32
}

/// Yaw of a hero facing the game's angle 0 (+z).
fn facing_z() -> f32 {
    crate::field::yaw_of_angle(0.0)
}

#[test]
fn the_partner_is_in_range_and_the_most_faced() {
    let npcs = [
        npc(1, 0, [fx(1.2), 0, 0], vec![]),
        npc(2, 0, [0, 0, fx(1.0)], vec![]),
        npc(3, 0, [0, 0, fx(3.0)], vec![]),
    ];
    let target = nearest(&npcs, Vec3::ZERO, facing_z());
    assert_eq!(target, Some(Target { npc: 2, context: 0 }));
    // Turned towards +x, the other one is faced.
    let east = crate::field::yaw_of_angle(std::f32::consts::FRAC_PI_2);
    assert_eq!(nearest(&npcs, Vec3::ZERO, east).map(|t| t.npc), Some(1));
    // Out of reach: 1.6 on one axis passes the 1.5 square.
    let far = [npc(4, 0, [0, 0, fx(1.6)], vec![])];
    assert_eq!(nearest(&far, Vec3::ZERO, facing_z()), None);
    // Within the square but past the 1.75 distance: diagonal.
    let diagonal = [npc(5, 0, [fx(1.4), 0, fx(1.4)], vec![])];
    assert_eq!(nearest(&diagonal, Vec3::ZERO, facing_z()), None);
}

#[test]
fn zones_give_the_context_and_things_need_them() {
    let zone = TalkZone {
        rect: [fx(2.0), fx(2.0), fx(-2.0), fx(-2.0)],
        label: 80,
    };
    let sign = npc(9, 1, [fx(10.0), 0, 0], vec![zone]);
    // A thing to examine is only reached from a zone, however far its position.
    assert_eq!(
        nearest(std::slice::from_ref(&sign), Vec3::ZERO, facing_z()),
        Some(Target {
            npc: 9,
            context: 80
        })
    );
    let outside = Vec3::new(2.5, 0.0, 0.0);
    assert_eq!(
        nearest(std::slice::from_ref(&sign), outside, facing_z()),
        None
    );
    let near_sign = npc(9, 1, [fx(2.5), 0, 0], vec![]);
    assert_eq!(nearest(&[near_sign], outside, facing_z()), None);
    // The zone condition of the triggers reads the same test.
    let zones = zones_containing(&[sign], Vec3::ZERO);
    assert_eq!(zones.get(&9), Some(&true));
}

#[test]
fn counters_saturate_and_the_map_one_resets() {
    let mut counts = TalkCounts::default();
    assert_eq!(counts.get(7), [0, 0]);
    for _ in 0..20 {
        counts.record(7);
    }
    assert_eq!(counts.get(7), [15, 15]);
    counts.enter_map();
    assert_eq!(counts.get(7), [15, 0]);
    counts.clear();
    assert_eq!(counts.get(7), [0, 0]);
    // Action 0x6a: one NPC only; numbers past 255 are outside the game's tables.
    counts.record(7);
    counts.record(8);
    counts.record(300);
    counts.reset(7);
    assert_eq!(counts.get(7), [0, 0]);
    assert_eq!(counts.get(8), [1, 1]);
    counts.reset(300);
    assert_eq!(counts.get(300), [1, 1]);
}

#[test]
fn jumps_continue_after_their_label() {
    // Yes jumps over the No branch to the label B.
    let text = b"a<YESNO><YES><JP_B><NO>b<END><LB_B>c<END>";
    let mut d = Dialogue::new(script(text, 100));
    play(&mut d, 2);
    d.step(press());
    play(&mut d, 2);
    assert_eq!(
        d.view().lines[0],
        [(2, 0)],
        "the jump lands on the label's text"
    );
}

#[test]
fn an_answer_without_branch_ends_the_message() {
    // "Yes" loops back to the question; "No" has no branch and must not follow the jump.
    let mut d = Dialogue::new(script(b"<LB_A>a<YESNO><YES><JP_A><END>", 100));
    // The label spends a unit like any code, then "a", then the prompt opens.
    play(&mut d, 3);
    d.step(Input {
        cancel: true,
        ..Input::default()
    });
    assert_eq!(
        d.outcome().and_then(|o| o.answer),
        Some(1),
        "the message ended with the answer"
    );
}
