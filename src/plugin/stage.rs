//! The pipeline as an ordered list of named stages.
//!
//! A stage is one step of `process_mesh` — slicing, wall generation, infill —
//! reified as a value. That reification is the whole point: a plugin says
//! *where* its work goes by naming an existing stage, so the set of hook points
//! grows every time the engine grows a stage, with no change to this API.

use std::borrow::Cow;

use crate::logging::PhaseTimer;
use crate::plugin::context::SliceContext;

/// The name a stage is addressed by.
///
/// Core stage ids are the [`crate::logging::phases`] constants, so the name a
/// plugin targets and the name the phase timings report are the same string —
/// there is no second catalog to keep in sync.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StageId(Cow<'static, str>);

impl StageId {
    /// A stage id known at compile time.
    pub const fn new(id: &'static str) -> Self {
        Self(Cow::Borrowed(id))
    }

    /// A stage id computed at runtime — what an externally loaded plugin needs.
    pub fn owned(id: impl Into<String>) -> Self {
        Self(Cow::Owned(id.into()))
    }

    /// The id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for StageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&'static str> for StageId {
    fn from(id: &'static str) -> Self {
        Self::new(id)
    }
}

/// One step of the pipeline.
///
/// Implementations must be `Send + Sync`: rayon parallelises wall generation,
/// interior regions, surfaces and infill, so a stage may be entered from a
/// worker thread and may itself fan out.
pub trait Stage: Send + Sync {
    /// The name this stage is addressed by.
    fn id(&self) -> StageId;

    /// Do the work, reading and writing [`SliceContext`].
    fn run(&self, cx: &mut SliceContext<'_>);
}

/// A stage that runs *instead of* another one, and decides whether to call it.
///
/// This is how "wrap it" is expressed. A wrapper that ignores `inner` replaces
/// the stage outright; one that calls it brackets it. The wrapper takes over
/// the wrapped stage's id, so a second registration targeting that id still
/// finds it.
pub trait StageWrapper: Send + Sync {
    /// The wrapper's own name, used only in log lines.
    fn id(&self) -> StageId;

    /// Run in place of the stage named by the registration's target.
    fn run(&self, cx: &mut SliceContext<'_>, inner: &dyn Stage);
}

/// Where an inserted stage goes, relative to an existing one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// Immediately before the named stage.
    Before(StageId),
    /// Immediately after the named stage.
    After(StageId),
}

impl Placement {
    /// The stage this placement is relative to.
    pub fn target(&self) -> &StageId {
        match self {
            Self::Before(id) | Self::After(id) => id,
        }
    }
}

/// One plugin's request to change the stage order.
pub enum StageRegistration {
    /// Add a stage next to an existing one.
    Insert {
        /// Where the new stage goes.
        placement: Placement,
        /// The stage to add.
        stage: Box<dyn Stage>,
    },
    /// Take over an existing stage.
    Wrap {
        /// The stage being wrapped.
        target: StageId,
        /// What runs in its place.
        wrapper: Box<dyn StageWrapper>,
    },
}

impl StageRegistration {
    /// Insert `stage` immediately before `target`.
    pub fn before(target: impl Into<StageId>, stage: Box<dyn Stage>) -> Self {
        Self::Insert {
            placement: Placement::Before(target.into()),
            stage,
        }
    }

    /// Insert `stage` immediately after `target`.
    pub fn after(target: impl Into<StageId>, stage: Box<dyn Stage>) -> Self {
        Self::Insert {
            placement: Placement::After(target.into()),
            stage,
        }
    }

    /// Run `wrapper` in place of `target`.
    pub fn wrap(target: impl Into<StageId>, wrapper: Box<dyn StageWrapper>) -> Self {
        Self::Wrap {
            target: target.into(),
            wrapper,
        }
    }
}

/// A wrapped stage, presenting itself under the wrapped stage's id.
struct Wrapped {
    inner: Box<dyn Stage>,
    wrapper: Box<dyn StageWrapper>,
}

impl Stage for Wrapped {
    fn id(&self) -> StageId {
        self.inner.id()
    }

