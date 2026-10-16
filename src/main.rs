//! `sdcpi`: strategy determination for BPMN+CPI processes.
//!
//!     sdcpi determine <instance> (--B a,b,... | --B-file F) [options]
//!     sdcpi info      <instance>
//!     sdcpi bound     <instance>
//!     sdcpi optima    <instance>
//!
//! An instance is a path to a YAML file.

mod anytime;
mod arena;
mod parse;
mod bound;
mod exact;
mod num;
mod pareto;
mod search;
mod svg;
mod text;
mod serve;
mod state;
mod tables;
mod to_prism;
mod tree;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use num::{Exact, Num};
use search::{Ablation, Answer, Config, Selection, Steal};
use tables::Store;
use tree::Tree;

fn main() {
    anytime::mark_origin();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("{}", USAGE);
        std::process::exit(2);
    }
    let code = match args[0].as_str() {
        "info" => cmd_info(&args[1..]),
        "optima" => cmd_optima(&args[1..]),
        "determine" => cmd_determine(&args[1..]),
        "pareto" => cmd_pareto(&args[1..]),
        "parse" => cmd_parse(&args[1..]),
        "to_prism" => cmd_to_prism(&args[1..]),
        "to_svg" => cmd_to_svg(&args[1..]),
        "to_text" => cmd_to_text(&args[1..]),
        "to_objective" => cmd_to_objective(&args[1..]),
        "serve" => serve::cmd_serve(&args[1..]),
        "test" => cmd_test(&args[1..]),
        "bound" => cmd_bound(&args[1..]),
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            0
        }
        other => {
            eprintln!("unknown command {other:?}\n{USAGE}");
            2
        }
    };
    std::process::exit(code);
}

const USAGE: &str = "\
sdcpi  on-the-fly strategy synthesis for BPMN+CPI processes

  sdcpi determine <instance> (--B a,b,... | --B-file F) [options]
  sdcpi pareto    <instance> [--precision E] [--obdd-library L] [options]
  sdcpi parse     (<process> | --file F) [--saved F] [--out F]
  sdcpi to_prism  <instance> [--encode-history true|false] [--out F]
  sdcpi to_svg    <instance> [--detail true|false] [--tooltips true|false] [--bars all|a,b,...]
                  [--clocks true|false] [--names true|false] [--out F]
  sdcpi to_text   <instance> [--regenerate MODE --impacts K --seed S] [--out F]
  sdcpi to_objective <instance> (--B a,b,... | --B-file F | --pareto true) [--out F]
  sdcpi info      <instance>
  sdcpi bound     <instance>
  sdcpi optima    <instance>
  sdcpi serve     [--port N]
  sdcpi test      [<suite>] [options...]

test runs scripts/<suite>/run.py beside the tree of this executable,
forwarding the options; without a suite it lists the suites, and the
options of a suite are its own --help. The library knows the contract
and nothing of the content.

the grammar of parse, written inline or in the file
  process ::= region (, region)* | region op region      op ::= || | ^ | ^[p]
  region  ::= task | ( process )
  task    ::= ( name , duration )  |  ( name , duration , { name: value, ... } )
  , sequence, || parallel, ^ a choice, ^[p] a nature node taking its left
  operand with probability p in (0,1); durations positive integers; the map
  names only the impacts that are strictly positive, {} and no map both
  meaning all zero; # comments to end of line. A region may be @saved, or
  @saved[shown], a process of the store of the page (--saved F, the file
  sdcpi serve writes) spliced in place, to any depth and never in a cycle. The instance file is written
  to standard output, identifiers in-order, impact_names in the order the
  names first appear, the vectors mapped by that order.

options of every command that reads an instance
  --maximize a,b,...    maximize these impacts, by name or index, the time
                        included: the frontier takes the greatest expected
                        values there and the least elsewhere, a budget on
                        them is a floor, and the engine minimizes their
                        negation
  --time 1              the expected time as the component zero of the cost,
                        in front of the impacts, which keep their order: a
                        fast forward pays its advance there, a task is
                        bounded by what is left of its duration and a
                        parallel node by the longer of its regions. A budget
                        given by name calls it `time`; the PRISM model gets
                        it as the reward impact0, paid t_step by every
                        fast forward

options for to_prism
  --encode-history B    true, the default, records in the state the branch
                        taken at every choice and every nature node, so a
                        memoryless (positional) scheduler of the model
                        checker ranges over the history-dependent policies
                        of the instance; false emits the plain model, whose
                        states forget closed decisions, right for every
                        single-component query. The model is written to
                        standard output, or to --out F

