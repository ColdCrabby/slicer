//! The move IR — what the emitter produces before anything is rendered.
//!
//! The G-code generator used to append strings directly, which meant there was
//! never a moment at which the program existed as *moves*. A plugin could only
//! have edited the finished text, and post-processing text is explicitly not
//! how this engine extends: it sees no geometry, no settings, and no roles.
//!
//! So emission is split in three:
//!
//! ```text
//! plan  →  Vec<Move>  →  filters  →  render
//! ```
//!
//! The middle step is the point. A [`MoveFilter`] sees the whole program as
//! structured moves — with their role, width, feedrate and extrusion still
//! attached — and may rewrite it before a single character is produced. That
//! is what native arc welding needs, and what travel rewriting needs.
//!
//! ## Why `Raw` exists
//!
//! Only **motion** is modelled. Fan changes, temperature commands, markers and
//! comments are carried as [`Move::Raw`] — already-rendered text that the
//! renderer emits verbatim.
//!
//! That is a deliberate boundary, not an unfinished one. Modelling every
//! command would mean re-deciding, in typed form, every choice the emitter
//! already makes correctly, for no gain: a filter that wants to weld arcs or
//! reroute travel cares about motion and needs the rest only to stay in order.
//! Rendering `Raw` verbatim is also what makes the IR provably output-neutral
//! — the bytes for everything unmodelled cannot drift, because they are the
//! same bytes.

use crate::core::ExtrusionRole;
use crate::gcode::dialect::GcodeDialect;

/// One step of the program.
///
/// Positions are absolute machine coordinates in mm, feedrates mm/min. `e` is
/// the value to print, already resolved for the absolute/relative convention;
/// `de` is the signed incremental filament length that value stands for, which
/// is what a filter needs when it merges or splits moves.
#[derive(Debug, Clone, PartialEq)]
pub enum Move {
    /// Deposit material while moving.
    Extrude {
        /// Destination X.
        x: f64,
        /// Destination Y.
        y: f64,
        /// Destination Z, when the segment is non-planar. `None` keeps the
        /// current Z, which is the ordinary flat case.
        z: Option<f64>,
        /// The E value to print.
        e: f64,
        /// The incremental filament length `e` represents.
        de: f64,
        /// Feedrate in mm/min.
        feed_mm_min: f64,
        /// What this extrusion is for.
        role: ExtrusionRole,
        /// The bead width in mm this segment was charged at.
        width_mm: f64,
        /// Trailing comment, without the leading `;`.
        comment: Option<String>,
    },
    /// Move without depositing material.
    Travel {
        /// Destination X.
        x: f64,
        /// Destination Y.
        y: f64,
        /// Feedrate in mm/min.
        feed_mm_min: f64,
        /// Trailing comment, without the leading `;`.
        comment: Option<String>,
    },
    /// Move the Z axis alone.
    ZMove {
        /// Destination Z.
        z: f64,
        /// Feedrate in mm/min.
        feed_mm_min: f64,
        /// Trailing comment, without the leading `;`.
        comment: Option<String>,
    },
    /// Move the extruder alone — a retract or a prime.
    Extruder {
        /// The E value to print.
        e: f64,
        /// The incremental filament length `e` represents.
        de: f64,
        /// Feedrate in mm/min.
        feed_mm_min: f64,
        /// Trailing comment, without the leading `;`.
        comment: Option<String>,
    },
    /// Text the emitter already rendered — fan, temperature, markers,
    /// comments, custom scripts. Emitted verbatim.
    Raw(String),
}

impl Move {
    /// Tag an extrusion with the role and bead width it was charged at.
    ///
    /// The line helpers cannot know these — they are handed a length and a
    /// feedrate — but a filter reasoning about the program very much needs
    /// them: welding arcs across a wall and across an infill dash are not the
    /// same decision.
    pub fn with_role(mut self, path_role: crate::core::ExtrusionRole, width: f64) -> Self {
        if let Move::Extrude { role, width_mm, .. } = &mut self {
            *role = path_role;
            *width_mm = width;
        }
        self
    }

    /// Attach a trailing comment, replacing any it already had.
    pub fn with_comment(mut self, text: impl Into<String>) -> Self {
        let text = text.into();
        match &mut self {
            Move::Extrude { comment, .. }
            | Move::Travel { comment, .. }
            | Move::ZMove { comment, .. }
            | Move::Extruder { comment, .. } => *comment = Some(text),
            Move::Raw(_) => {}
        }
        self
    }

    /// Whether this move deposits material.
    pub fn is_extrusion(&self) -> bool {
        matches!(self, Move::Extrude { .. })
    }

    /// The destination XY, for the moves that have one.
    pub fn end_xy(&self) -> Option<(f64, f64)> {
        match self {
            Move::Extrude { x, y, .. } | Move::Travel { x, y, .. } => Some((*x, *y)),
            _ => None,
        }
    }

    /// The incremental filament length, for the moves that carry one.
    pub fn delta_e(&self) -> f64 {
        match self {
            Move::Extrude { de, .. } | Move::Extruder { de, .. } => *de,
            _ => 0.0,
        }
    }

