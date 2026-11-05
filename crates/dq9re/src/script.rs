//! `dq9re script`: runs event scripts headless with a fake application that completes every
//! wait at once, and prints the event trace (`docs/re/stb.md`, § Interpréteur).

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use dq9_core::script::{Director, Event, Frame, Text};
use dq9_core::trigger::{Request, State};
use nds_rom::Rom;

use crate::stb::stage_events;

/// Updates after which a script is declared stuck.
const MAX_FRAMES: u32 = 200_000;

/// Outcome of one script.
#[derive(Debug, Default)]
pub struct Report {
    pub events: usize,
    pub frames: u32,
    /// Natives reported unimplemented, with their call counts.
    pub unimplemented: BTreeMap<i32, u32>,
    pub failure: Option<String>,
}

/// Runs `event` to its end. `trace` receives `(frame, event, message text)` as they come.
pub fn run_event(
    rom: &Rom,
    event: u16,
    lang: &str,
    mut trace: impl FnMut(u32, &Event, Option<&str>),
) -> Report {
    let mut report = Report::default();
    let mut director = Director::new(lang);
    director.dispatch(vec![Request::Event(event)]);
    let world = State::default();
    while director.is_active() {
        if report.frames >= MAX_FRAMES {
            report.failure = Some(format!("still running after {MAX_FRAMES} updates"));
            break;
        }
        let events = director.step(rom, Frame::default(), &world);
        for e in &events {
            report.events += 1;
            let text = match e {
                Event::Message {
                    text: Text::Id(id), ..
                } => director.texts().get(id).map(String::as_str),
                _ => None,
            };
            trace(report.frames, e, text);
            match e {
                Event::Fault(fault) => report.failure = Some(fault.to_string()),
                Event::LoadFailed { reason, .. } => report.failure = Some(reason.clone()),
                _ => {}
            }
            if let Some(signal) = e.instant_signal() {
                director.signal(signal);
            }
        }
        report.frames += 1;
    }
    report.unimplemented = director.unimplemented().clone();
    report
}

/// `file|event|stage`: an archive path (`data/event/ev01130.gp2`), an event number, or a
/// story stage (`1.1-3`).
fn events_of(rom: &Rom, target: &str) -> Result<Vec<u16>> {
    if let Ok(event) = target.parse::<u16>() {
        return Ok(vec![event]);
    }
    if target.contains('/') || target.ends_with(".gp2") {
        let stem = target.rsplit('/').next().unwrap_or(target);
        let digits = stem.trim_start_matches("ev").trim_end_matches(".gp2");
        return Ok(vec![
            digits
                .parse()
                .with_context(|| format!("no event number in {target}"))?,
        ]);
    }
    let events = stage_events(rom, target)?;
    if events.is_empty() {
        bail!("no scene at stage {target}");
    }
    Ok(events.into_iter().map(|(event, _)| event).collect())
}

pub fn run(rom: &Rom, target: &str, lang: &str, quiet: bool) -> Result<()> {
    let events = events_of(rom, target)?;
    let mut total: BTreeMap<i32, u32> = BTreeMap::new();
    let mut failures = 0;
    for &event in &events {
        if !quiet {
            println!("== event {event}");
        }
        let mut seen = BTreeMap::new();
        let report = run_event(rom, event, lang, |frame, e, text| {
            if let Event::Unimplemented { id } = e {
                seen.insert(*id, frame);
            }
            if !quiet {
                match text {
                    Some(text) => println!(
                        "{frame:>6}  {e:?}  # {}",
                        crate::text::escape(text.as_bytes())
                    ),
                    None => println!("{frame:>6}  {e:?}"),
                }
            }
        });
        for (id, calls) in &report.unimplemented {
            *total.entry(*id).or_default() += calls;
        }
        println!(
            "event {event}: {} events in {} updates{}",
            report.events,
            report.frames,
            report
                .failure
                .as_deref()
                .map(|f| {
                    failures += 1;
                    format!(", FAILED: {f}")
                })
                .unwrap_or_default()
        );
    }
    println!(
        "{} scripts, {failures} failed, {} unimplemented natives",
        events.len(),
        total.len()
    );
    for (id, calls) in &total {
        println!(
            "  {id:>4} {calls:>6} calls  {}",
            dq9_formats::stb::native_name(*id).unwrap_or("-")
        );
    }
    if failures > 0 {
        bail!("{failures} scripts failed");
    }
    Ok(())
}