options for to_objective
  the budget of determine, --B or --B-file in either form, emitted as the
  question put to the model checker over the model of to_prism: one
  R{\"impactj\"}<=Bj [ F n<root>=-2 ] term per component, joined in one
  multi(...) property, every value kept exactly as written. The property is
  written to standard output, or to --out F. With --pareto true no budget is
  read and the property asks for the frontier instead, one
  R{\"impactj\"}min=? [ F n<root>=-2 ] term per component

options for determine
  --B a,b,...           the budget B, one value per component, or the named
                        form {name: value, ...}, rearranged against the
                        impact_names of the instance before starting
  --B-file F            read B from a yaml holding `B: [a, b, ...]` or
                        `B: {name: value, ...}`
  --workers N           number of workers (default 1)
  --ablation MODE       both | accept | reject | none      (default both)
  --selection MODE      weighted | uniform | oldest        (default weighted)
  --seed S              seed of the draw                   (default 20260816)
  --timeout SECS        give up after this many seconds
  --epsilon E           relative slack on the comparison with B (default 0)
  --steal MODE          ring | any                        (default ring)
  --print-size 1        on a positive answer print how large the partial
                        strategy is: the histories of the winning frontier, how
                        many of them are still open, and how many decisions it
                        takes. Neither the strategy nor its decisions are
                        printed
  --print-strategy 1    print the strategy at the end. Without it the decisions
                        are never recorded, which the search is faster for, and
                        only the outcome is reported
  --arith MODE          f64 | exact                       (default f64)
                        exact runs the search on rationals of arbitrary
                        precision, every comparison with B exact and the
                        accepting sum of the winning frontier printed as a
                        rational; --epsilon is ignored there

options for pareto
  the frontier of the achievable expected impacts, one strategy per point.
  The box [L, U] is discretised into ticks and explored with two OBDDs, an
  unknown point going to determine at every step; the search runs on exact
  rationals. Every option of determine but --B is forwarded.
  --extract MODE        tightest | loosest | random | draw (default tightest)
                        tightest takes the lexicographically largest tick
                        vector, a minimal unknown point, where an accept is
                        a point of the frontier itself; loosest the
                        smallest; random draws an unknown point by a walk
                        on the diagram, seeded by --seed, and loosens it to
                        a maximal unknown point; draw keeps the drawn point
                        as it is
  --certificate MODE    both | above | below | none      (default both)
                        which of the two certificates the loop reads.
                        above turns an accept into the value the winning
                        frontier pays, at most the budget asked, and
                        retires the cone above that value; without it the
                        accept says only yes and the budget asked is the
                        apex. below turns a refusal into the minimal
                        rejecting sums of the exhausted tree and retires
                        every budget outside the cones above them; without
                        it the refusal retires the cone below the budget
                        asked alone
  --precision E         the tick of the grid; without it the tick is 1/D
                        with D a common denominator of every achievable
                        expected value, and the frontier is exact
  --obdd-library L      biodivine | oxidd                 (default biodivine)
  --components i,j,...  the frontier over these components only, the others
                        left at their upper bound       (default all)
  --window lo..hi,...   the frontier among the strategies whose expected
                        values lie within these ranges, one per component
                        in order, either side empty for the bound at the
                        initial state; a partial strategy that cannot land
                        inside is left at once
  --tolerance f,...     one fraction per component, of the range of the
                        component between its two bounds at the start: a
                        point within these of a point found is left out,
                        and a partial strategy that cannot leave them is
                        closed; the frontier is then an approximation,
                        every strategy within the tolerances of a point
                        reported and no point reported within them of
                        another; one-phase search alone
  --time-limit SECS     stop the exploration after this many seconds in all
                        and report the frontier found so far as partial; a
                        query never runs past the time left
  --json F              write the report to F as JSON as well
  --anytime F           in one phase, append every apex to F as it is
                        recorded, one JSON object per line, with the second it
                        was recorded at counted from the start of the process,
                        flushed as it is written so that a run killed at any
                        moment leaves on disk what it had found; the last line
                        says the search ended, and its absence says the run was
                        killed
  --onephase 0|1        1, the default: the frontier in one phase, one
                        search of the whole tree, work stealing included,
                        against a shared diagram of the budgets known
                        feasible. A node whose lower bound is a budget known
                        feasible is closed, every completion being dominated
                        or equal; an upper bound not known feasible is
                        recorded with its partial strategy and the node goes
                        on. The readers run at once, a recording waits for
                        them. --extract and --certificate are not read.
                        0 rebuilds the frontier from threshold queries
  --oracle KIND         antichain | obdd                  (default antichain)
                        what holds the budgets known feasible in one phase:
                        the antichain of the apexes, one scan per test, or
                        the diagram of the union of their cones
  --print-certificate 1 print, at every refusal, the budget asked and the
                        vectors of its certificate
  --print-strategy 1    print the strategy behind every point, and the
                        decisions in the JSON report

options for optima
  --max-states N        give up past this many choice states (default 2000000)
";

// ---------------------------------------------------------------------------
// arguments
// ---------------------------------------------------------------------------

pub(crate) struct Args {
    positional: Vec<String>,
    flags: Vec<(String, String)>,
}

/// Every option some command takes: an option outside this list is a
/// mistake, refused with its name, never ignored in silence.
const KNOWN_OPTIONS: &[&str] = &["B", "B-file", "ablation", "anytime", "arith", "bars", "bench", "certificate", "clocks", "components", "detail", "encode-history", "epsilon", "extract", "file", "impacts", "json", "max-states", "maximize", "names", "no-exact", "obdd-library", "onephase", "oracle", "out", "pareto", "port", "precision", "print-certificate", "print-size", "print-strategy", "regenerate", "saved", "seed", "selection", "steal", "time", "time-limit", "timeout", "tolerance", "tooltips", "window", "workers"];

pub(crate) fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut out = Args {
        positional: Vec::new(),
        flags: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(name) = a.strip_prefix("--") {
            let (n, v) = if let Some((n, v)) = name.split_once('=') {
                i += 1;
                (n.to_string(), v.to_string())
            } else {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| format!("--{name} wants a value"))?;
                i += 2;
                (name.to_string(), v.clone())
            };
            if !KNOWN_OPTIONS.contains(&n.as_str()) {
                return Err(format!("unknown option --{n}; see `sdcpi help`"));
            }
            if out.flags.iter().any(|(m, _)| *m == n) {
                return Err(format!("--{n} is given twice"));
            }
            out.flags.push((n, v));
        } else {
            out.positional.push(a.clone());
            i += 1;
        }
    }
    Ok(out)
}

impl Args {
    pub(crate) fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

fn load(args: &Args) -> Result<(PathBuf, Tree), String> {
    let spec = args
        .positional
        .first()
        .ok_or_else(|| "no instance given".to_string())?;
    if args.positional.len() > 1 {
        return Err(format!("unexpected argument {:?} after the instance", args.positional[1]));
    }
    let path = PathBuf::from(spec);
    let tree = Tree::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    // the expected time as the component zero, in front of the impacts,
    // for every command that reads an instance
    let tree = if args.flag("time").is_some() { tree.with_time() } else { tree };
    // the components maximized, by name or index: their columns negated
    let tree = match args.flag("maximize") {
        Some(m) => {
            let names: Vec<String> = m.split(',').map(|t| t.trim().to_string()).collect();
            tree.with_directions(&names)?
        }
        None => tree,
    };
    Ok((path, tree))
}

/// One component of the budget: the value, and the literal it was written
/// as, so that what goes into an emitted objective is what the writer typed
/// and never a reformatting of it.
struct BVal {
    text: String,
    value: f64,
}

/// A budget as given, before the instance fixes the order of its components:
/// either the array itself, or a map from impact names to values that is
/// rearranged against the `impact_names` of the instance.
enum BSpec {
    List(Vec<BVal>),
    Map(Vec<(String, BVal)>),
}

/// The budget from one piece of text: `a, b, ...` or `[a, b, ...]` is the
/// array, `{name: value, ...}` is the named form.
fn parse_bspec(text: &str) -> Result<BSpec, String> {
    let t = text.trim();
    if let Some(inner) = t.strip_prefix('{') {
        let inner = inner
            .strip_suffix('}')
            .ok_or_else(|| "B opens with '{' and never closes it".to_string())?;
        let mut pairs: Vec<(String, BVal)> = Vec::new();
        for part in inner.split(',') {
            let (name, val) = part
                .split_once(':')
                .ok_or_else(|| format!("`{}` in B is not `name: value`", part.trim()))?;
            let v = val
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("`{}` in B is not a number", val.trim()))?;
            pairs.push((
                name.trim().to_string(),
                BVal { text: val.trim().to_string(), value: v },
            ));
        }
        if pairs.is_empty() {
            return Err("B is an empty map".to_string());
        }
        Ok(BSpec::Map(pairs))
    } else {
        let inner = match t.strip_prefix('[') {
            Some(r) => r
                .strip_suffix(']')
                .ok_or_else(|| "B opens with '[' and never closes it".to_string())?,
            None => t,
        };
        Ok(BSpec::List(vector(inner)?))
    }
}

/// The threshold from a YAML file holding one key, `B: [a, b, ...]` or
/// `B: {name: value, ...}`, on one line. Other keys are ignored, so a file
/// that also records where the numbers came from stays readable here.
fn bound_file(path: &str) -> Result<BSpec, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('#') {
            continue;
        }
        let rest = match t.strip_prefix("B:") {
            Some(r) => r.trim(),
            None => continue,
        };
        return parse_bspec(rest).map_err(|e| format!("{path}: {e}"));
    }
    Err(format!("{path}: no `B:` key in it"))
}

