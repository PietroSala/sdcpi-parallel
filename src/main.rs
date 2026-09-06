//! `sdcpi`: strategy determination for BPMN+CPI processes.
//!
//!     sdcpi determine <instance> (--B a,b,... | --B-file F) [options]
//!     sdcpi info      <instance>
//!     sdcpi bound     <instance>
//!     sdcpi optima    <instance>
//!
//! An instance is a path to a YAML file.

mod arena;
mod parse;
mod bound;
mod exact;
mod num;
mod pareto;
mod search;
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
        "to_objective" => cmd_to_objective(&args[1..]),
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
  sdcpi parse     (<process> | --file F) [--out F]
  sdcpi to_prism  <instance> [--encode-history true|false] [--out F]
  sdcpi to_objective <instance> (--B a,b,... | --B-file F | --pareto true) [--out F]
  sdcpi info      <instance>
  sdcpi bound     <instance>
  sdcpi optima    <instance>
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
  meaning all zero; # comments to end of line. The instance file is written
  to standard output, identifiers in-order, impact_names in the order the
  names first appear, the vectors mapped by that order.

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
  --extract MODE        random | draw | loosest | tightest (default random)
                        random draws an unknown point by a walk on the
                        diagram, seeded by --seed, and loosens it to a
                        maximal unknown point; draw keeps the drawn point
                        as it is; loosest takes the lexicographically
                        smallest tick vector, tightest the largest, where
                        an accept is a point of the frontier itself
  --certificate B       true, the default, reads the certificate of every
                        refusal, the minimal rejecting sums of the leaves
                        of the exhausted tree, and retires every budget
                        below all of them; false retires the cone below
                        the refused point alone
  --precision E         the tick of the grid; without it the tick is 1/D
                        with D a common denominator of every achievable
                        expected value, and the frontier is exact
  --obdd-library L      biodivine | oxidd                 (default biodivine)
  --components i,j,...  the frontier over these components only, the others
                        left at their upper bound       (default all)
  --time-limit SECS     stop the exploration after this many seconds in all
                        and report the frontier found so far as partial; a
                        query never runs past the time left
  --json F              write the report to F as JSON as well
  --print-strategy 1    print the strategy behind every point, and the
                        decisions in the JSON report

options for optima
  --max-states N        give up past this many choice states (default 2000000)
";

// ---------------------------------------------------------------------------
// arguments
// ---------------------------------------------------------------------------

struct Args {
    positional: Vec<String>,
    flags: Vec<(String, String)>,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut out = Args {
        positional: Vec::new(),
        flags: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(name) = a.strip_prefix("--") {
            if let Some((n, v)) = name.split_once('=') {
                out.flags.push((n.to_string(), v.to_string()));
                i += 1;
            } else {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| format!("--{name} wants a value"))?;
                out.flags.push((name.to_string(), v.clone()));
                i += 2;
            }
        } else {
            out.positional.push(a.clone());
            i += 1;
        }
    }
    Ok(out)
}

