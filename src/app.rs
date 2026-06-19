//! Application layer — port-generic use-cases. Each function takes only the
//! ports it needs and returns domain/view values; `main.rs` does all printing.
//! No clap, no filesystem, no shell-outs here.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::cockpit::health::Health;
use crate::dag::{self, DagError};
use crate::flow::delivery::{self, DeliveryMode};
use crate::flow::facts::{DeliveryFacts, ReviewState};
use crate::flow::rail::render_rail;
use crate::flow::stage::derive_stage;
use crate::model::config::Config;
use crate::model::glossary::Glossary;
use crate::model::local::resolve_worktree_dir;
use crate::model::node::DagNode;
use crate::model::spec::SpecRecord;
use crate::ports::{
    CheckpointStore, DagRepo, DeliveryProbe, ForgePort, GitPort, SessionRepo, SettingsRepo,
    SpecRepo,
};
use crate::render::dag_board::{self, Board, BoardNode};
use crate::session::{SessionId, SessionRecord};

/// Outcome of `init`, so `main.rs` can print the right line.
pub enum InitOutcome {
    AlreadyInitialized,
    Initialized,
}

/// Fail fast if `.circuit/` was never initialized. (Port-level guard for
/// use-cases + tests; the CLI keeps its own path-aware guard for the message.)
pub fn require_initialized<S: SettingsRepo>(settings: &S) -> anyhow::Result<()> {
    if !settings.is_initialized() {
        anyhow::bail!("no .circuit/ workspace — run `circuit init` first");
    }
    Ok(())
}

/// Create a spec session record.
pub fn spec_new<S: SettingsRepo, R: SpecRepo>(
    settings: &S,
    specs: &R,
    id: &str,
    title: String,
    intent: String,
    contexts: Vec<String>,
) -> anyhow::Result<()> {
    require_initialized(settings)?;
    let mut spec = SpecRecord::new(id, title, intent);
    spec.bounded_contexts = contexts;
    specs
        .save_spec(&spec)
        .with_context(|| format!("writing spec {id}"))?;
    Ok(())
}

/// Initialize `.circuit/` settings. Returns whether it was already present.
/// The `.gitignore` side-effect and printing stay in the CLI edge.
pub fn init<S: SettingsRepo>(settings: &S) -> anyhow::Result<InitOutcome> {
    if settings.is_initialized() {
        return Ok(InitOutcome::AlreadyInitialized);
    }
    settings
        .save_config(&Config::default())
        .context("writing config.toml")?;
    settings
        .save_glossary(&Glossary::default())
        .context("writing glossary.toml")?;
    Ok(InitOutcome::Initialized)
}

#[allow(clippy::too_many_arguments)]
pub fn dag_add_node<S: SettingsRepo, R: DagRepo>(
    settings: &S,
    dag_repo: &R,
    id: &str,
    spec: String,
    title: String,
    branch: String,
    intent: String,
    depends_on: Vec<String>,
) -> anyhow::Result<()> {
    require_initialized(settings)?;
    let mut node = DagNode::new(id, spec, title, branch);
    node.intent = intent;
    node.depends_on = depends_on;
    dag_repo
        .save_dag_node(&node)
        .with_context(|| format!("writing dag node {id}"))?;
    Ok(())
}

pub fn dag_link<S: SettingsRepo, R: DagRepo>(
    settings: &S,
    dag_repo: &R,
    from: &str,
    to: &str,
) -> anyhow::Result<()> {
    require_initialized(settings)?;
    let mut node = dag_repo
        .load_dag_node(from)
        .with_context(|| format!("loading dag node {from}"))?;
    if !node.depends_on.contains(&to.to_string()) {
        node.depends_on.push(to.to_string());
    }
    dag_repo
        .save_dag_node(&node)
        .with_context(|| format!("writing dag node {from}"))?;
    Ok(())
}

/// Validate the whole DAG; returns the error list plus the node count.
pub fn dag_check<R: DagRepo>(dag_repo: &R) -> anyhow::Result<(Vec<DagError>, usize)> {
    let nodes = dag_repo.list_dag_nodes().context("reading dag nodes")?;
    let count = nodes.len();
    Ok((dag::validate(&nodes), count))
}

#[derive(Debug)]
pub struct SpawnOutcome {
    pub session_id: String,
    pub dag_node: String,
    pub branch: String,
    pub worktree: PathBuf,
}