/// The budget in the order of the instance. An array must carry one value per
/// component already; a map is rearranged against `impact_names`, and it must
/// name every impact of the instance exactly once.
fn resolve_b(spec: BSpec, names: &[String], k: usize) -> Result<Vec<BVal>, String> {
    match spec {
        BSpec::List(v) if v.len() == k => Ok(v),
        BSpec::List(v) => Err(format!(
            "B has {} components against {} declared",
            v.len(),
            k
        )),
        BSpec::Map(pairs) => {
            if names.len() != k {
                return Err(
                    "the instance carries no impact_names, so give B as an array".to_string()
                );
            }
            let mut out: Vec<Option<BVal>> = (0..k).map(|_| None).collect();
            for (name, v) in pairs {
                let i = names.iter().position(|n| n == &name).ok_or_else(|| {
                    format!(
                        "B names `{name}`, and the instance declares [{}]",
                        names.join(", ")
                    )
                })?;
                if out[i].is_some() {
                    return Err(format!("B names `{name}` twice"));
                }
                out[i] = Some(v);
            }
            let missing: Vec<&str> = names
                .iter()
                .zip(&out)
                .filter(|(_, v)| v.is_none())
                .map(|(n, _)| n.as_str())
                .collect();
            if !missing.is_empty() {
                return Err(format!("B leaves [{}] without a value", missing.join(", ")));
            }
            Ok(out.into_iter().map(|v| v.unwrap()).collect())
        }
    }
}

/// The budget as the flags give it, `--B` inline or `--B-file` from a file.
fn budget_spec(args: &Args) -> Result<BSpec, String> {
    if let Some(p) = args.flag("B-file") {
        bound_file(p)
    } else if let Some(t) = args.flag("B") {
        parse_bspec(t)
    } else {
        Err("give --B or --B-file".to_string())
    }
}

