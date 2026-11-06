use dq9_formats::font::{Font, FontId};
use dq9_formats::message::{LeadingTags, compile};

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
        ..Input::default()
    }
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
fn typing_is_one_glyph_per_step_and_pages_wait() {
    let mut d = Dialogue::new(script(b"ab<PAGE>cd", 100));
    play(&mut d, 1);
    assert_eq!(d.view().lines[0].len(), 1);
    play(&mut d, 1);
    assert_eq!(d.view().lines[0].len(), 2);
    assert!(!d.view().arrow);
    play(&mut d, 1);
    assert!(d.view().arrow, "the page waits for the action key");
    play(&mut d, 5);
    assert_eq!(d.view().lines[0].len(), 2, "nothing moves without input");
    d.step(press());
    assert!(d.view().lines[0].is_empty(), "the box is cleared");
    d.step(press());
    assert_eq!(
        d.view().lines[0].len(),
        2,
        "the action key types up to the next wait"
    );
    assert!(d.view().arrow);
}

#[test]
fn end_waits_for_the_key_then_closes() {
    let mut d = Dialogue::new(script(b"a", 100));
    play(&mut d, 3);
    assert!(d.view().arrow);
    assert_eq!(d.outcome(), None);
    d.step(press());
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

#[test]
fn a_fourth_line_scrolls_the_box() {
    let mut d = Dialogue::new(script(b"a\\nb\\nc\\nd", 100));
    play(&mut d, 5);
    assert_eq!(d.view().lines.len(), 3);
    play(&mut d, usize::try_from(SCROLL_STEPS).unwrap() + 1);
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
    d.step(press());
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

#[test]
fn a_prompt_without_branches_falls_through_with_its_answer() {
    let mut d = Dialogue::new(script(b"a<NOYES>", 100));
    play(&mut d, 2);
    assert_eq!(d.view().prompt.unwrap().cursor, 1, "NOYES starts on No");
    d.step(press());
    play(&mut d, 1);
    assert!(d.view().arrow);
    d.step(press());
    assert_eq!(d.outcome().and_then(|o| o.answer), Some(1));
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
        d.step(press());
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
        d.step(press());
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
    play(&mut d, 2);
    d.step(Input {
        cancel: true,
        ..Input::default()
    });
    play(&mut d, 1);
    assert!(d.view().arrow, "the message ended");
    d.step(press());
    assert_eq!(d.outcome().and_then(|o| o.answer), Some(1));
}