    /// Render this move through `dialect`, including its trailing newline.
    pub fn render(&self, dialect: &dyn GcodeDialect) -> String {
        let with_comment = |line: String, comment: &Option<String>| match comment {
            Some(c) => format!("{line} ; {c}\n"),
            None => format!("{line}\n"),
        };
        match self {
            Move::Extrude {
                x,
                y,
                z,
                e,
                feed_mm_min,
                comment,
                ..
            } => {
                let line = match z {
                    Some(z) => dialect.move_extrude_z(*x, *y, *z, *e, *feed_mm_min),
                    None => dialect.move_extrude(*x, *y, *e, *feed_mm_min),
                };
                with_comment(line, comment)
            }
            Move::Travel {
                x,
                y,
                feed_mm_min,
                comment,
            } => with_comment(dialect.travel_xy(*x, *y, *feed_mm_min), comment),
            Move::ZMove {
                z,
                feed_mm_min,
                comment,
            } => with_comment(dialect.move_z(*z, *feed_mm_min), comment),
            Move::Extruder {
                e,
                feed_mm_min,
                comment,
                ..
            } => with_comment(dialect.set_extruder_pos(*e, *feed_mm_min), comment),
            Move::Raw(text) => text.clone(),
        }
    }
}

/// The program under construction: an ordered list of moves.
///
/// The emitter pushes into this instead of into a `String`, so there is a
/// moment — after planning, before rendering — at which the whole program
/// exists as moves and a filter can rewrite it.
#[derive(Debug, Default, Clone)]
pub struct MoveProgram {
    moves: Vec<Move>,
}

impl MoveProgram {
    /// An empty program.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append already-rendered text, emitted verbatim.
    ///
    /// Named to read like the `String::push_str` it replaced at the emitter's
    /// many small append sites.
    pub fn raw_str(&mut self, text: &str) {
        self.raw(text);
    }

    /// Append already-rendered text, emitted verbatim.
    pub fn raw(&mut self, text: impl Into<String>) {
        let text = text.into();
        // Consecutive raw fragments are merged so the emitter's many small
        // pushes do not become many small allocations in the move list.
        if let Some(Move::Raw(last)) = self.moves.last_mut() {
            last.push_str(&text);
            return;
        }
        self.moves.push(Move::Raw(text));
    }

    /// Append a move.
    pub fn push(&mut self, m: Move) {
        self.moves.push(m);
    }

    /// The moves, in order.
    pub fn moves(&self) -> &[Move] {
        &self.moves
    }

    /// The moves, mutably — what a filter rewrites.
    pub fn moves_mut(&mut self) -> &mut Vec<Move> {
        &mut self.moves
    }

    /// Number of moves.
    pub fn len(&self) -> usize {
        self.moves.len()
    }

    /// Whether the program is empty.
    pub fn is_empty(&self) -> bool {
        self.moves.is_empty()
    }

    /// Render the whole program to G-code text.
    pub fn render(&self, dialect: &dyn GcodeDialect) -> String {
        let mut out = String::with_capacity(64 * 1024);
        for m in &self.moves {
            out.push_str(&m.render(dialect));
        }
        out
    }
}

/// Something that rewrites the program before it is rendered.
///
/// The hook family that native arc welding needs. A filter sees every move in
/// order, with role, width, feedrate and extrusion intact, and may replace,
/// merge, split or drop them.
///
/// Implementations must be `Send + Sync`, like every other plugin hook.
pub trait MoveFilter: Send + Sync {
    /// A short name, used in log lines.
    fn name(&self) -> &str;

    /// Rewrite `program` in place.
    fn filter(&self, program: &mut MoveProgram, params: &crate::settings::params::SlicingParams);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialect() -> Box<dyn GcodeDialect> {
        Box::new(crate::gcode::dialects::MarlinDialect)
    }

    #[test]
    fn raw_text_renders_verbatim() {
        // The property that makes the IR provably output-neutral for every
        // command it does not model.
        let mut p = MoveProgram::new();
        p.raw(";TYPE:Outer wall\n");
        p.raw("M107\n");
        assert_eq!(p.render(dialect().as_ref()), ";TYPE:Outer wall\nM107\n");
    }

    #[test]
    fn consecutive_raw_fragments_merge() {
        let mut p = MoveProgram::new();
        p.raw("a");
        p.raw("b");
        assert_eq!(p.len(), 1, "one raw run, not two");
        assert_eq!(p.render(dialect().as_ref()), "ab");
    }

    #[test]
    fn a_comment_is_appended_the_way_the_emitter_wrote_it() {
        let mut p = MoveProgram::new();
        p.push(Move::Travel {
            x: 1.0,
            y: 2.0,
            feed_mm_min: 9000.0,
            comment: Some("travel".into()),
        });
        assert_eq!(
            p.render(dialect().as_ref()),
            "G1 X1.000 Y2.000 F9000 ; travel\n"
        );
    }

    #[test]
    fn a_non_planar_extrude_renders_with_z() {
        let mut p = MoveProgram::new();
        p.push(Move::Extrude {
            x: 1.0,
            y: 2.0,
            z: Some(0.35),
            e: 0.5,
            de: 0.5,
            feed_mm_min: 1800.0,
            role: ExtrusionRole::OuterWall,
            width_mm: 0.4,
            comment: None,
        });
        assert_eq!(
            p.render(dialect().as_ref()),
            "G1 X1.000 Y2.000 Z0.350 E0.50000 F1800\n"
        );
    }
}