fn vector(text: &str) -> Result<Vec<BVal>, String> {
    text.split(',')
        .map(|p| {
            let t = p.trim();
            t.parse::<f64>()
                .map(|value| BVal { text: t.to_string(), value })
                .map_err(|_| format!("{p:?} is not a number"))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// commands
// ---------------------------------------------------------------------------
pub(crate) fn fail(message: &str) -> i32 {
    eprintln!("sdcpi: {message}");
    2
}

fn fmt_vec(v: &[f64]) -> String {
    let parts: Vec<String> = v.iter().map(|x| format!("{x:.6}")).collect();
    format!("[{}]", parts.join(", "))
}

fn fmt_exact<N: Num>(v: &[N]) -> String {
    let parts: Vec<String> = v.iter().map(|x| x.describe()).collect();
    format!("[{}]", parts.join(", "))
}


fn cmd_info(args: &[String]) -> i32 {
    let args = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let (path, tree) = match load(&args) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let m = &tree.meta;
    println!("instance      {}", m.key);
    println!("path          {}", path.display());
    println!("grid          nested {} independent {} number {}", m.nested, m.independent, m.process_number);
    println!("dimensions    {}", tree.k);
    println!("mode          {}", m.mode);
    println!("tasks         {}", tree.n_tasks);
    println!("nodes         {}", tree.n_nodes);
    println!("xor nodes     {}", tree.xors.len());
    let (choices, natures) = tree.xors.iter().fold((0, 0), |(c, n), &id| {
        if tree.kind(id) == tree::Kind::Choice {
            (c + 1, n)
        } else {
            (c, n + 1)
        }
    });
    println!("  choice      {choices}");
    println!("  nature      {natures}");
    // the header of a campaign file carries it; a file written by parse
    // does not, and the tree says it
    let max_duration = if m.max_duration > 0 {
        m.max_duration
    } else {
        tree.tasks.iter().map(|&t| tree.node(t).duration as u32).max().unwrap_or(0)
    };
    println!("max duration  {max_duration}");
    let total = tree.undirected().total_impact();
    println!("total impact  {}", fmt_vec(&total));
    if tree.maximized.iter().any(|&m| m) {
        println!("maximized     [{}]", tree.maximized_names().join(", "));
    }
    0
}
fn cmd_parse(args: &[String]) -> i32 {
    let args = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let text = match (args.flag("file"), args.positional.first()) {
        (Some(f), None) => match std::fs::read_to_string(f) {
            Ok(t) => t,
            Err(e) => return fail(&format!("{f}: {e}")),
        },
        (None, Some(_)) => args.positional.join(" "),
        _ => return fail("give the process inline or through --file, not both and not neither"),
    };
    // a reference @saved is resolved against a store of the page, --saved F
    let parsed = match args.flag("saved") {
        None => parse::to_yaml(&text),
        Some(f) if !std::path::Path::new(f).is_file() => return fail(&format!("{f}: no such file")),
        Some(f) => match serve::read_store(std::path::Path::new(f)) {
            Err(e) => return fail(&e),
            Ok(entries) => {
                let mut resolve = |name: &str| -> Result<String, String> {
                    entries
                        .iter()
                        .find(|(n, _)| n == name)
                        .map(|(_, t)| t.clone())
                        .ok_or_else(|| format!("no process is saved as {name:?} in {f}"))
                };
                parse::to_yaml_with(&text, &mut resolve)
            }
        },
    };
    match parsed {
        Ok(p) => match args.flag("out") {
            Some(path) => match std::fs::write(path, &p.yaml) {
                Ok(()) => 0,
                Err(e) => fail(&format!("{path}: {e}")),
            },
            None => {
                // writing, not printing: a consumer that stops early, as
                // `head` does, closes the pipe, and that is not a panic
                use std::io::Write;
                let _ = std::io::stdout().write_all(p.yaml.as_bytes());
                0
            }
        },
        Err(e) => fail(&e),
    }
}


fn cmd_optima(args: &[String]) -> i32 {
    let args = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let (_, tree) = match load(&args) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let records = tables::Records::<f64>::new();
    let store = Store::new(&tree, &records, 1);
    let max_states = args
        .flag("max-states")
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000_000usize);
    let started = Instant::now();
    match exact::optima(&store, max_states) {
        Ok(o) => {
            let secs = started.elapsed().as_secs_f64();
            println!("instance      {}", tree.meta.key);
            println!("choice states {}", store.cs_made.load(std::sync::atomic::Ordering::Relaxed));
            println!("seconds       {secs:.4}");
            for c in 0..tree.k {
                // shortest form that reads back as the same double. Ten
                // decimal places would keep four significant digits of a
                // component worth 2e-7, and a reader that pads such a number to
                // make it a bound no policy can fail would be padding under its
                // own rounding
                let (lo, hi) = if tree.maximized[c] { (-o.max[c], -o.min[c]) } else { (o.min[c], o.max[c]) };
                println!("component {c}  min {:e}  max {:e}", lo, hi);
            }
            0
        }
        Err(e) => fail(&format!("{e:?}")),
    }
}
/// The process of an instance as text in the grammar of parse, the impacts
/// drawn again by a mode of the benchmark when asked.
fn cmd_to_text(args: &[String]) -> i32 {
    let args = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let (_, tree) = match load(&args) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    // the instance as it was written, whatever direction a flag gave it
    let tree = tree.undirected();
    let fresh = match args.flag("regenerate") {
        None => None,
        Some(mode) => {
            let dim = args
                .flag("impacts")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(tree.k);
            let seed = args
                .flag("seed")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(20260815);
            match text::regenerate(&tree, dim, mode, seed) {
                Ok(f) => Some(f),
                Err(e) => return fail(&e),
            }
        }
    };
    let out = text::to_text(&tree, fresh.as_deref());
    match args.flag("out") {
        Some(p) => match std::fs::write(p, &out) {
            Ok(()) => 0,
            Err(e) => fail(&format!("{p}: {e}")),
        },
        None => {
            use std::io::Write;
            let _ = std::io::stdout().write_all(out.as_bytes());
            0
        }
    }
}

/// The process as a BPMN diagram, in SVG, to standard output or to --out F;
/// --detail true wraps every region in a dashed box with its identifier.
fn cmd_to_svg(args: &[String]) -> i32 {
    let args = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let (_, tree) = match load(&args) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    // the instance as it was written, whatever direction a flag gave it
    let tree = tree.undirected();
    let detailed = match args.flag("detail") {
        None | Some("false") => false,
        Some("true") => true,
        Some(v) => return fail(&format!("--detail wants true or false, found {v:?}")),
    };
    let tooltips = match args.flag("tooltips") {
        None | Some("false") => false,
        Some("true") => true,
        Some(v) => return fail(&format!("--tooltips wants true or false, found {v:?}")),
    };
    // the impacts as bars: all of them, or the ones named, in alphabetical
    // order either way
    let bars = match args.flag("bars") {
        None => None,
        Some("all") => Some(svg::alphabetical(&tree)),
        Some(list) => {
            let names = svg::impact_names(&tree);
            let wanted: Vec<&str> = list.split(',').map(str::trim).collect();
            if let Some(w) = wanted.iter().find(|w| !names.iter().any(|n| n == *w)) {
                return fail(&format!("--bars names {w:?}, and the instance has [{}]", names.join(", ")));
            }
            Some(
                svg::alphabetical(&tree)
                    .into_iter()
                    .filter(|&j| wanted.contains(&names[j].as_str()))
                    .collect(),
            )
        }
    };
    let clocks = match args.flag("clocks") {
        None | Some("false") => false,
        Some("true") => true,
        Some(v) => return fail(&format!("--clocks wants true or false, found {v:?}")),
    };
    let names = match args.flag("names") {
        None | Some("true") => true,
        Some("false") => false,
        Some(v) => return fail(&format!("--names wants true or false, found {v:?}")),
    };
    let text = svg::render(&tree, &svg::Style { detailed, tooltips, bars, clocks, names, ..Default::default() });
    match args.flag("out") {
        Some(p) => match std::fs::write(p, &text) {
            Ok(()) => 0,
            Err(e) => fail(&format!("{p}: {e}")),
        },
        None => {
            use std::io::Write;
            let _ = std::io::stdout().write_all(text.as_bytes());
            0
        }
    }
}

fn cmd_to_objective(args: &[String]) -> i32 {
    let args = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let (_, tree) = match load(&args) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let pareto = match args.flag("pareto") {
        None | Some("false") => false,
        Some("true") => true,
        Some(v) => return fail(&format!("--pareto wants true or false, found {v:?}")),
    };
    let parts: Vec<String> = if pareto {
        // The frontier of every expected impact, the question the model
        // checker answers with points and no strategy.
        (0..tree.k)
            .map(|j| format!("R{{\"impact{j}\"}}{}=? [ F n{}=-2 ]", if tree.maximized[j] { "max" } else { "min" }, tree.root))
            .collect()
    } else {
        let spec = match budget_spec(&args) {
            Ok(s) => s,
            Err(e) => return fail(&e),
        };
        let b = match resolve_b(spec, &tree.meta.impact_names, tree.k) {
            Ok(v) => v,
            Err(e) => return fail(&e),
        };
        b.iter()
            .enumerate()
            .map(|(j, v)| format!("R{{\"impact{j}\"}}{}{} [ F n{}=-2 ]", if tree.maximized[j] { ">=" } else { "<=" }, v.text, tree.root))
            .collect()
    };
    let prop = format!("multi({})\n", parts.join(", "));
    match args.flag("out") {
        Some(p) => match std::fs::write(p, &prop) {
            Ok(()) => 0,
            Err(e) => fail(&format!("{p}: {e}")),
        },
        None => {
            use std::io::Write;
            let _ = std::io::stdout().write_all(prop.as_bytes());
            0
        }
    }
}

/// A suite is a folder under scripts/ with a run.py; this dispatcher knows
/// that contract and nothing of the content, so the library stays free of
/// what the suites measure.
fn cmd_test(args: &[String]) -> i32 {
    let root = std::env::current_exe()
        .ok()
        .and_then(|e| e.ancestors().nth(3).map(|p| p.to_path_buf()));
    let scripts = match root {
        Some(r) if r.join("scripts").is_dir() => r.join("scripts"),
        _ => PathBuf::from("scripts"),
    };
    let Some((suite, rest)) = args.split_first() else {
        let mut found = false;
        if let Ok(entries) = std::fs::read_dir(&scripts) {
            for e in entries.flatten() {
                if e.path().join("run.py").exists() {
                    println!("{}", e.file_name().to_string_lossy());
                    found = true;
                }
            }
        }
        if !found {
            eprintln!("no suite under {}", scripts.display());
            return 2;
        }
        return 0;
    };
    if suite.starts_with('-') {
        return fail("the suite comes first: sdcpi test <suite> [options...]");
    }
    let script = scripts.join(suite).join("run.py");
    if !script.exists() {
        return fail(&format!("{} does not exist", script.display()));
    }
    match std::process::Command::new("python3").arg(&script).args(rest).status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => fail(&format!("python3: {e}")),
    }
}

fn cmd_to_prism(args: &[String]) -> i32 {
    let args = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let (path, tree) = match load(&args) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let history = match args.flag("encode-history") {
        None | Some("true") => true,
        Some("false") => false,
        Some(v) => return fail(&format!("--encode-history wants true or false, found {v:?}")),
    };
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let text = to_prism::emit(&tree, &stem, &file_name, history);
    match args.flag("out") {
        Some(p) => match std::fs::write(p, &text) {
            Ok(()) => 0,
            Err(e) => fail(&format!("{p}: {e}")),
        },
        None => {
            use std::io::Write;
            let _ = std::io::stdout().write_all(text.as_bytes());
            0
        }
    }
}

/// The options of the search as the flags give them, shared by determine and
/// by pareto, which runs one search per query.
pub(crate) struct SearchOptions {
    steal: Steal,
    epsilon: f64,
    workers: usize,
    ablation: Ablation,
    selection: Selection,
    seed: u64,
    timeout: Option<Duration>,
}

impl SearchOptions {
    pub(crate) fn from_args(args: &Args) -> Result<SearchOptions, String> {
        Ok(SearchOptions {
            steal: match args.flag("steal").map(Steal::parse) {
                None => Steal::Ring,
                Some(Some(v)) => v,
                Some(None) => return Err("--steal wants ring or any".to_string()),
            },
            epsilon: args
                .flag("epsilon")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0),
            workers: match args.flag("workers") {
                None => 1,
                Some(v) => match v.parse::<usize>() {
                    Ok(w) if (1..=256).contains(&w) => w,
                    _ => return Err(format!("--workers wants a whole number from 1 to 256, found {v:?}")),
                },
            },
            ablation: match args.flag("ablation").map(Ablation::parse) {
                None => Ablation::Both,
                Some(Some(a)) => a,
                Some(None) => {
                    return Err("--ablation wants both, accept, reject or none".to_string())
                }
            },
            selection: match args.flag("selection").map(Selection::parse) {
                None => Selection::Weighted,
                Some(Some(s)) => s,
                Some(None) => {
                    return Err("--selection wants weighted, uniform or oldest".to_string())
                }
            },
            seed: args
                .flag("seed")
                .and_then(|v| v.parse().ok())
                .unwrap_or(20260816),
            timeout: args
                .flag("timeout")
                .and_then(|v| v.parse::<f64>().ok())
                .map(Duration::from_secs_f64),
        })
    }

    pub(crate) fn config<N: Num>(&self, threshold: Vec<N>, record_strategy: bool) -> Config<N> {
        Config {
            threshold,
            oracle: None,
            record_strategy,
            certificate: false,
            window: None,
            cancel: None,
            publish_open: false,
            tolerance: Vec::new(),
            steal: self.steal,
            epsilon: self.epsilon,
            workers: self.workers,
            ablation: self.ablation,
            selection: self.selection,
            seed: self.seed,
            timeout: self.timeout,
        }
    }
}