    fn run(&self, cx: &mut SliceContext<'_>) {
        self.wrapper.run(cx, self.inner.as_ref());
    }
}

/// A closure as a stage, for the many core steps that are a single call.
pub struct FnStage<F> {
    id: StageId,
    run: F,
}

impl<F> FnStage<F>
where
    F: Fn(&mut SliceContext<'_>) + Send + Sync,
{
    /// Name a closure as a stage.
    pub fn new(id: impl Into<StageId>, run: F) -> Self {
        Self { id: id.into(), run }
    }

    /// The same, boxed ready for a [`StageRegistry`].
    pub fn boxed(id: impl Into<StageId>, run: F) -> Box<dyn Stage>
    where
        F: 'static,
    {
        Box::new(Self::new(id, run))
    }
}

impl<F> Stage for FnStage<F>
where
    F: Fn(&mut SliceContext<'_>) + Send + Sync,
{
    fn id(&self) -> StageId {
        self.id.clone()
    }

    fn run(&self, cx: &mut SliceContext<'_>) {
        (self.run)(cx);
    }
}

/// The ordered stage list a run executes.
pub struct StageRegistry {
    stages: Vec<Box<dyn Stage>>,
}

impl StageRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self { stages: Vec::new() }
    }

    /// Append a stage to the end of the current order.
    pub fn push(&mut self, stage: Box<dyn Stage>) -> &mut Self {
        self.stages.push(stage);
        self
    }

    /// The stage ids in execution order.
    pub fn ids(&self) -> Vec<StageId> {
        self.stages.iter().map(|s| s.id()).collect()
    }

    /// Number of stages in the current order.
    pub fn len(&self) -> usize {
        self.stages.len()
    }

    /// Whether the registry holds no stages.
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    /// Position of `id` in the current order.
    fn index_of(&self, id: &StageId) -> Option<usize> {
        self.stages.iter().position(|s| &s.id() == id)
    }

    /// Fold one registration into the order.
    ///
    /// A registration naming a stage that does not exist is **rejected**, not
    /// silently appended: a plugin that targets a stage the engine has since
    /// renamed should say so, not quietly run at the wrong point in the
    /// pipeline.
    pub fn apply(&mut self, registration: StageRegistration) -> Result<(), StageError> {
        match registration {
            StageRegistration::Insert { placement, stage } => {
                let target = placement.target().clone();
                let at = self
                    .index_of(&target)
                    .ok_or_else(|| StageError::UnknownTarget(target.clone()))?;
                let at = match placement {
                    Placement::Before(_) => at,
                    Placement::After(_) => at + 1,
                };
                self.stages.insert(at, stage);
                Ok(())
            }
            StageRegistration::Wrap { target, wrapper } => {
                let at = self
                    .index_of(&target)
                    .ok_or_else(|| StageError::UnknownTarget(target.clone()))?;
                let inner = self.stages.remove(at);
                self.stages.insert(at, Box::new(Wrapped { inner, wrapper }));
                Ok(())
            }
        }
    }

    /// Run every stage in order, timing each one under its own id.
    ///
    /// Cancellation is checked after each stage rather than at two hand-picked
    /// points, so a cancelled run stops at the next stage boundary wherever it
    /// is. The layers built so far are left in `cx` either way.
    pub fn run(&self, cx: &mut SliceContext<'_>) {
        let logger = cx.logger;
        for stage in &self.stages {
            let id = stage.id();
            let timer = PhaseTimer::start(id.as_str(), logger);
            stage.run(cx);
            timer.finish();
            if logger.is_cancelled() {
                logger.log_info(&format!("slice cancelled after stage '{}'", id));
                return;
            }
        }
    }
}

impl Default for StageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// What can go wrong folding a registration into the stage order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageError {
    /// The registration named a stage that is not in the pipeline.
    UnknownTarget(StageId),
}

