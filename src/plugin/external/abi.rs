//! The coarse projection an external plugin sees.
//!
//! Tier 1 hands a plugin the *real* `SliceLayer` and the *real*
//! `clipper2::Paths`, which is the whole reason it is a compile-time trait.
//! Nothing like that can cross a sandbox boundary without either losing
//! fidelity or paying for the marshalling per layer, per slice.
//!
//! So Tier 2 does not get every hook family — it gets the one whose data is
//! flat enough to project honestly: **move filters**. A move is a handful of
//! numbers and an enum, and that survives a boundary intact.
//!
//! ## The wire format
//!
//! A header followed by one fixed-size record per move, little-endian
//! throughout. Fixed records rather than a self-describing encoding because a
//! program is hundreds of thousands of moves and the guest has to walk all of
//! them: JSON here would cost more than the filter saves.
//!
//! Text the guest cannot meaningfully edit — comments, and the already-rendered
//! `Raw` fragments that carry fan, temperature and markers — stays **host-side**
//! and travels as an id. The guest may reorder or drop those records, and the
//! host resolves each id back to the original text when it rebuilds the
//! program. A guest therefore cannot invent new G-code text, only rearrange and
//! rewrite motion. That is a deliberate narrowing of what a sandboxed plugin
//! can do, not an oversight.

use crate::core::ExtrusionRole;
use crate::gcode::{Move, MoveProgram};

/// Magic prefix, so a guest can reject a buffer it does not understand.
pub const MAGIC: u32 = 0x4d56_5031; // "MVP1"

/// Version of the record layout. Bumped whenever a field moves.
pub const ABI_VERSION: u32 = 1;

/// Bytes per record.
pub const RECORD_BYTES: usize = 72;

/// Bytes in the header: magic, version, count, reserved.
pub const HEADER_BYTES: usize = 16;

/// Sentinel for "no comment" / "not a raw fragment".
pub const NO_ID: u32 = u32::MAX;

/// Record kinds, matching [`Move`]'s variants.
pub mod kind {
    /// [`crate::gcode::Move::Extrude`].
    pub const EXTRUDE: u8 = 0;
    /// [`crate::gcode::Move::Travel`].
    pub const TRAVEL: u8 = 1;
    /// [`crate::gcode::Move::ZMove`].
    pub const Z_MOVE: u8 = 2;
    /// [`crate::gcode::Move::Extruder`].
    pub const EXTRUDER: u8 = 3;
    /// [`crate::gcode::Move::Raw`] — opaque, referenced by id.
    pub const RAW: u8 = 4;
}

/// The host-side strings a projected program refers to by id.
///
/// Kept out of the guest's reach on purpose: it is what stops a sandboxed
/// plugin from writing arbitrary G-code text into the output.
#[derive(Debug, Default, Clone)]
pub struct SideTable {
    raw: Vec<String>,
    comments: Vec<String>,
}

impl SideTable {
    /// Resolve a raw fragment by id.
    pub fn raw(&self, id: u32) -> Option<&str> {
        self.raw.get(id as usize).map(String::as_str)
    }

    /// Resolve a comment by id.
    pub fn comment(&self, id: u32) -> Option<&str> {
        self.comments.get(id as usize).map(String::as_str)
    }

    fn intern_raw(&mut self, text: &str) -> u32 {
        let id = self.raw.len() as u32;
        self.raw.push(text.to_string());
        id
    }

    fn intern_comment(&mut self, text: &Option<String>) -> u32 {
        match text {
            None => NO_ID,
            Some(t) => {
                if let Some(existing) = self.comments.iter().position(|c| c == t) {
                    return existing as u32;
                }
                let id = self.comments.len() as u32;
                self.comments.push(t.clone());
                id
            }
        }
    }
}

fn role_code(role: ExtrusionRole) -> u8 {
    match role {
        ExtrusionRole::OuterWall => 0,
        ExtrusionRole::InnerWall => 1,
        ExtrusionRole::OverhangPerimeter => 2,
        ExtrusionRole::Infill => 3,
        ExtrusionRole::Bridge => 4,
        ExtrusionRole::TopSurface => 5,
        ExtrusionRole::BottomSurface => 6,
        ExtrusionRole::InternalSolid => 7,
        ExtrusionRole::GapFill => 8,
        ExtrusionRole::Support => 9,
        ExtrusionRole::Skirt => 10,
        ExtrusionRole::Ironing => 11,
    }
}

fn role_from(code: u8) -> ExtrusionRole {
    match code {
        1 => ExtrusionRole::InnerWall,
        2 => ExtrusionRole::OverhangPerimeter,
        3 => ExtrusionRole::Infill,
        4 => ExtrusionRole::Bridge,
        5 => ExtrusionRole::TopSurface,
        6 => ExtrusionRole::BottomSurface,
        7 => ExtrusionRole::InternalSolid,
        8 => ExtrusionRole::GapFill,
        9 => ExtrusionRole::Support,
        10 => ExtrusionRole::Skirt,
        11 => ExtrusionRole::Ironing,
        _ => ExtrusionRole::OuterWall,
    }
}