fn cmd_determine(args: &[String]) -> i32 {
    let args = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let (_path, tree) = match load(&args) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };

    let spec = match budget_spec(&args) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    let budget: Vec<BVal> = match resolve_b(spec, &tree.meta.impact_names, tree.k) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let opts = match SearchOptions::from_args(&args) {
        Ok(o) => o,
        Err(e) => return fail(&e),
    };
    let print_strategy = args.flag("print-strategy").is_some();
    let print_size = args.flag("print-size").is_some();
    match args.flag("arith") {
        None | Some("f64") => {
            let threshold: Vec<f64> = budget.iter().map(|b| b.value).collect();
            run_determine(&tree, threshold, &opts, print_size, print_strategy, false)
        }
        Some("exact") => {
            let mut threshold: Vec<Exact> = Vec::with_capacity(budget.len());
            for b in &budget {
                match Exact::from_decimal(&b.text) {
                    Some(v) => threshold.push(v),
                    None => return fail(&format!("`{}` in B is not a decimal", b.text)),
                }
            }
            run_determine(&tree, threshold, &opts, print_size, print_strategy, true)
        }
        Some(v) => fail(&format!("--arith wants f64 or exact, found {v:?}")),
    }
}

fn run_determine<N: Num>(
    tree: &Tree,
    threshold: Vec<N>,
    opts: &SearchOptions,
    print_size: bool,
    print_strategy: bool,
    exact: bool,
) -> i32 {
    // recording costs nothing measurable, 1.7367 s against 1.7363 s on
    // an instance that searches for over a second, and without it the size
    // of the partial strategy cannot be told
    // a budget on a maximized component is a floor: the engine reads it on
    // the negation
    let shown_b = threshold.clone();
    let threshold: Vec<N> = threshold
        .into_iter()
        .enumerate()
        .map(|(c, v)| if tree.maximized.get(c).copied().unwrap_or(false) { N::zero().sub(&v) } else { v })
        .collect();
    let cfg = opts.config(threshold.clone(), print_strategy || print_size);
    let (answer, stats) = search::search(tree, cfg);

    let approx: Vec<f64> = shown_b.iter().map(|b| b.to_f64()).collect();
    println!("instance       {}", tree.meta.key);
    println!("B              {}", fmt_vec(&approx));
    if exact {
        println!("B exact        {}", fmt_exact(&shown_b));
    }
    if tree.maximized.iter().any(|&m| m) {
        println!("maximized      [{}]", tree.maximized_names().join(", "));
    }
    println!(
        "answer         {}",
        match &answer {
            Answer::Yes(_) => "yes",
            Answer::No(_) => "no",
            Answer::Timeout(_) => "timeout",
            Answer::Failed(_) => "failed",
        }
    );
    if let Answer::Failed(e) = &answer {
        println!("reason         {e}");
    }
    println!("seconds        {:.6}", stats.elapsed.as_secs_f64());
    println!("expanded       {}", stats.expanded);
    println!("choice states  {}", stats.choice_states);
    println!("histories      {}", stats.histories);
    println!("macro steps    {}", stats.macro_steps);
    println!("step outcomes  {}", stats.outcomes);
    println!("peak open      {}", stats.peak_open);
    // The outcome on its own line, so that a caller reads one number and not
    // prose. A run that timed out or failed has no answer, and saying 0 there
    // would be saying that no strategy exists, which is not what happened.
    println!(
        "outcome        {}",
        match &answer {
            Answer::Yes(_) => "1 strategy found",
            Answer::No(_) => "0 strategy not found",
            Answer::Timeout(_) => "- timed out, no answer",
            Answer::Failed(_) => "- failed, no answer",
        }
    );
    if let Answer::Yes(s) = &answer {
        if exact {
            let shown: Vec<N> = s
                .accepting
                .iter()
                .enumerate()
                .map(|(c, v)| if tree.maximized.get(c).copied().unwrap_or(false) { N::zero().sub(v) } else { v.clone() })
                .collect();
            println!("accepting      {}", fmt_exact(&shown));
        }
        if print_size || print_strategy {
            // How large the partial strategy is, which is the third thing a
            // caller may want after the outcome and the strategy itself. The
            // histories of the winning frontier are the ones the strategy has
            // to prescribe for, the final ones included; the open ones are the
            // ones it leaves to a bound rather than to a decision, and there
            // are none of those once the accepting test is ablated away.
            println!("histories won  {}", s.histories);
            println!("open won       {}", s.frontier_size);
            println!("decisions won  {}", s.decisions.len());
        }
        if print_strategy {
            println!("decisions      {}", s.decisions.len());
            for (dh, action) in &s.decisions {
                println!("  after {dh:?} take {action}");
            }
        }
    }
    match answer {
        Answer::Failed(_) => 1,
        _ => 0,
    }
}

