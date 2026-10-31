//! Alert text in normal and brainrot flavours (toggle in Alerts settings).

use crate::policy::Kind;

pub struct Line {
    pub title: String,
    pub body: String,
}

fn pick(brainrot: bool, normal: (&str, &str), rot: (&str, &str)) -> Line {
    let (t, b) = if brainrot { rot } else { normal };
    Line { title: t.into(), body: b.into() }
}

pub fn warn(kind: &Kind, brainrot: bool) -> Line {
    match kind {
        Kind::Site(d) => pick(
            brainrot,
            (&format!("{d}? Not now."), "Close it and get back to the task. Next time the alarm goes off."),
            (&format!("bro opened {d}"), "close it. lock in. next time it's the siren, no cap."),
        ),
        other => alarm(other, brainrot),
    }
}

pub fn alarm(kind: &Kind, brainrot: bool) -> Line {
    match kind {
        Kind::Site(d) => pick(
            brainrot,
            (&format!("{d} again"), "Second strike. Close it and come back to the task."),
            (&format!("{d} AGAIN??"), "aura -1000. scuba scuba back to the books."),
        ),
        Kind::ScreenSite(d) => pick(
            brainrot,
            (&format!("Long scroll on {d}"), "You've been on the screen for a while and it isn't study material."),
            ("doomscroll detected", &format!("{d} is not the syllabus, chief.")),
        ),
        Kind::Absent => pick(
            brainrot,
            ("You left your desk", "Come back and press I'm back (or take a BRB pause next time)."),
            ("where did you go", "the books are lonely. come back."),
        ),
        Kind::LookingAway => pick(
            brainrot,
            ("Eyes off the work", "You've been looking away for a while. Back to the page."),
            ("bro is staring at the wall", "the wall is not on the exam. eyes on the page."),
        ),
        Kind::Phone => pick(
            brainrot,
            ("Phone spotted", "Put the phone away until the break."),
            ("PUT. THE. PHONE. DOWN.", "reels can wait. your rank can't."),
        ),
        Kind::BrbOverdue => pick(
            brainrot,
            ("BRB time is up", "Your pause ran out. Come back and press I'm back."),
            ("the water break became a side quest", "time's up. get back here."),
        ),
    }
}

pub fn ask(domain: &str, brainrot: bool) -> Line {
    pick(
        brainrot,
        (&format!("Is {domain} for studying?"), "Your answer is remembered for this session."),
        (&format!("{domain}: study or brainrot?"), "be honest. i'll remember."),
    )
}

pub fn posture(brainrot: bool) -> Line {
    pick(
        brainrot,
        ("You're leaning into the screen", "Sit back and relax your shoulders."),
        ("shrimp posture detected", "sit up. spine check."),
    )
}

pub fn phase_done(focus_ended: bool, brainrot: bool) -> Line {
    if focus_ended {
        pick(brainrot, ("Focus block done", "Take your break. Stand up, drink water."), ("W session", "go touch grass for a bit."))
    } else {
        pick(brainrot, ("Break's over", "Start the next focus block when ready."), ("break's over", "LOCK IN."))
    }
}

pub fn welcome_back(brainrot: bool) -> Line {
    pick(brainrot, ("Welcome back", "Timer resumed."), ("he's back", "aura restored. timer resumed."))
}