/// Project `program` into the wire format, plus the side table its ids refer
/// to.
pub fn encode(program: &MoveProgram) -> (Vec<u8>, SideTable) {
    let moves = program.moves();
    let mut side = SideTable::default();
    let mut buf = Vec::with_capacity(HEADER_BYTES + moves.len() * RECORD_BYTES);

    buf.extend_from_slice(&MAGIC.to_le_bytes());
    buf.extend_from_slice(&ABI_VERSION.to_le_bytes());
    buf.extend_from_slice(&(moves.len() as u32).to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());

    for m in moves {
        let mut rec = [0u8; RECORD_BYTES];
        let (k, role, has_z, x, y, z, e, de, feed, width, raw_id, comment_id) = match m {
            Move::Extrude {
                x,
                y,
                z,
                e,
                de,
                feed_mm_min,
                role,
                width_mm,
                comment,
            } => (
                kind::EXTRUDE,
                role_code(*role),
                u8::from(z.is_some()),
                *x,
                *y,
                z.unwrap_or(0.0),
                *e,
                *de,
                *feed_mm_min,
                *width_mm,
                NO_ID,
                side.intern_comment(comment),
            ),
            Move::Travel {
                x,
                y,
                feed_mm_min,
                comment,
            } => (
                kind::TRAVEL,
                0,
                0,
                *x,
                *y,
                0.0,
                0.0,
                0.0,
                *feed_mm_min,
                0.0,
                NO_ID,
                side.intern_comment(comment),
            ),
            Move::ZMove {
                z,
                feed_mm_min,
                comment,
            } => (
                kind::Z_MOVE,
                0,
                1,
                0.0,
                0.0,
                *z,
                0.0,
                0.0,
                *feed_mm_min,
                0.0,
                NO_ID,
                side.intern_comment(comment),
            ),
            Move::Extruder {
                e,
                de,
                feed_mm_min,
                comment,
            } => (
                kind::EXTRUDER,
                0,
                0,
                0.0,
                0.0,
                0.0,
                *e,
                *de,
                *feed_mm_min,
                0.0,
                NO_ID,
                side.intern_comment(comment),
            ),
            Move::Raw(text) => (
                kind::RAW,
                0,
                0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                side.intern_raw(text),
                NO_ID,
            ),
        };

        rec[0] = k;
        rec[1] = role;
        rec[2] = has_z;
        for (i, v) in [x, y, z, e, de, feed, width].iter().enumerate() {
            let at = 8 + i * 8;
            rec[at..at + 8].copy_from_slice(&v.to_le_bytes());
        }
        rec[64..68].copy_from_slice(&raw_id.to_le_bytes());
        rec[68..72].copy_from_slice(&comment_id.to_le_bytes());
        buf.extend_from_slice(&rec);
    }

    (buf, side)
}