/// The frontier of the achievable expected impacts, with a strategy per point.
fn cmd_pareto(args: &[String]) -> i32 {
    let args = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let (_path, tree) = match load(&args) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let opts = match SearchOptions::from_args(&args) {
        Ok(o) => o,
        Err(e) => return fail(&e),
    };
    let precision = match args.flag("precision") {
        None => None,
        Some(t) => match num::parse_decimal(t) {
            Some(r) => Some(r),
            None => return fail(&format!("--precision wants a decimal, found {t:?}")),
        },
    };
    let library = match args.flag("obdd-library").map(pareto::Library::parse) {
        None => pareto::Library::Biodivine,
        Some(Some(l)) => l,
        Some(None) => return fail("--obdd-library wants biodivine or oxidd"),
    };
    let components = match args.flag("components") {
        None => None,
        Some(t) => {
            let mut out = Vec::new();
            for part in t.trim_matches(|c| c == '[' || c == ']').split(',') {
                match part.trim().parse::<usize>() {
                    Ok(i) => out.push(i),
                    Err(_) => return fail(&format!("--components wants indices, found {t:?}")),
                }
            }
            Some(out)
        }
    };
    // the window on the expected values: per component `lo..hi`, either
    // side empty, separated by commas, in the order of the components
    let window = match args.flag("window") {
        None => None,
        Some(t) => {
            let mut out = Vec::new();
            for part in t.split(',') {
                let (lo, hi) = match part.trim().split_once("..") {
                    Some(p) => p,
                    None => return fail(&format!("--window wants lo..hi per component, found {part:?}")),
                };
                let side = |s: &str| -> Result<Option<num_rational::BigRational>, String> {
                    if s.trim().is_empty() {
                        Ok(None)
                    } else {
                        num::parse_rational(s.trim()).map(Some).ok_or_else(|| format!("{s:?} is not a decimal"))
                    }
                };
                match (side(lo), side(hi)) {
                    (Ok(a), Ok(b)) => out.push((a, b)),
                    (Err(e), _) | (_, Err(e)) => return fail(&format!("--window: {e}")),
                }
            }
            if out.len() != tree.k {
                return fail(&format!(
                    "--window has {} ranges, the instance has {} components{}",
                    out.len(),
                    tree.k,
                    if tree.time { ", the time among them" } else { "" }
                ));
            }
            Some(out)
        }
    };
    // the tolerances, a fraction of the range of every component, in the
    // order of the components, the time first when it is in
    let tolerance: Option<Vec<num_rational::BigRational>> = match args.flag("tolerance") {
        None => None,
        Some(t) => {
            let mut out = Vec::new();
            for part in t.split(',') {
                let f = match num::parse_rational(part.trim()) {
                    Some(f) if f >= num_rational::BigRational::from_integer(0.into()) && f <= num_rational::BigRational::from_integer(1.into()) => f,
                    _ => return fail(&format!("--tolerance wants fractions between 0 and 1, found {part:?}")),
                };
                out.push(f);
            }
            if out.len() != tree.k {
                return fail(&format!(
                    "--tolerance has {} fractions, the instance has {} components{}",
                    out.len(),
                    tree.k,
                    if tree.time { ", the time among them" } else { "" }
                ));
            }
            Some(out)
        }
    };
    let extract = match args.flag("extract").map(pareto::Extract::parse) {
        None => pareto::Extract::Tightest,
        Some(Some(e)) => e,
        Some(None) => return fail("--extract wants random, draw, loosest or tightest"),
    };
    let certificates = match args.flag("certificate").map(pareto::Certificates::parse) {
        None => pareto::Certificates::Both,
        Some(Some(c)) => c,
        Some(None) => return fail("--certificate wants both, above, below or none"),
    };
    let time_limit = match args.flag("time-limit") {
        None => None,
        Some(t) => match t.parse::<f64>() {
            Ok(v) if v > 0.0 => Some(Duration::from_secs_f64(v)),
            _ => return fail(&format!("--time-limit wants a positive number of seconds, found {t:?}")),
        },
    };
    // One phase is the default since 2026-10-16; --onephase 0 asks for the
    // reconstruction from threshold queries.
    let onephase = args.flag("onephase").map_or(true, |v| v != "0");
    let anytime = match args.flag("anytime") {
        None => None,
        Some(_) if !onephase => {
            return fail("--anytime wants --onephase 1: in two phases the apexes are not recorded one by one")
        }
        Some(path) => match anytime::Sink::create(std::path::Path::new(path), tree.maximized.clone()) {
            Ok(sink) => Some(std::sync::Arc::new(sink)),
            Err(e) => return fail(&e),
        },
    };
    let print_strategy = args.flag("print-strategy").is_some();
    let started = Instant::now();
    let report = match pareto::frontier(
        &tree,
        pareto::Options {
            onephase,
            oracle: match args.flag("oracle").map(pareto::OracleKind::parse) {
                None => pareto::OracleKind::Antichain,
                Some(Some(k)) => k,
                Some(None) => return fail("--oracle wants antichain or obdd"),
            },
            extract,
            certificates,
            print_certificate: args.flag("print-certificate").is_some(),
            time_limit,
            anytime,
            precision,
            components,
            window: directed_window(&tree, window),
            tolerance,
            live: None,
            library,
            search: Config {
                certificate: certificates.below(),
                ..opts.config::<Exact>(Vec::new(), true)
            },
        },
    ) {
        Ok(r) => r,
        Err(e) => return fail(&e),
    };
    let elapsed = started.elapsed();

    let rat = |v: &[num_rational::BigRational]| -> String {
        let parts: Vec<String> = v.iter().map(|r| r.to_string()).collect();
        format!("[{}]", parts.join(", "))
    };
    let dec = |v: &[num_rational::BigRational]| -> String {
        let parts: Vec<String> = v.iter().map(|r| format!("{:.6}", pareto::approx(r))).collect();
        format!("[{}]", parts.join(", "))
    };
    println!("instance       {}", tree.meta.key);
    println!("library        {}", report.library);
    println!("mode           {}", if report.onephase { "one phase" } else { "two phases" });
    if report.onephase {
        println!(
            "oracle         {}: {} membership tests, {} nodes closed on them, {} apexes recorded, size {}",
            report.oracle.name(), report.oracle_reads, report.oracle_pruned, report.accepts, report.max_size
        );
    }
    println!("extract        {}", report.extract.name());
    println!("ablation       {}", report.ablation.name());
    println!("certificates   {}", report.certificates.name());
    match &report.denominator {
        Some(d) => {
            println!("tick           1/D exact");
            println!("D              {d}");
        }
        None => println!("tick           {} ({:.6e})", report.epsilon, pareto::approx(&report.epsilon)),
    }
    let comps: Vec<String> = report.components.iter().map(|c| c.to_string()).collect();
    println!("components     [{}]", comps.join(", "));
    if tree.maximized.iter().any(|&m| m) {
        println!("maximized      [{}]", tree.maximized_names().join(", "));
    }
    if let Some(w) = &report.window {
        let parts: Vec<String> = w
            .iter()
            .enumerate()
            .map(|(c, (a, b))| {
                let (a, b) = if tree.maximized.get(c).copied().unwrap_or(false) { (-b.clone(), -a.clone()) } else { (a.clone(), b.clone()) };
                format!("{:.6}..{:.6}", pareto::approx(&a), pareto::approx(&b))
            })
            .collect();
        println!("window         [{}], {} points found outside it", parts.join(", "), report.outside);
    }
    if let (Some(f), Some(a)) = (&report.tolerance_fraction, &report.tolerance) {
        let fr: Vec<String> = f.iter().map(|x| format!("{:.4}", pareto::approx(x))).collect();
        let ab: Vec<String> = a.iter().map(|x| format!("{:.6}", pareto::approx(x))).collect();
        println!(
            "tolerance      [{}] of the range, within [{}] of a point found; every strategy is within them of a point reported",
            fr.join(", "),
            ab.join(", ")
        );
    }
    let (shown_u, shown_l) = tree.shown_bounds(&report.upper, &report.lower);
    println!("U at s0        {}", dec(&shown_u));
    println!("L at s0        {}", dec(&shown_l));
    let ticks: Vec<String> = report.ticks.iter().map(|t| t.to_string()).collect();
    println!("ticks          [{}]", ticks.join(", "));
    let bits: Vec<String> = report.bits.iter().map(|b| b.to_string()).collect();
    println!(
        "bits           [{}] total {}",
        bits.join(", "),
        report.bits.iter().sum::<usize>()
    );
    println!(
        "status         {}",
        match &report.status {
            pareto::Status::Complete => "complete".to_string(),
            pareto::Status::Timeout => "timeout, frontier partial".to_string(),
            pareto::Status::Failed(e) => format!("failed, {e}"),
        }
    );
    println!("queries        {}", report.queries);
    println!("accepts        {}", report.accepts);
    println!("fails          {}", report.fails);
    println!("witness runs   {}", report.witness_queries);
    println!(
        "certified      {} refusals carried {} vectors, {} cones built and {} reused",
        report.certified, report.cert_vectors, report.cone_built, report.cone_hits
    );
    println!("seconds        {:.6}", elapsed.as_secs_f64());
    println!("search seconds {:.6}", report.search_time.as_secs_f64());
    println!("obdd seconds   {:.6}", report.obdd_time.as_secs_f64());
    println!(
        "  extract      {:.6}   cones {:.6}   merge {:.6}",
        report.extract_time.as_secs_f64(),
        report.cone_time.as_secs_f64(),
        report.merge_time.as_secs_f64()
    );
    println!("obdd peak size {}", report.max_size);
    println!("expanded       {}", report.search.expanded);
    println!("points         {}", report.points.len());
    if matches!(report.status, pareto::Status::Timeout) {
        println!(
            "certified      {} of {} points are final, {} open lower bounds at the cut",
            report.certified_points,
            report.points.len(),
            report.open_bounds
        );
    }
    for (i, p) in report.points.iter().enumerate() {
        let shown = tree.shown_vec(&p.value);
        println!("point {:<9}{}", i + 1, dec(&shown));
        println!("  exact        {}", rat(&shown));
        println!(
            "  origin       {}",
            match p.origin {
                pareto::Origin::Upper => "U".to_string(),
                pareto::Origin::Axis(c) => format!("axis {c}"),
                pareto::Origin::Query(n) => format!("query {n}"),
            }
        );
        if matches!(report.status, pareto::Status::Timeout) {
            println!("  final        {}", if p.certified { "yes" } else { "no, provisional" });
        }
        match &p.witness {
            Some(w) => {
                println!(
                    "  witness      {} decisions, {} histories, {} open",
                    w.decisions.len(),
                    w.histories,
                    w.open
                );
                if print_strategy {
                    for (dh, action) in &w.decisions {
                        println!("    after {dh:?} take {action}");
                    }
                }
            }
            None => println!("  witness      none"),
        }
    }
    if let Some(path) = args.flag("json") {
        let text = json_report(&tree, &report, elapsed, print_strategy);
        if let Err(e) = std::fs::write(path, text) {
            return fail(&format!("{path}: {e}"));
        }
    }
    match report.status {
        pareto::Status::Failed(_) => 1,
        _ => 0,
    }
}