#[allow(clippy::too_many_arguments)]
pub fn session_spawn<S, D, Se, G>(
    settings: &S,
    dag_repo: &D,
    sessions: &Se,
    git: &G,
    dag_node: &str,
    worktrees_env: Option<&str>,
    repo_root: &Path,
) -> anyhow::Result<SpawnOutcome>
where
    S: SettingsRepo,
    D: DagRepo,
    Se: SessionRepo,
    G: GitPort,
{
    require_initialized(settings)?;
    let node = dag_repo
        .load_dag_node(dag_node)
        .with_context(|| format!("loading dag node {dag_node}"))?;
    let config = settings.load_config().context("loading config.toml")?;
    let base = &config.base_branch;
    if git
        .branch_facts(&node.branch, base)
        .with_context(|| format!("checking branch {}", node.branch))?
        .exists
    {
        anyhow::bail!(
            "branch {} already exists — refusing to spawn over it",
            node.branch
        );
    }
    let id = SessionId::generate();
    let record = SessionRecord::impl_(id, node.spec.clone(), node.id.clone(), node.branch.clone());
    sessions
        .save_session(&record)
        .with_context(|| format!("writing session {id}"))?;
    let local = settings.load_local().context("loading local.toml")?;
    let worktree = resolve_worktree_dir(worktrees_env, &local, repo_root, &id.to_string());
    git.create_branch(&node.branch, base)
        .with_context(|| format!("creating branch {}", node.branch))?;
    git.add_worktree(&node.branch, &worktree)
        .with_context(|| format!("adding worktree at {}", worktree.display()))?;
    Ok(SpawnOutcome {
        session_id: id.to_string(),
        dag_node: node.id.clone(),
        branch: node.branch.clone(),
        worktree,
    })
}

/// Outcome of `session_archive`, so the CLI edge prints the right lines.
pub struct ArchiveOutcome {
    pub session_id: String,
    pub branch: Option<String>,
    pub already_archived: bool,
}

/// Archive (retire) a session: tear down its worktree (located by branch), then
/// optionally delete the branch, then flip the durable `archived` status. A
/// dirty worktree / un-merged branch is refused without `force`.
pub fn session_archive<S, Se, G>(
    settings: &S,
    sessions: &Se,
    git: &G,
    id: &str,
    delete_branch: bool,
    force: bool,
) -> anyhow::Result<ArchiveOutcome>
where
    S: SettingsRepo,
    Se: SessionRepo,
    G: GitPort,
{
    require_initialized(settings)?;
    let mut record = resolve_session(sessions, id)?;
    if record.is_archived() {
        return Ok(ArchiveOutcome {
            session_id: record.id.to_string(),
            branch: record.branch.clone(),
            already_archived: true,
        });
    }
    // 1. Tear down the worktree, located by branch (never stored).
    if let Some(branch) = &record.branch {
        let worktrees = git.list_worktrees().context("listing worktrees")?;
        if let Some(wt) = worktrees
            .into_iter()
            .find(|w| w.branch.as_deref() == Some(branch.as_str()))
        {
            git.remove_worktree(&wt.path, force).with_context(|| {
                format!(
                    "removing worktree at {} (pass --force to discard uncommitted \
                     changes or unlock — stop the agent first if it is still running)",
                    wt.path.display()
                )
            })?;
        }
    }
    // 2. Optionally delete the branch (un-merged requires --force).
    if delete_branch {
        if let Some(branch) = &record.branch {
            git.delete_branch(branch, force).with_context(|| {
                format!("deleting branch {branch} (pass --force to delete an un-merged branch)")
            })?;
        }
    }
    // 3. Flip the durable status signal.
    record.archive();
    sessions
        .save_session(&record)
        .with_context(|| format!("saving archived session {}", record.id))?;
    Ok(ArchiveOutcome {
        session_id: record.id.to_string(),
        branch: record.branch.clone(),
        already_archived: false,
    })
}

/// Outcome of `session_unarchive`.
pub struct UnarchiveOutcome {
    pub session_id: String,
    pub was_not_archived: bool,
    /// `Some(path)` when the worktree was rehydrated from a kept branch.
    pub rehydrated_worktree: Option<PathBuf>,
    /// `Some(branch)` when the branch was gone, so no worktree was recreated.
    pub branch_missing: Option<String>,
}

/// Restore an archived session: flip status to active (saved first, so a later
/// worktree failure leaves the session truthfully active but worktree-less),
/// then re-add the worktree from the kept branch.
pub fn session_unarchive<S, Se, G>(
    settings: &S,
    sessions: &Se,
    git: &G,
    id: &str,
    worktrees_env: Option<&str>,
    repo_root: &Path,
) -> anyhow::Result<UnarchiveOutcome>
where
    S: SettingsRepo,
    Se: SessionRepo,
    G: GitPort,
{
    require_initialized(settings)?;
    let mut record = resolve_session(sessions, id)?;
    if !record.is_archived() {
        return Ok(UnarchiveOutcome {
            session_id: record.id.to_string(),
            was_not_archived: true,
            rehydrated_worktree: None,
            branch_missing: None,
        });
    }
    record.unarchive();
    sessions
        .save_session(&record)
        .with_context(|| format!("saving restored session {}", record.id))?;

    let mut rehydrated_worktree = None;
    let mut branch_missing = None;
    if let Some(branch) = &record.branch {
        let base = settings
            .load_config()
            .context("loading config.toml")?
            .base_branch;
        let exists = git
            .branch_facts(branch, &base)
            .with_context(|| format!("checking branch {branch}"))?
            .exists;
        if exists {
            let local = settings.load_local().context("loading local.toml")?;
            let worktree =
                resolve_worktree_dir(worktrees_env, &local, repo_root, &record.id.to_string());
            git.add_worktree(branch, &worktree)
                .with_context(|| format!("re-adding worktree at {}", worktree.display()))?;
            rehydrated_worktree = Some(worktree);
        } else {
            branch_missing = Some(branch.clone());
        }
    }
    Ok(UnarchiveOutcome {
        session_id: record.id.to_string(),
        was_not_archived: false,
        rehydrated_worktree,
        branch_missing,
    })
}