/// What can go wrong reading a buffer back from a guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The buffer is not a projected program.
    BadMagic,
    /// The guest speaks a different record layout.
    BadVersion(u32),
    /// The buffer is shorter than its own header claims.
    Truncated,
    /// A record referred to a raw fragment or comment that does not exist.
    ///
    /// Rejected rather than dropped: a guest inventing ids is either confused
    /// or hostile, and either way its output should not be printed.
    UnknownId(u32),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadMagic => write!(f, "not a projected move program"),
            Self::BadVersion(v) => write!(f, "move ABI v{v}, expected v{ABI_VERSION}"),
            Self::Truncated => write!(f, "buffer shorter than its header claims"),
            Self::UnknownId(id) => write!(f, "record refers to unknown host string {id}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Rebuild a program from a guest's buffer, resolving ids through `side`.
pub fn decode(buf: &[u8], side: &SideTable) -> Result<MoveProgram, DecodeError> {
    if buf.len() < HEADER_BYTES {
        return Err(DecodeError::Truncated);
    }
    let magic = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    if magic != MAGIC {
        return Err(DecodeError::BadMagic);
    }
    let version = u32::from_le_bytes(buf[4..8].try_into().unwrap());
    if version != ABI_VERSION {
        return Err(DecodeError::BadVersion(version));
    }
    let count = u32::from_le_bytes(buf[8..12].try_into().unwrap()) as usize;
    if buf.len() < HEADER_BYTES + count * RECORD_BYTES {
        return Err(DecodeError::Truncated);
    }

    let mut program = MoveProgram::new();
    for i in 0..count {
        let at = HEADER_BYTES + i * RECORD_BYTES;
        let rec = &buf[at..at + RECORD_BYTES];
        let f = |n: usize| f64::from_le_bytes(rec[8 + n * 8..16 + n * 8].try_into().unwrap());
        let (x, y, z, e, de, feed, width) = (f(0), f(1), f(2), f(3), f(4), f(5), f(6));
        let raw_id = u32::from_le_bytes(rec[64..68].try_into().unwrap());
        let comment_id = u32::from_le_bytes(rec[68..72].try_into().unwrap());

        let comment = if comment_id == NO_ID {
            None
        } else {
            Some(
                side.comment(comment_id)
                    .ok_or(DecodeError::UnknownId(comment_id))?
                    .to_string(),
            )
        };

        let m = match rec[0] {
            kind::EXTRUDE => Move::Extrude {
                x,
                y,
                z: (rec[2] != 0).then_some(z),
                e,
                de,
                feed_mm_min: feed,
                role: role_from(rec[1]),
                width_mm: width,
                comment,
            },
            kind::TRAVEL => Move::Travel {
                x,
                y,
                feed_mm_min: feed,
                comment,
            },
            kind::Z_MOVE => Move::ZMove {
                z,
                feed_mm_min: feed,
                comment,
            },
            kind::EXTRUDER => Move::Extruder {
                e,
                de,
                feed_mm_min: feed,
                comment,
            },
            _ => Move::Raw(
                side.raw(raw_id)
                    .ok_or(DecodeError::UnknownId(raw_id))?
                    .to_string(),
            ),
        };
        program.push(m);
    }
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MoveProgram {
        let mut p = MoveProgram::new();
        p.raw(";TYPE:Outer wall\n");
        p.push(Move::Travel {
            x: 1.0,
            y: 2.0,
            feed_mm_min: 9000.0,
            comment: Some("travel".into()),
        });
        p.push(Move::Extrude {
            x: 3.0,
            y: 4.0,
            z: Some(0.35),
            e: 0.5,
            de: 0.5,
            feed_mm_min: 1800.0,
            role: ExtrusionRole::GapFill,
            width_mm: 0.42,
            comment: None,
        });
        p.push(Move::Extruder {
            e: -1.0,
            de: -1.0,
            feed_mm_min: 2400.0,
            comment: Some("retract".into()),
        });
        p
    }

    #[test]
    fn a_program_survives_the_round_trip_unchanged() {
        let original = sample();
        let (buf, side) = encode(&original);
        let back = decode(&buf, &side).unwrap();
        assert_eq!(original.moves(), back.moves());
    }

    #[test]
    fn every_record_is_the_same_size() {
        let (buf, _) = encode(&sample());
        assert_eq!(buf.len(), HEADER_BYTES + 4 * RECORD_BYTES);
    }

    #[test]
    fn a_guest_may_reorder_and_drop_records() {
        // The rewrite a filter exists to do — including on the opaque raw
        // fragments, which it can move but not author.
        let (buf, side) = encode(&sample());
        let mut reordered = buf[..HEADER_BYTES].to_vec();
        reordered[8..12].copy_from_slice(&2u32.to_le_bytes());
        let rec =
            |i: usize| &buf[HEADER_BYTES + i * RECORD_BYTES..HEADER_BYTES + (i + 1) * RECORD_BYTES];
        reordered.extend_from_slice(rec(2));
        reordered.extend_from_slice(rec(0));

        let back = decode(&reordered, &side).unwrap();
        assert_eq!(back.len(), 2);
        assert!(back.moves()[0].is_extrusion());
        assert!(matches!(back.moves()[1], Move::Raw(_)));
    }

    #[test]
    fn a_guest_cannot_invent_g_code_text() {
        // The point of keeping strings host-side: a sandboxed plugin can
        // rearrange and rewrite motion, but it cannot write arbitrary commands
        // into the output.
        let (buf, side) = encode(&sample());
        let mut forged = buf.clone();
        let raw_at = HEADER_BYTES + 64; // record 0 is the raw fragment
        forged[raw_at..raw_at + 4].copy_from_slice(&999u32.to_le_bytes());
        assert_eq!(
            decode(&forged, &side).unwrap_err(),
            DecodeError::UnknownId(999)
        );
    }

    #[test]
    fn a_foreign_or_truncated_buffer_is_rejected() {
        let (buf, side) = encode(&sample());
        assert_eq!(
            decode(&buf[..8], &side).unwrap_err(),
            DecodeError::Truncated
        );

        let mut wrong = buf.clone();
        wrong[0..4].copy_from_slice(&0xdead_beefu32.to_le_bytes());
        assert_eq!(decode(&wrong, &side).unwrap_err(), DecodeError::BadMagic);

        let mut future = buf.clone();
        future[4..8].copy_from_slice(&99u32.to_le_bytes());
        assert_eq!(
            decode(&future, &side).unwrap_err(),
            DecodeError::BadVersion(99)
        );
    }

    #[test]
    fn a_record_count_larger_than_the_buffer_is_rejected() {
        // The obvious hostile shape: claim a million records, send four.
        let (mut buf, side) = encode(&sample());
        buf[8..12].copy_from_slice(&1_000_000u32.to_le_bytes());
        assert_eq!(decode(&buf, &side).unwrap_err(), DecodeError::Truncated);
    }
}
