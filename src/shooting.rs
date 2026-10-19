//! Shooting programs. EXE 00629638 types A, B, C and D, with the optional
//! special sequence. See docs/research/SHOOTING.md. Device timing and the
//! runtime shooting state are not validated by this encoding.
use crate::storage::{END, NO_OPERATION};

const LEFT_DOWN: u16 = 0x2501;
const LEFT_UP: u16 = 0x2401;
const EXEC_DELAY: u16 = 0x2608;
const ENABLE_MOVER: u16 = 0x2609;
const MOVE_RECOVERY: u16 = 0x260a;
const STOP: u16 = 0x262d;

/// One shot. Movement uses converted device counts, not screen pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shot {
    pub interval_ms: u32,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cycle {
    B,
    C,
    D,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Program {
    pub press: Vec<u16>,
    pub release: Vec<u16>,
}

/// EXE 0061a4ef..0061b05a packs the first three D slots into auxiliary
/// words 17..19. `y` already has the device sign. Keep the EXE arithmetic,
/// including the interval's carry into the high byte when it exceeds 255.
pub fn vertical_parameter(shot: Shot) -> Result<u16, String> {
    check(&[shot], 1, |_, shot| outside(shot.y))?;
    Ok((shot.interval_ms as i32 + shot.y * 256) as u16)
}

// core_var 0, VarH = VarH & 2, and a conditional branch back. A restarts at
// LeftDown. B, C and D restart at core_var, after LeftDown or NOP.
fn state_wait(words: &mut Vec<u16>, type_a: bool) {
    words.extend([
        0x9007,
        0,
        0x8307,
        0x0702,
        0x8707,
        0x00ff,
        if type_a { 0xfffb } else { 0xfffc },
    ]);
}

fn motion(x: i32, y: i32) -> u16 {
    0x4000 | ((x as u16 & 0x7f) << 7) | (y as u16 & 0x7f)
}

// A magnitude above 63 splits into two moves. The EXE divides toward zero
// and keeps the remainder in the second move.
fn compensate(words: &mut Vec<u16>, mut x: i32, mut y: i32) {
    if x.abs() > 63 || y.abs() > 63 {
        let (half_x, half_y) = (x / 2, y / 2);
        words.extend([ENABLE_MOVER, motion(half_x, half_y)]);
        x -= half_x;
        y -= half_y;
    }
    words.extend([ENABLE_MOVER, motion(x, y)]);
}

fn split(interval: u32, fixed_press: bool) -> (u16, u16) {
    if fixed_press && interval >= 40 {
        (20, (interval - 20) as u16)
    } else {
        let half = (interval / 2).max(1) as u16;
        (half, half)
    }
}

fn check(
    shots: &[Shot],
    limit: usize,
    movement: impl Fn(usize, &Shot) -> bool,
) -> Result<(), String> {
    if shots.is_empty() || shots.len() > limit {
        return Err(format!("射击次数应为 1 至 {limit}"));
    }
    for (index, shot) in shots.iter().enumerate() {
        if !(1..=999).contains(&shot.interval_ms) {
            return Err("射击间隔应为 1 至 999 毫秒".into());
        }
        if movement(index, shot) {
            return Err("补偿位移应为 -126 至 126".into());
        }
    }
    Ok(())
}

fn outside(value: i32) -> bool {
    !(-126..=126).contains(&value)
}

/// Type A: 1 to 5 shots. The final shot's movement is unused.
/// Return one complete macro, including END.
pub fn burst(shots: &[Shot], recover: bool, special: bool) -> Result<Vec<u16>, String> {
    check(shots, 5, |index, shot| {
        index + 1 < shots.len() && (outside(shot.x) || outside(shot.y))
    })?;
    let mut words = Vec::new();
    for (index, shot) in shots.iter().enumerate() {
        let (down, up) = split(shot.interval_ms, true);
        words.push(LEFT_DOWN);
        if special {
            state_wait(&mut words, true);
        }
        words.extend([EXEC_DELAY, 0x2800 + down, LEFT_UP]);
        if index + 1 < shots.len() {
            words.extend([EXEC_DELAY, 0x2800 + up]);
            compensate(&mut words, shot.x, shot.y);
        }
    }
    if recover {
        words.push(MOVE_RECOVERY);
    }
    words.push(END);
    Ok(words)
}

/// Types B, C and D. B and C accept 1 to 8 shots; D accepts one shot and
/// uses only its Y movement. `loop_start` is zero-based. Unlike A, each shot
/// includes its trailing delay and movement.
pub fn cycle(
    kind: Cycle,
    shots: &[Shot],
    loop_start: usize,
    recover: bool,
    special: bool,
) -> Result<Program, String> {
    let d = kind == Cycle::D;
    check(shots, if d { 1 } else { 8 }, |_, shot| {
        (!d && outside(shot.x)) || outside(shot.y)
    })?;
    if loop_start >= shots.len() {
        return Err("循环起点超出射击次数".into());
    }
    let mut press = Vec::new();
    let mut target = 0;
    for (index, shot) in shots.iter().enumerate() {
        if index == loop_start {
            target = press.len();
        }
        let c_hold = kind == Cycle::C && index != 0;
        press.push(if c_hold { NO_OPERATION } else { LEFT_DOWN });
        if special {
            state_wait(&mut press, false);
        }
        let (down, up) = split(shot.interval_ms, !d);
        press.extend([EXEC_DELAY, 0x2800 + down]);
        if kind != Cycle::C {
            press.push(LEFT_UP);
        }
        press.extend([EXEC_DELAY, 0x2800 + up]);
        compensate(&mut press, if d { 0 } else { shot.x }, shot.y);
    }
    // The branch is relative to itself. The largest displacement is -136.
    press.push(((target as i32 - press.len() as i32) & 0x1fff) as u16);
    press.push(END);
    let mut release = Vec::new();
    if recover {
        release.extend([0x2810, MOVE_RECOVERY]);
    }
    release.extend([0x2810, STOP, END]);
    Ok(Program { press, release })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::validate;

    fn shot(interval_ms: u32, x: i32, y: i32) -> Shot {
        Shot { interval_ms, x, y }
    }

    #[test]
    fn vertical_parameter_reference_words() {
        for (interval, y, word) in [
            (1, 0, 0x0001),
            (100, -20, 0xec64),
            (255, 126, 0x7eff),
            (999, 126, 0x81e7),
            (999, -126, 0x85e7),
        ] {
            assert_eq!(vertical_parameter(shot(interval, i32::MIN, y)), Ok(word));
        }
        assert!(vertical_parameter(shot(0, 0, 0)).is_err());
        assert!(vertical_parameter(shot(1000, 0, 0)).is_err());
        assert!(vertical_parameter(shot(100, 0, 127)).is_err());
    }

    #[test]
    fn burst_reference() {
        let words = burst(
            &[shot(10, 0, 0), shot(80, 126, -126), shot(55, 0, 0)],
            true,
            false,
        )
        .unwrap();
        assert_eq!(
            words,
            [
                0x2501, 0x2608, 0x2805, 0x2401, 0x2608, 0x2805, 0x2609, 0x4000, 0x2501, 0x2608,
                0x2814, 0x2401, 0x2608, 0x283c, 0x2609, 0x5fc1, 0x2609, 0x5fc1, 0x2501, 0x2608,
                0x2814, 0x2401, 0x260a, END
            ]
        );
        assert_eq!(validate(&words), Ok(()));
    }

    #[test]
    fn burst_intervals_and_last_shot() {
        for (interval, down, up) in [
            (1, 1, 1),
            (2, 1, 1),
            (3, 1, 1),
            (39, 19, 19),
            (40, 20, 20),
            (41, 20, 21),
            (999, 20, 979),
        ] {
            let words = burst(
                &[shot(interval, 0, 0), shot(1, i32::MIN, i32::MAX)],
                false,
                false,
            )
            .unwrap();
            assert_eq!(words.len(), 13);
            assert_eq!((words[2], words[5]), (0x2800 + down, 0x2800 + up));
            assert_eq!((words[10], words[12]), (0x2801, END));
        }
        assert_eq!(
            burst(&[shot(1, i32::MAX, i32::MIN)], false, false)
                .unwrap()
                .len(),
            5
        );
        assert!(burst(&[], false, false).is_err());
        assert!(burst(&[shot(1000, 0, 0)], false, false).is_err());
        assert!(burst(&[shot(1, 127, 0), shot(1, 0, 0)], false, false).is_err());
        assert!(burst(&[shot(1, 0, 0); 6], false, false).is_err());
    }

    #[test]
    fn split_motion_keeps_totals() {
        for x in -126..=126 {
            for y in -126..=126 {
                let words = burst(&[shot(40, x, y), shot(25, 0, 0)], false, false).unwrap();
                let (mut sum_x, mut sum_y) = (0, 0);
                for word in words.iter().filter(|w| (0x4000..0x8000).contains(*w)) {
                    sum_x += ((((word >> 7) & 127) as i32) << 25) >> 25;
                    sum_y += (((word & 127) as i32) << 25) >> 25;
                }
                assert_eq!((sum_x, sum_y), (x, y));
            }
        }
    }

    #[test]
    fn cycle_references() {
        let release = [0x2810, 0x260a, 0x2810, 0x262d, END];
        let b = cycle(
            Cycle::B,
            &[shot(39, 0, 0), shot(41, 126, -125)],
            1,
            true,
            false,
        )
        .unwrap();
        assert_eq!(
            b.press,
            [
                0x2501, 0x2608, 0x2813, 0x2401, 0x2608, 0x2813, 0x2609, 0x4000, 0x2501, 0x2608,
                0x2814, 0x2401, 0x2608, 0x2815, 0x2609, 0x5fc2, 0x2609, 0x5fc1, 0x1ff6, END
            ]
        );
        assert_eq!(b.release, release);
        let c = cycle(
            Cycle::C,
            &[shot(55, 0, 0), shot(1, -126, 126)],
            1,
            false,
            false,
        )
        .unwrap();
        assert_eq!(
            c.press,
            [
                0x2501, 0x2608, 0x2814, 0x2608, 0x2823, 0x2609, 0x4000, 0x262e, 0x2608, 0x2801,
                0x2608, 0x2801, 0x2609, 0x60bf, 0x2609, 0x60bf, 0x1ff7, END
            ]
        );
        assert_eq!(c.release, release[2..]);
        let d = cycle(Cycle::D, &[shot(999, i32::MIN, -125)], 0, true, false).unwrap();
        assert_eq!(
            d.press,
            [
                0x2501, 0x2608, 0x29f3, 0x2401, 0x2608, 0x29f3, 0x2609, 0x4042, 0x2609, 0x4041,
                0x1ff6, END
            ]
        );
        assert_eq!(d.release, release);
        for program in [&b, &c, &d] {
            assert_eq!(validate(&program.press), Ok(()));
            assert_eq!(validate(&program.release), Ok(()));
        }
        assert!(cycle(Cycle::D, &[shot(1, 0, 0); 2], 0, false, false).is_err());
        assert!(cycle(Cycle::B, &[shot(1, 0, 0); 2], 2, false, false).is_err());
        assert!(cycle(Cycle::B, &[shot(1, 0, 0); 9], 0, false, false).is_err());
    }

    #[test]
    fn special_templates_and_maximum() {
        let wait = |last| [0x2501, 0x9007, 0, 0x8307, 0x0702, 0x8707, 0x00ff, last];
        let a = burst(&[shot(40, 0, 0)], true, true).unwrap();
        assert_eq!(a[..8], wait(0xfffb));
        assert_eq!(a[8..], [0x2608, 0x2814, 0x2401, 0x260a, END]);
        let b = cycle(Cycle::B, &[shot(40, 0, 0)], 0, false, true).unwrap();
        assert_eq!(b.press[..8], wait(0xfffc));
        assert_eq!(
            b.press[8..],
            [
                0x2608, 0x2814, 0x2401, 0x2608, 0x2814, 0x2609, 0x4000, 0x1ff1, END
            ]
        );
        let c = cycle(Cycle::C, &[shot(40, 0, 0)], 0, false, true).unwrap();
        assert_eq!(
            c.press[8..],
            [0x2608, 0x2814, 0x2608, 0x2814, 0x2609, 0x4000, 0x1ff2, END]
        );
        let d = cycle(Cycle::D, &[shot(999, i32::MAX, 126)], 0, false, true).unwrap();
        assert_eq!(
            d.press[8..],
            [
                0x2608, 0x29f3, 0x2401, 0x2608, 0x29f3, 0x2609, 0x403f, 0x2609, 0x403f, 0x1fef, END
            ]
        );
        for program in [&a, &b.press, &c.press, &d.press] {
            assert_eq!(validate(program), Ok(()));
        }
        let shots = [shot(999, -126, 126); 8];
        let a = burst(&shots[..5], true, true).unwrap();
        assert_eq!(a.len(), 81);
        assert_eq!(validate(&a), Ok(()));
        let b = cycle(Cycle::B, &shots, 0, true, true).unwrap();
        assert_eq!(b.press.len(), 138);
        assert_eq!(validate(&b.press), Ok(()));
    }
}