/// PR body = node intent (when non-empty) + a provenance footer tying the PR
/// back to its spec + DAG node. The footer is always present. Pure.
fn compose_pr_body(node: &DagNode) -> String {
    let footer = format!(
        "---\n🔁 Circuit · spec `{}` · node `{}`",
        node.spec, node.id
    );
    if node.intent.trim().is_empty() {
        footer
    } else {
        format!("{}\n\n{}", node.intent.trim(), footer)
    }
}

/// Shared precondition gate for the forge write verbs: resolve the session,
/// require a branch, require Forge delivery mode, and load the base branch.
/// Returns `(record, branch, base)`. Runs before any forge call.
fn forge_preconditions<S, Se, P>(
    settings: &S,
    sessions: &Se,
    probe: &P,
    selector: &str,
) -> anyhow::Result<(SessionRecord, String, String)>
where
    S: SettingsRepo,
    Se: SessionRepo,
    P: DeliveryProbe,
{
    require_initialized(settings)?;
    let record = resolve_session(sessions, selector)?;
    let branch = record
        .branch
        .clone()
        .ok_or_else(|| anyhow::anyhow!("session {} has no branch — spawn it first", record.id))?;
    if delivery::resolve(probe.gh_available(), probe.has_github_remote()) != DeliveryMode::Forge {
        anyhow::bail!("PR actions require a GitHub forge; this repo uses local checkpoints");
    }
    let base = settings
        .load_config()
        .context("loading config.toml")?
        .base_branch;
    Ok((record, branch, base))
}

/// Outcome of `session_pr`.
#[derive(Debug)]
pub struct PrOutcome {
    pub session_id: SessionId,
    pub branch: String,
    pub base: String,
    pub title: String,
}

/// Open a PR for the session's branch. Title comes from the session's DAG node;
/// body from `compose_pr_body`. Refused unless mode is Forge, the session has a
/// branch and a DAG node, and no PR exists yet.
pub fn session_pr<S, Se, D, F, P>(
    settings: &S,
    sessions: &Se,
    dag: &D,
    forge: &F,
    probe: &P,
    selector: &str,
) -> anyhow::Result<PrOutcome>
where
    S: SettingsRepo,
    Se: SessionRepo,
    D: DagRepo,
    F: ForgePort,
    P: DeliveryProbe,
{
    let (record, branch, base) = forge_preconditions(settings, sessions, probe, selector)?;
    let node_id = record.dag_node.clone().ok_or_else(|| {
        anyhow::anyhow!(
            "session {} has no DAG node — cannot derive PR title/body",
            record.id
        )
    })?;
    let node = dag
        .load_dag_node(&node_id)
        .with_context(|| format!("loading DAG node {node_id}"))?;
    match forge
        .review_state(&branch)
        .with_context(|| format!("checking PR state for {branch}"))?
    {
        ReviewState::None => {}
        other => anyhow::bail!("a PR for {branch} already exists (state: {other:?})"),
    }
    forge
        .create_pr(&branch, &base, &node.title, &compose_pr_body(&node))
        .with_context(|| format!("opening PR for {branch}"))?;
    Ok(PrOutcome {
        session_id: record.id,
        branch,
        base,
        title: node.title,
    })
}

/// Resolve a selector: exact ULID, else a unique DAG-node-name match.
pub fn resolve_session<Se: SessionRepo>(
    sessions: &Se,
    selector: &str,
) -> anyhow::Result<SessionRecord> {
    if selector.parse::<SessionId>().is_ok() {
        if let Ok(s) = sessions.load_session(selector) {
            return Ok(s);
        }
    }
    let all = sessions.list_sessions().context("listing sessions")?;
    let mut matches: Vec<SessionRecord> = all
        .into_iter()
        .filter(|s| s.dag_node.as_deref() == Some(selector))
        .collect();
    match matches.len() {
        1 => Ok(matches.pop().unwrap()),
        0 => anyhow::bail!(
            "no session matches `{selector}` (not a known session id or DAG-node name)"
        ),
        n => anyhow::bail!(
            "`{selector}` matches {n} sessions — pass the session id (ULID) to disambiguate"
        ),
    }
}