impl Args {
    fn flag(&self, name: &str) -> Option<&str> {
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
    let path = PathBuf::from(spec);
    let tree = Tree::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
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
fn fail(message: &str) -> i32 {
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
    println!("max duration  {}", m.max_duration);
    let total = tree.total_impact();
    println!("total impact  {}", fmt_vec(&total));
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
    match parse::to_yaml(&text) {
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
                println!("component {c}  min {:e}  max {:e}", o.min[c], o.max[c]);
            }
            0
        }
        Err(e) => fail(&format!("{e:?}")),
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
            .map(|j| format!("R{{\"impact{j}\"}}min=? [ F n{}=-2 ]", tree.root))
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
            .map(|(j, v)| format!("R{{\"impact{j}\"}}<={} [ F n{}=-2 ]", v.text, tree.root))
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
struct SearchOptions {
    steal: Steal,
    epsilon: f64,
    workers: usize,
    ablation: Ablation,
    selection: Selection,
    seed: u64,
    timeout: Option<Duration>,
}

impl SearchOptions {
    fn from_args(args: &Args) -> Result<SearchOptions, String> {
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
            workers: args
                .flag("workers")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1),
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

    fn config<N: Num>(&self, threshold: Vec<N>, record_strategy: bool) -> Config<N> {
        Config {
            threshold,
            record_strategy,
            certificate: false,
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
    let cfg = opts.config(threshold.clone(), print_strategy || print_size);
    let (answer, stats) = search::search(tree, cfg);

    let approx: Vec<f64> = threshold.iter().map(|b| b.to_f64()).collect();
    println!("instance       {}", tree.meta.key);
    println!("B              {}", fmt_vec(&approx));
    if exact {
        println!("B exact        {}", fmt_exact(&threshold));
    }
    println!(
        "answer         {}",
        match &answer {
            Answer::Yes(_) => "yes",
            Answer::No(_) => "no",
            Answer::Timeout => "timeout",
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
            Answer::Timeout => "- timed out, no answer",
            Answer::Failed(_) => "- failed, no answer",
        }
    );
    if let Answer::Yes(s) = &answer {
        if exact {
            println!("accepting      {}", fmt_exact(&s.accepting));
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
    let extract = match args.flag("extract").map(pareto::Extract::parse) {
        None => pareto::Extract::Random,
        Some(Some(e)) => e,
        Some(None) => return fail("--extract wants random, draw, loosest or tightest"),
    };
    let certificate = match args.flag("certificate") {
        None | Some("true") => true,
        Some("false") => false,
        Some(v) => return fail(&format!("--certificate wants true or false, found {v:?}")),
    };
    let time_limit = match args.flag("time-limit") {
        None => None,
        Some(t) => match t.parse::<f64>() {
            Ok(v) if v > 0.0 => Some(Duration::from_secs_f64(v)),
            _ => return fail(&format!("--time-limit wants a positive number of seconds, found {t:?}")),
        },
    };
    let print_strategy = args.flag("print-strategy").is_some();
    let started = Instant::now();
    let report = match pareto::frontier(
        &tree,
        pareto::Options {
            extract,
            time_limit,
            precision,
            components,
            library,
            search: Config {
                certificate,
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
    println!("extract        {}", report.extract.name());
    match &report.denominator {
        Some(d) => {
            println!("tick           1/D exact");
            println!("D              {d}");
        }
        None => println!("tick           {} ({:.6e})", report.epsilon, pareto::approx(&report.epsilon)),
    }
    let comps: Vec<String> = report.components.iter().map(|c| c.to_string()).collect();
    println!("components     [{}]", comps.join(", "));
    println!("U at s0        {}", dec(&report.upper));
    println!("L at s0        {}", dec(&report.lower));
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
    println!("certificates   {} refusals carried {} vectors", report.certified, report.cert_vectors);
    println!("seconds        {:.6}", elapsed.as_secs_f64());
    println!("search seconds {:.6}", report.search_time.as_secs_f64());
    println!("obdd seconds   {:.6}", report.obdd_time.as_secs_f64());
    println!("obdd peak size {}", report.max_size);
    println!("expanded       {}", report.search.expanded);
    println!("points         {}", report.points.len());
    for (i, p) in report.points.iter().enumerate() {
        println!("point {:<9}{}", i + 1, dec(&p.value));
        println!("  exact        {}", rat(&p.value));
        println!(
            "  origin       {}",
            match p.origin {
                pareto::Origin::Upper => "U".to_string(),
                pareto::Origin::Axis(c) => format!("axis {c}"),
                pareto::Origin::Query(n) => format!("query {n}"),
            }
        );
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
    o.push_str(&format!("  \"extract\": {},\n", esc(r.extract.name())));
    o.push_str(&format!("  \"tick\": {},\n", esc(&r.epsilon.to_string())));
    o.push_str(&format!(
        "  \"denominator\": {},\n",
        match &r.denominator {
            Some(d) => esc(&d.to_string()),
            None => "null".to_string(),
        }
    ));
    o.push_str(&format!("  \"components\": {},\n", nums(&r.components)));
    o.push_str(&format!("  \"upper\": {},\n", strs(&r.upper)));
    o.push_str(&format!("  \"lower\": {},\n", strs(&r.lower)));
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
    o.push_str(&format!("  \"certified_refusals\": {},\n", r.certified));
    o.push_str(&format!("  \"certificate_vectors\": {},\n", r.cert_vectors));
    o.push_str(&format!("  \"seconds\": {:.6},\n", elapsed.as_secs_f64()));
    o.push_str(&format!("  \"search_seconds\": {:.6},\n", r.search_time.as_secs_f64()));
    o.push_str(&format!("  \"obdd_seconds\": {:.6},\n", r.obdd_time.as_secs_f64()));
    o.push_str(&format!("  \"obdd_peak_size\": {},\n", r.max_size));
    o.push_str(&format!("  \"expanded\": {},\n", r.search.expanded));
    o.push_str(&format!("  \"choice_states\": {},\n", r.search.choice_states));
    o.push_str(&format!("  \"histories\": {},\n", r.search.histories));
    o.push_str(&format!("  \"peak_open\": {},\n", r.search.peak_open));
    o.push_str("  \"points\": [\n");
    for (i, p) in r.points.iter().enumerate() {
        let approx: Vec<String> = p.value.iter().map(|v| format!("{:.9}", pareto::approx(v))).collect();
        o.push_str("    {\n");
        o.push_str(&format!("      \"value\": {},\n", strs(&p.value)));
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
    println!("U at s0    {}", fmt_vec(&b.upper));
    println!("L at s0    {}", fmt_vec(&b.lower));
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
    println!("root E     {}", fmt_vec(&e));
    println!("root L     {}", fmt_vec(&l));
    println!("root size  {} histories", initial.len());
    if args.flag("no-exact").is_none() {
        match exact::optima(&store, 2_000_000usize) {
            Ok(o) => {
                println!("exact min  {}", fmt_vec(&o.min));
                println!("exact max  {}", fmt_vec(&o.max));
            }
            Err(e) => println!("exact      unavailable: {e:?}"),
        }
    }
    0
}
// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------