impl std::fmt::Display for StageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTarget(id) => {
                write!(f, "no pipeline stage named '{}'", id)
            }
        }
    }
}

impl std::error::Error for StageError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::NullLogger;
    use crate::mesh::types::Mesh;
    use crate::settings::params::SlicingParams;
    use std::sync::Mutex;

    /// A stage that appends its id to a shared trace.
    struct Trace {
        id: StageId,
    }

    #[derive(Default)]
    struct Log(Mutex<Vec<String>>);

    impl Stage for Trace {
        fn id(&self) -> StageId {
            self.id.clone()
        }
        fn run(&self, cx: &mut SliceContext<'_>) {
            if let Some(log) = cx.state.get_mut::<Log>() {
                log.0.lock().unwrap().push(self.id.to_string());
            }
        }
    }

    fn trace(id: &'static str) -> Box<dyn Stage> {
        Box::new(Trace {
            id: StageId::new(id),
        })
    }

    fn run_order(build: impl FnOnce(&mut StageRegistry)) -> Vec<String> {
        let mesh = Mesh::new();
        let params = SlicingParams::default();
        let logger = NullLogger;
        let mut cx = SliceContext::new(&mesh, &params, &logger);
        cx.state.insert(Log::default());

        let mut registry = StageRegistry::new();
        registry.push(trace("a")).push(trace("b")).push(trace("c"));
        build(&mut registry);
        registry.run(&mut cx);

        let log = cx.state.remove::<Log>().unwrap();
        let out = log.0.lock().unwrap().clone();
        out
    }

    #[test]
    fn core_order_runs_as_pushed() {
        assert_eq!(run_order(|_| {}), vec!["a", "b", "c"]);
    }

    #[test]
    fn insertions_land_on_the_named_side_of_their_target() {
        let order = run_order(|r| {
            r.apply(StageRegistration::before("b", trace("pre")))
                .unwrap();
            r.apply(StageRegistration::after("b", trace("post")))
                .unwrap();
        });
        assert_eq!(order, vec!["a", "pre", "b", "post", "c"]);
    }

    /// A wrapper that runs the wrapped stage between two markers.
    struct Bracket;

    impl StageWrapper for Bracket {
        fn id(&self) -> StageId {
            StageId::new("bracket")
        }
        fn run(&self, cx: &mut SliceContext<'_>, inner: &dyn Stage) {
            if let Some(log) = cx.state.get_mut::<Log>() {
                log.0.lock().unwrap().push("open".into());
            }
            inner.run(cx);
            if let Some(log) = cx.state.get_mut::<Log>() {
                log.0.lock().unwrap().push("close".into());
            }
        }
    }

    #[test]
    fn a_wrapper_brackets_the_stage_it_takes_over() {
        let order = run_order(|r| {
            r.apply(StageRegistration::wrap("b", Box::new(Bracket)))
                .unwrap();
        });
        assert_eq!(order, vec!["a", "open", "b", "close", "c"]);
    }

    #[test]
    fn a_wrapped_stage_keeps_its_id_for_later_registrations() {
        // Wrapping must not make a stage unaddressable — a second plugin
        // targeting the same stage would otherwise be rejected purely because
        // of the order the two were loaded in.
        let order = run_order(|r| {
            r.apply(StageRegistration::wrap("b", Box::new(Bracket)))
                .unwrap();
            r.apply(StageRegistration::after("b", trace("post")))
                .unwrap();
        });
        assert_eq!(order, vec!["a", "open", "b", "close", "post", "c"]);
    }

    #[test]
    fn an_unknown_target_is_rejected_not_appended() {
        let mut registry = StageRegistry::new();
        registry.push(trace("a"));
        let err = registry
            .apply(StageRegistration::after("nope", trace("x")))
            .unwrap_err();
        assert_eq!(err, StageError::UnknownTarget(StageId::new("nope")));
        assert_eq!(registry.len(), 1);
    }
}