/// Render the spec-level DAG board. Returns the text to print (no trailing newline).
pub fn board<S, D, Se, G>(
    settings: &S,
    dag_repo: &D,
    sessions_repo: &Se,
    git: &G,
    spec: &str,
) -> anyhow::Result<String>
where
    S: SettingsRepo,
    D: DagRepo,
    Se: SessionRepo,
    G: GitPort,
{
    require_initialized(settings)?;
    let base = settings
        .load_config()
        .context("reading config.toml")?
        .base_branch;
    let nodes: Vec<DagNode> = dag_repo
        .list_dag_nodes()
        .context("reading dag nodes")?
        .into_iter()
        .filter(|n| n.spec == spec)
        .collect();
    let sessions = sessions_repo.list_sessions().context("reading sessions")?;
    let mut board_nodes = Vec::new();
    for n in &nodes {
        let stage = match git.branch_facts(&n.branch, &base) {
            Ok(branch) => {
                let session = sessions
                    .iter()
                    .find(|s| s.dag_node.as_deref() == Some(n.id.as_str()))
                    .cloned()
                    .unwrap_or_else(|| {
                        SessionRecord::impl_(SessionId::generate(), &n.spec, &n.id, &n.branch)
                    });
                let facts = DeliveryFacts {
                    branch,
                    review: None,
                };
                Some(derive_stage(&session, &facts))
            }
            Err(_) => None,
        };
        let health = crate::cockpit::rollup::node_health(git, &n.branch);
        board_nodes.push(BoardNode {
            id: n.id.clone(),
            depends_on: n.depends_on.clone(),
            stage,
            health,
        });
    }
    let board = Board { nodes: board_nodes };
    let mut out = String::new();
    write!(out, "{}", dag_board::render(&board)).unwrap();
    out.push_str("\n--- nodes ---\n");
    let mut sorted: Vec<&BoardNode> = board.nodes.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let healths: Vec<_> = sorted.iter().map(|n| n.health).collect();
    for n in &sorted {
        writeln!(
            out,
            "  {}  {}  {}",
            n.id,
            dag_board::stage_cell(&n.stage),
            dag_board::glyph(n.health)
        )
        .unwrap();
    }
    let spec_health = crate::cockpit::health::rollup_children(&healths);
    let trace = crate::cockpit::rollup::traceability(git, &nodes, &base);
    let m = trace
        .merged
        .map(|count| count.to_string())
        .unwrap_or_else(|| "?".to_string());
    write!(out, "\nSpec health: {}\n", dag_board::glyph(spec_health)).unwrap();
    write!(out, "Tasks: {}/{} done", m, trace.total).unwrap();
    Ok(out)
}

/// Analyze a Rust source tree: architecture indicators + mermaid diagram.
/// Returns the full report text (no final trailing newline; `main.rs` adds one
/// via `println!`).
pub fn analyze(path: &std::path::Path) -> anyhow::Result<String> {
    let graph = crate::builder::build_graph(path)?;
    let cycles = crate::indicators::cycles::find_cycles(&graph);
    let violations = crate::indicators::dependency_rule::violations(&graph);
    let mut out = String::new();
    writeln!(
        out,
        "Architecture — No-cycles (ADP): {}",
        if cycles.is_empty() {
            "● SOUND".to_string()
        } else {
            format!("⛔ {} cyclic group(s)", cycles.len())
        }
    )
    .unwrap();
    for c in &cycles {
        writeln!(out, "  cycle: {}", c.join(" → ")).unwrap();
    }
    writeln!(
        out,
        "Architecture — Dependency rule: {}",
        if violations.is_empty() {
            "● SOUND".to_string()
        } else {
            format!("⛔ {} violation(s)", violations.len())
        }
    )
    .unwrap();
    for v in &violations {
        writeln!(
            out,
            "  {} ({:?}) → {} ({:?})  VIOLATION",
            v.from, v.from_layer, v.to, v.to_layer
        )
        .unwrap();
    }
    writeln!(out, "\n--- mermaid ---").unwrap();
    write!(
        out,
        "{}",
        crate::render::mermaid::render(&graph, &violations, &cycles)
    )
    .unwrap();
    Ok(out)
}