/// The report of pareto as JSON, written by hand: the crate carries no
/// serialiser and the shape is small. Rationals are strings, so that nothing
/// is rounded on the way out.
fn json_report(tree: &Tree, r: &pareto::Report, elapsed: Duration, decisions: bool) -> String {
    fn esc(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }
    fn strs<T: ToString>(v: &[T]) -> String {
        let parts: Vec<String> = v.iter().map(|x| esc(&x.to_string())).collect();
        format!("[{}]", parts.join(", "))
    }
    fn nums<T: std::fmt::Display>(v: &[T]) -> String {
        let parts: Vec<String> = v.iter().map(|x| x.to_string()).collect();
        format!("[{}]", parts.join(", "))
    }
    let mut o = String::new();
    o.push_str("{\n");
    o.push_str(&format!("  \"instance\": {},\n", esc(&tree.meta.key)));
    o.push_str(&format!("  \"library\": {},\n", esc(r.library)));
    o.push_str(&format!("  \"onephase\": {},\n", r.onephase));
    o.push_str(&format!("  \"oracle\": {},\n", esc(r.oracle.name())));
    o.push_str(&format!("  \"oracle_reads\": {},\n", r.oracle_reads));
    o.push_str(&format!("  \"oracle_pruned\": {},\n", r.oracle_pruned));
    o.push_str(&format!("  \"extract\": {},\n", esc(r.extract.name())));
    o.push_str(&format!("  \"ablation\": {},\n", esc(r.ablation.name())));
    o.push_str(&format!("  \"certificates\": {},\n", esc(r.certificates.name())));
    o.push_str(&format!("  \"tick\": {},\n", esc(&r.epsilon.to_string())));
    o.push_str(&format!(
        "  \"denominator\": {},\n",
        match &r.denominator {
            Some(d) => esc(&d.to_string()),
            None => "null".to_string(),
        }
    ));
    o.push_str(&format!("  \"components\": {},\n", nums(&r.components)));
    let (shown_u, shown_l) = tree.shown_bounds(&r.upper, &r.lower);
    o.push_str(&format!("  \"maximized\": {},\n", strs(&tree.maximized_names())));
    match (&r.tolerance_fraction, &r.tolerance) {
        (Some(f), Some(a)) => {
            o.push_str(&format!("  \"tolerance_fraction\": {},\n", strs(f)));
            o.push_str(&format!("  \"tolerance\": {},\n", strs(a)));
        }
        _ => o.push_str("  \"tolerance\": null,\n"),
    }
    o.push_str(&format!("  \"upper\": {},\n", strs(&shown_u)));
    o.push_str(&format!("  \"lower\": {},\n", strs(&shown_l)));
    o.push_str(&format!("  \"ticks\": {},\n", strs(&r.ticks)));
    o.push_str(&format!("  \"bits\": {},\n", nums(&r.bits)));
    o.push_str(&format!(
        "  \"status\": {},\n",
        esc(match &r.status {
            pareto::Status::Complete => "complete",
            pareto::Status::Timeout => "timeout",
            pareto::Status::Failed(_) => "failed",
        })
    ));
    if let pareto::Status::Failed(e) = &r.status {
        o.push_str(&format!("  \"reason\": {},\n", esc(e)));
    }
    o.push_str(&format!("  \"queries\": {},\n", r.queries));
    o.push_str(&format!("  \"accepts\": {},\n", r.accepts));
    o.push_str(&format!("  \"fails\": {},\n", r.fails));
    o.push_str(&format!("  \"witness_runs\": {},\n", r.witness_queries));
    o.push_str(&format!("  \"open_bounds\": {},\n", r.open_bounds));
    o.push_str(&format!("  \"certified_points\": {},\n", r.certified_points));
    o.push_str(&format!("  \"certified_refusals\": {},\n", r.certified));
    o.push_str(&format!("  \"certificate_vectors\": {},\n", r.cert_vectors));
    o.push_str(&format!("  \"seconds\": {:.6},\n", elapsed.as_secs_f64()));
    o.push_str(&format!("  \"search_seconds\": {:.6},\n", r.search_time.as_secs_f64()));
    o.push_str(&format!("  \"obdd_seconds\": {:.6},\n", r.obdd_time.as_secs_f64()));
    o.push_str(&format!("  \"extract_seconds\": {:.6},\n", r.extract_time.as_secs_f64()));
    o.push_str(&format!("  \"cone_seconds\": {:.6},\n", r.cone_time.as_secs_f64()));
    o.push_str(&format!("  \"merge_seconds\": {:.6},\n", r.merge_time.as_secs_f64()));
    o.push_str(&format!("  \"obdd_peak_size\": {},\n", r.max_size));
    o.push_str(&format!("  \"expanded\": {},\n", r.search.expanded));
    o.push_str(&format!("  \"choice_states\": {},\n", r.search.choice_states));
    o.push_str(&format!("  \"histories\": {},\n", r.search.histories));
    o.push_str(&format!("  \"peak_open\": {},\n", r.search.peak_open));
    o.push_str("  \"points\": [\n");
    for (i, p) in r.points.iter().enumerate() {
        let approx: Vec<String> = tree.shown_vec(&p.value).iter().map(|v| format!("{:.9}", pareto::approx(v))).collect();
        o.push_str("    {\n");
        o.push_str(&format!("      \"value\": {},\n", strs(&tree.shown_vec(&p.value))));
        o.push_str(&format!("      \"certified\": {},\n", p.certified));
        o.push_str(&format!("      \"approx\": [{}],\n", approx.join(", ")));
        o.push_str(&format!(
            "      \"origin\": {},\n",
            esc(&match p.origin {
                pareto::Origin::Upper => "U".to_string(),
                pareto::Origin::Axis(c) => format!("axis {c}"),
                pareto::Origin::Query(n) => format!("query {n}"),
            })
        ));
        match &p.witness {
            Some(w) => {
                o.push_str(&format!("      \"decisions\": {},\n", w.decisions.len()));
                o.push_str(&format!("      \"histories\": {},\n", w.histories));
                o.push_str(&format!("      \"open\": {}", w.open));
                if decisions {
                    let items: Vec<String> = w
                        .decisions
                        .iter()
                        .map(|(dh, a)| format!("[{}, {a}]", nums(dh)))
                        .collect();
                    o.push_str(&format!(",\n      \"strategy\": [{}]", items.join(", ")));
                }
                o.push('\n');
            }
            None => o.push_str("      \"decisions\": null\n"),
        }
        o.push_str(if i + 1 < r.points.len() { "    },\n" } else { "    }\n" });
    }
    o.push_str("  ]\n}\n");
    o
}