/// Render the flow rail for one session or all. Returns the text to print.
#[allow(clippy::too_many_arguments)]
pub fn flow<S, Se, G, F, C, P>(
    settings: &S,
    sessions: &Se,
    git: &G,
    forge: &F,
    checkpoints: &C,
    probe: &P,
    selector: Option<&str>,
    all: bool,
) -> anyhow::Result<String>
where
    S: SettingsRepo,
    Se: SessionRepo,
    G: GitPort,
    F: ForgePort,
    C: CheckpointStore,
    P: DeliveryProbe,
{
    let sessions_list = match selector {
        // An explicit selector always shows the named session, even archived.
        Some(sel) => vec![resolve_session(sessions, sel)?],
        None => {
            let mut listed = sessions.list_sessions().context("listing sessions")?;
            // Hide archived sessions by default; `all` includes them.
            if !all {
                listed.retain(|s| !s.is_archived());
            }
            listed
        }
    };
    if sessions_list.is_empty() {
        return Ok("No sessions yet.".to_string());
    }
    let config = settings.load_config().context("loading config.toml")?;
    let mode = delivery::resolve(probe.gh_available(), probe.has_github_remote());
    let mut blocks = Vec::new();
    for s in &sessions_list {
        let branch_facts = match &s.branch {
            Some(b) => git
                .branch_facts(b, &config.base_branch)
                .with_context(|| format!("deriving facts for {b}"))?,
            None => Default::default(),
        };
        let review = match (&s.branch, mode) {
            (Some(b), DeliveryMode::Forge) => forge.review_state(b).ok(),
            (Some(_), DeliveryMode::Local) => checkpoints.review_state(&s.id.to_string()).ok(),
            (None, _) => None,
        };
        let facts = DeliveryFacts {
            branch: branch_facts,
            review,
        };
        let view = derive_stage(s, &facts);
        let label = s.dag_node.clone().unwrap_or_else(|| s.id.to_string());
        blocks.push(render_rail(
            &label,
            s.kind,
            view,
            s.branch.as_deref(),
            &facts.branch,
            facts.review,
            Health::Unknown,
            s.is_archived(),
        ));
    }
    Ok(blocks.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::fakes::MemStore;
    use std::cell::RefCell;

    /// Records write-action arguments and returns a configurable review state.
    struct SpyForge {
        review: crate::flow::facts::ReviewState,
        review_fails: bool,
        action_fails: bool,
        created: RefCell<Vec<(String, String, String, String)>>,
        merged: RefCell<Vec<String>>,
        updated: RefCell<Vec<(String, String)>>,
    }
    impl SpyForge {
        fn with_review(review: crate::flow::facts::ReviewState) -> Self {
            SpyForge {
                review,
                review_fails: false,
                action_fails: false,
                created: RefCell::new(vec![]),
                merged: RefCell::new(vec![]),
                updated: RefCell::new(vec![]),
            }
        }
    }
    impl crate::ports::ForgePort for SpyForge {
        type Error = crate::app::fakes::FakeErr;
        fn review_state(&self, _b: &str) -> Result<crate::flow::facts::ReviewState, Self::Error> {
            if self.review_fails {
                Err(crate::app::fakes::FakeErr("forge unreachable".into()))
            } else {
                Ok(self.review)
            }
        }
        fn create_pr(&self, b: &str, base: &str, t: &str, body: &str) -> Result<(), Self::Error> {
            if self.action_fails {
                return Err(crate::app::fakes::FakeErr("gh create failed".into()));
            }
            self.created
                .borrow_mut()
                .push((b.into(), base.into(), t.into(), body.into()));
            Ok(())
        }
        fn merge(&self, b: &str) -> Result<(), Self::Error> {
            if self.action_fails {
                return Err(crate::app::fakes::FakeErr("gh merge failed".into()));
            }
            self.merged.borrow_mut().push(b.into());
            Ok(())
        }
        fn update_from_base(&self, b: &str, base: &str) -> Result<(), Self::Error> {
            if self.action_fails {
                return Err(crate::app::fakes::FakeErr("gh update failed".into()));
            }
            self.updated.borrow_mut().push((b.into(), base.into()));
            Ok(())
        }
    }

    fn forge_store_with_impl_session(node: &str, intent: &str) -> (MemStore, String) {
        let store = MemStore {
            initialized: true,
            ..Default::default()
        };
        let mut dag_node = crate::model::node::DagNode::new(
            node,
            "auth",
            "Add login flow",
            format!("impl/{node}"),
        );
        dag_node.intent = intent.to_string();
        store.nodes.borrow_mut().insert(node.into(), dag_node);
        let s = impl_session(node);
        let id = s.id.to_string();
        store.sessions.borrow_mut().insert(id.clone(), s);
        (store, id)
    }

    fn forge_probe() -> crate::app::fakes::FakeProbe {
        crate::app::fakes::FakeProbe {
            gh: true,
            remote: true,
        }
    }

    #[test]
    fn analyze_self_emits_report_with_mermaid() {
        let out = analyze(std::path::Path::new("src")).unwrap();
        assert!(out.contains("Architecture — Dependency rule:"));
        assert!(out.contains("--- mermaid ---"));
    }

    #[test]
    fn init_on_fresh_store_reports_initialized() {
        let store = MemStore::default();
        assert!(matches!(init(&store).unwrap(), InitOutcome::Initialized));
    }

    #[test]
    fn init_on_initialized_store_is_noop() {
        let store = MemStore {
            initialized: true,
            ..Default::default()
        };
        assert!(matches!(
            init(&store).unwrap(),
            InitOutcome::AlreadyInitialized
        ));
    }

    #[test]
    fn spec_new_requires_init() {
        let store = MemStore::default();
        let err =
            spec_new(&store, &store, "checkout", "C".into(), "pay".into(), vec![]).unwrap_err();
        assert!(err.to_string().contains("circuit init"));
    }

    #[test]
    fn spec_new_saves_spec_with_contexts() {
        let store = MemStore {
            initialized: true,
            ..Default::default()
        };
        spec_new(
            &store,
            &store,
            "checkout",
            "Checkout".into(),
            "Pay.".into(),
            vec!["billing".into()],
        )
        .unwrap();
        let saved = store.specs.borrow().get("checkout").cloned().unwrap();
        assert_eq!(saved.bounded_contexts, vec!["billing".to_string()]);
    }

    #[test]
    fn dag_add_node_saves_with_deps() {
        let store = MemStore {
            initialized: true,
            ..Default::default()
        };
        dag_add_node(
            &store,
            &store,
            "auth",
            "checkout".into(),
            "Auth".into(),
            "impl/auth".into(),
            "do auth".into(),
            vec!["base".into()],
        )
        .unwrap();
        let n = store.nodes.borrow().get("auth").cloned().unwrap();
        assert_eq!(n.branch, "impl/auth");
        assert_eq!(n.depends_on, vec!["base".to_string()]);
    }

    #[test]
    fn dag_link_appends_dependency_once() {
        let store = MemStore {
            initialized: true,
            ..Default::default()
        };
        dag_add_node(
            &store,
            &store,
            "a",
            "s".into(),
            "A".into(),
            "impl/a".into(),
            "".into(),
            vec![],
        )
        .unwrap();
        dag_link(&store, &store, "a", "b").unwrap();
        dag_link(&store, &store, "a", "b").unwrap();
        let n = store.nodes.borrow().get("a").cloned().unwrap();
        assert_eq!(n.depends_on, vec!["b".to_string()]);
    }

    #[test]
    fn dag_check_returns_validation_errors() {
        let store = MemStore {
            initialized: true,
            ..Default::default()
        };
        dag_add_node(
            &store,
            &store,
            "a",
            "s".into(),
            "A".into(),
            "impl/a".into(),
            "".into(),
            vec!["ghost".into()],
        )
        .unwrap();
        let (errs, count) = dag_check(&store).unwrap();
        assert_eq!(count, 1);
        assert!(!errs.is_empty());
    }

    struct ExistingBranchGit;
    impl crate::ports::GitPort for ExistingBranchGit {
        type Error = crate::app::fakes::FakeErr;
        fn branch_facts(
            &self,
            _b: &str,
            _base: &str,
        ) -> Result<crate::flow::facts::BranchFacts, Self::Error> {
            Ok(crate::flow::facts::BranchFacts {
                exists: true,
                ..Default::default()
            })
        }
        fn create_branch(&self, _b: &str, _base: &str) -> Result<(), Self::Error> {
            Ok(())
        }
        fn add_worktree(&self, _b: &str, _p: &std::path::Path) -> Result<(), Self::Error> {
            Ok(())
        }
        fn list_worktrees(&self) -> Result<Vec<crate::ports::Worktree>, Self::Error> {
            Ok(vec![])
        }
        fn remove_worktree(&self, _p: &std::path::Path, _f: bool) -> Result<(), Self::Error> {
            Ok(())
        }
        fn delete_branch(&self, _b: &str, _f: bool) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    struct NoopGit;
    impl crate::ports::GitPort for NoopGit {
        type Error = crate::app::fakes::FakeErr;
        fn branch_facts(
            &self,
            _b: &str,
            _base: &str,
        ) -> Result<crate::flow::facts::BranchFacts, Self::Error> {
            Ok(Default::default())
        }
        fn create_branch(&self, _b: &str, _base: &str) -> Result<(), Self::Error> {
            Ok(())
        }
        fn add_worktree(&self, _b: &str, _p: &std::path::Path) -> Result<(), Self::Error> {
            Ok(())
        }
        fn list_worktrees(&self) -> Result<Vec<crate::ports::Worktree>, Self::Error> {
            Ok(vec![])
        }
        fn remove_worktree(&self, _p: &std::path::Path, _f: bool) -> Result<(), Self::Error> {
            Ok(())
        }
        fn delete_branch(&self, _b: &str, _f: bool) -> Result<(), Self::Error> {
            Ok(())
        }
    }
    struct NoopForge;
    impl crate::ports::ForgePort for NoopForge {
        type Error = crate::app::fakes::FakeErr;
        fn review_state(&self, _b: &str) -> Result<crate::flow::facts::ReviewState, Self::Error> {
            Err(crate::app::fakes::FakeErr("no forge".into()))
        }
        fn create_pr(
            &self,
            _b: &str,
            _base: &str,
            _t: &str,
            _body: &str,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
        fn merge(&self, _b: &str) -> Result<(), Self::Error> {
            Ok(())
        }
        fn update_from_base(&self, _b: &str, _base: &str) -> Result<(), Self::Error> {
            Ok(())
        }
    }
    struct NoopCheckpoints;
    impl crate::ports::CheckpointStore for NoopCheckpoints {
        type Error = crate::app::fakes::FakeErr;
        fn review_state(&self, _s: &str) -> Result<crate::flow::facts::ReviewState, Self::Error> {
            Ok(crate::flow::facts::ReviewState::None)
        }
    }

    #[test]
    fn flow_empty_store_says_no_sessions() {
        let store = MemStore {
            initialized: true,
            ..Default::default()
        };
        let probe = crate::app::fakes::FakeProbe {
            gh: false,
            remote: false,
        };
        let out = flow(
            &store,
            &store,
            &NoopGit,
            &NoopForge,
            &NoopCheckpoints,
            &probe,
            None,
            false,
        )
        .unwrap();
        assert_eq!(out, "No sessions yet.");
    }

    fn impl_session(node: &str) -> SessionRecord {
        SessionRecord::impl_(SessionId::generate(), "spec", node, format!("impl/{node}"))
    }

    #[test]
    fn session_pr_happy_path_creates_pr_with_derived_args() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "Implements login.");
        let forge = SpyForge::with_review(ReviewState::None);
        let out = session_pr(&store, &store, &store, &forge, &forge_probe(), "auth-login").unwrap();
        assert_eq!(out.branch, "impl/auth-login");
        assert_eq!(out.base, "main");
        assert_eq!(out.title, "Add login flow");
        let created = forge.created.borrow();
        assert_eq!(created.len(), 1);
        let (b, base, title, body) = &created[0];
        assert_eq!(b, "impl/auth-login");
        assert_eq!(base, "main");
        assert_eq!(title, "Add login flow");
        assert!(body.starts_with("Implements login."));
        assert!(body.contains("node `auth-login`"));
    }

    #[test]
    fn session_pr_local_mode_is_refused() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "");
        let forge = SpyForge::with_review(ReviewState::None);
        let probe = crate::app::fakes::FakeProbe {
            gh: false,
            remote: false,
        };
        let err = session_pr(&store, &store, &store, &forge, &probe, "auth-login").unwrap_err();
        assert!(
            err.to_string().contains("require a GitHub forge"),
            "got: {err}"
        );
        assert!(forge.created.borrow().is_empty());
    }

    #[test]
    fn session_pr_no_branch_is_refused() {
        let (store, id) = forge_store_with_impl_session("auth-login", "");
        // Strip the branch off the stored session.
        {
            let mut sessions = store.sessions.borrow_mut();
            let s = sessions.get_mut(&id).unwrap();
            s.branch = None;
        }
        let forge = SpyForge::with_review(ReviewState::None);
        let err =
            session_pr(&store, &store, &store, &forge, &forge_probe(), "auth-login").unwrap_err();
        assert!(err.to_string().contains("no branch"), "got: {err}");
    }

    #[test]
    fn session_pr_existing_pr_is_refused() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "");
        let forge = SpyForge::with_review(ReviewState::Open);
        let err =
            session_pr(&store, &store, &store, &forge, &forge_probe(), "auth-login").unwrap_err();
        assert!(err.to_string().contains("already exists"), "got: {err}");
        assert!(forge.created.borrow().is_empty());
    }

    #[test]
    fn session_pr_forge_unreachable_propagates() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "");
        let mut forge = SpyForge::with_review(ReviewState::None);
        forge.review_fails = true;
        let err =
            session_pr(&store, &store, &store, &forge, &forge_probe(), "auth-login").unwrap_err();
        assert!(
            err.to_string().contains("forge unreachable") || err.to_string().contains("PR state")
        );
    }

    #[test]
    fn resolve_session_by_dag_node_name() {
        let store = MemStore {
            initialized: true,
            ..Default::default()
        };
        let s = impl_session("auth");
        store
            .sessions
            .borrow_mut()
            .insert(s.id.to_string(), s.clone());
        let got = resolve_session(&store, "auth").unwrap();
        assert_eq!(got.dag_node.as_deref(), Some("auth"));
    }

    #[test]
    fn resolve_session_unknown_errs() {
        let store = MemStore {
            initialized: true,
            ..Default::default()
        };
        assert!(resolve_session(&store, "nope").is_err());
    }

    #[test]
    fn board_empty_spec_has_health_and_tasks_lines() {
        let store = MemStore {
            initialized: true,
            ..Default::default()
        };
        // No nodes match the spec, so git is never consulted — use the fake
        // to keep the test free of any dependency on the local `.` being a repo.
        let out = board(&store, &store, &store, &NoopGit, "nonexistent-spec").unwrap();
        assert!(out.contains("Spec health"));
        assert!(out.contains("Tasks:"));
    }

    #[test]
    fn spawn_refuses_existing_branch() {
        let store = MemStore {
            initialized: true,
            ..Default::default()
        };
        store.nodes.borrow_mut().insert(
            "auth".into(),
            crate::model::node::DagNode::new(
                "auth",
                "checkout".to_string(),
                "Auth".to_string(),
                "impl/auth".to_string(),
            ),
        );
        let err = session_spawn(
            &store,
            &store,
            &store,
            &ExistingBranchGit,
            "auth",
            None,
            std::path::Path::new("/tmp/repo"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    fn node_with_intent(intent: &str) -> crate::model::node::DagNode {
        let mut n = crate::model::node::DagNode::new(
            "auth-login",
            "auth",
            "Add login flow",
            "impl/auth-login",
        );
        n.intent = intent.to_string();
        n
    }

    #[test]
    fn pr_body_includes_intent_then_footer() {
        let body = compose_pr_body(&node_with_intent("Implements OAuth2 login."));
        assert_eq!(
            body,
            "Implements OAuth2 login.\n\n---\n🔁 Circuit · spec `auth` · node `auth-login`"
        );
    }

    #[test]
    fn pr_body_empty_intent_is_footer_only() {
        let body = compose_pr_body(&node_with_intent("   "));
        assert_eq!(body, "---\n🔁 Circuit · spec `auth` · node `auth-login`");
    }

    #[test]
    fn pr_body_footer_always_carries_spec_and_node() {
        let body = compose_pr_body(&node_with_intent(""));
        assert!(body.contains("spec `auth`"));
        assert!(body.contains("node `auth-login`"));
    }
}

#[cfg(test)]
pub(crate) mod fakes {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use crate::model::config::Config;
    use crate::model::glossary::Glossary;
    use crate::model::local::LocalConfig;
    use crate::model::node::DagNode;
    use crate::model::spec::SpecRecord;
    use crate::ports::{DagRepo, DeliveryProbe, SessionRepo, SettingsRepo, SpecRepo};
    use crate::session::SessionRecord;

    #[derive(Debug)]
    pub struct FakeErr(pub String);
    impl std::fmt::Display for FakeErr {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }
    impl std::error::Error for FakeErr {}

    #[derive(Default)]
    pub struct MemStore {
        pub initialized: bool,
        pub config: Config,
        pub local: LocalConfig,
        pub glossary: Glossary,
        pub specs: RefCell<HashMap<String, SpecRecord>>,
        pub nodes: RefCell<HashMap<String, DagNode>>,
        pub sessions: RefCell<HashMap<String, SessionRecord>>,
    }

    impl SettingsRepo for MemStore {
        type Error = FakeErr;
        fn is_initialized(&self) -> bool {
            self.initialized
        }
        fn load_config(&self) -> Result<Config, FakeErr> {
            Ok(self.config.clone())
        }
        fn save_config(&self, _c: &Config) -> Result<(), FakeErr> {
            Ok(())
        }
        fn load_glossary(&self) -> Result<Glossary, FakeErr> {
            Ok(self.glossary.clone())
        }
        fn save_glossary(&self, _g: &Glossary) -> Result<(), FakeErr> {
            Ok(())
        }
        fn load_local(&self) -> Result<LocalConfig, FakeErr> {
            Ok(self.local.clone())
        }
    }
    impl SpecRepo for MemStore {
        type Error = FakeErr;
        fn load_spec(&self, id: &str) -> Result<SpecRecord, FakeErr> {
            self.specs
                .borrow()
                .get(id)
                .cloned()
                .ok_or_else(|| FakeErr(format!("no spec {id}")))
        }
        fn save_spec(&self, s: &SpecRecord) -> Result<(), FakeErr> {
            self.specs.borrow_mut().insert(s.id.clone(), s.clone());
            Ok(())
        }
    }
    impl DagRepo for MemStore {
        type Error = FakeErr;
        fn load_dag_node(&self, id: &str) -> Result<DagNode, FakeErr> {
            self.nodes
                .borrow()
                .get(id)
                .cloned()
                .ok_or_else(|| FakeErr(format!("no node {id}")))
        }
        fn save_dag_node(&self, n: &DagNode) -> Result<(), FakeErr> {
            self.nodes.borrow_mut().insert(n.id.clone(), n.clone());
            Ok(())
        }
        fn list_dag_nodes(&self) -> Result<Vec<DagNode>, FakeErr> {
            Ok(self.nodes.borrow().values().cloned().collect())
        }
    }
    impl SessionRepo for MemStore {
        type Error = FakeErr;
        fn load_session(&self, id: &str) -> Result<SessionRecord, FakeErr> {
            self.sessions
                .borrow()
                .get(id)
                .cloned()
                .ok_or_else(|| FakeErr(format!("no session {id}")))
        }
        fn save_session(&self, s: &SessionRecord) -> Result<(), FakeErr> {
            self.sessions
                .borrow_mut()
                .insert(s.id.to_string(), s.clone());
            Ok(())
        }
        fn list_sessions(&self) -> Result<Vec<SessionRecord>, FakeErr> {
            Ok(self.sessions.borrow().values().cloned().collect())
        }
    }

    pub struct FakeProbe {
        pub gh: bool,
        pub remote: bool,
    }
    impl DeliveryProbe for FakeProbe {
        fn gh_available(&self) -> bool {
            self.gh
        }
        fn has_github_remote(&self) -> bool {
            self.remote
        }
    }
}