/// The two bounds at the initial state, and the two sums on the frontier the
/// root of the computation tree carries, beside the exact optima. Everything the
/// accept and the reject test compare with `B`, printed rather than trusted.
fn cmd_bound(args: &[String]) -> i32 {
    let args = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let (_, tree) = match load(&args) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let records = tables::Records::<f64>::new();
    let store = Store::new(&tree, &records, 1);
    let engine_state = store.engine.initial_state();
    let b = bound::bounds::<f64>(&tree, &engine_state);
    println!("instance   {}", tree.meta.key);
    let (shown_u, shown_l) = tree.shown_bounds_f64(&b.upper, &b.lower);
    println!("U at s0    {}", fmt_vec(&shown_u));
    println!("L at s0    {}", fmt_vec(&shown_l));
    if tree.maximized.iter().any(|&m| m) {
        println!("maximized  [{}]", tree.maximized_names().join(", "));
    }
    let initial = match store.initial_histories() {
        Ok(v) => v,
        Err(e) => return fail(&format!("{e:?}")),
    };
    let k = tree.k;
    let (mut e, mut l) = (vec![0.0; k], vec![0.0; k]);
    for h in &initial {
        for c in 0..k {
            e[c] += h.e_hat[c];
            l[c] += h.l_hat[c];
        }
    }
    let (e, l) = tree.shown_bounds_f64(&e, &l);
    println!("root E     {}", fmt_vec(&e));
    println!("root L     {}", fmt_vec(&l));
    println!("root size  {} histories", initial.len());
    if args.flag("no-exact").is_none() {
        match exact::optima(&store, 2_000_000usize) {
            Ok(o) => {
                let (shown_max, shown_min) = tree.shown_bounds_f64(&o.max, &o.min);
                println!("exact min  {}", fmt_vec(&shown_min));
                println!("exact max  {}", fmt_vec(&shown_max));
            }
            Err(e) => println!("exact      unavailable: {e:?}"),
        }
        // the enclosure over every reachable choice state, printed as the
        // worst breach: a positive number is a state where a bound is not
        // one, and the two stay at zero or below
        match exact::enclosure(&store, 2_000_000usize) {
            Ok((low, high, states)) => {
                println!("worst L-min {low:e}  over {states} choice states");
                println!("worst max-U {high:e}");
            }
            Err(e) => println!("enclosure  unavailable: {e:?}"),
        }
    }
    0
}
// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// A window written as the values are shown, read by the engine: on a
/// maximized component the two ends swap and change sign.
fn directed_window(
    tree: &Tree,
    window: Option<Vec<(Option<num_rational::BigRational>, Option<num_rational::BigRational>)>>,
) -> Option<Vec<(Option<num_rational::BigRational>, Option<num_rational::BigRational>)>> {
    window.map(|w| {
        w.into_iter()
            .enumerate()
            .map(|(c, (lo, hi))| {
                if tree.maximized.get(c).copied().unwrap_or(false) {
                    (hi.map(|x| -x), lo.map(|x| -x))
                } else {
                    (lo, hi)
                }
            })
            .collect()
    })
}
