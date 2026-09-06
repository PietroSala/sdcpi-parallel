//! The frontier of the achievable expected impacts, one strategy per point.
//!
//! The budget space is the box `[L, U]`, larger meaning looser. Feasibility is
//! closed upward, a budget above a feasible one being feasible, and
//! infeasibility is closed downward. The box is discretised into ticks, one
//! tick per component counting tightness from `u_i` down, and two OBDDs over
//! the bits of the ticks hold the grid points whose status is unknown and the
//! ones known feasible. The loop extracts the loosest unknown point, the
//! lexicographically smallest tick vector, and puts it to `determine`. An
//! accept comes with the accepting sum of the winning frontier, which every
//! completion of the partial strategy pays at most: the cone above it is
//! feasible, the sum is recorded as an apex with the strategy as its witness.
//! A refusal is at a maximal infeasible grid point, every point above it being
//! known already, so the cone below it is retired. The minimal apexes at the
//! end are the frontier, each born with its strategy.
//!
//! Without a precision the tick is `1/D`, `D` a common denominator of every
//! achievable expected value: every run probability is a product of the
//! nature probabilities resolved in the run, every run cost a sum of impacts,
//! so the denominators of the probabilities along the tree times the common
//! denominator of the impacts is one, and two distinct achievable values are
//! at least `1/D` apart. The frontier is then exact, with no parameter, and
//! the search runs on rationals of arbitrary precision.

use std::time::{Duration, Instant};

use num_bigint::{BigInt, BigUint};
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, ToPrimitive, Zero};

use crate::bound::bounds;
use crate::num::Exact;
use crate::search::{self, Answer, Config, Stats};
use crate::state::Engine;
use crate::tree::{Kind, Tree};

// ---------------------------------------------------------------------------
// the OBDD library behind an interface of eight operations
// ---------------------------------------------------------------------------

/// What the loop asks of a decision diagram library: variables, constants,
/// the three connectives, a satisfiability test and a size.
pub trait Obdd: Sized {
    type F: Clone;
    fn with_vars(n: usize) -> Result<Self, String>;
    fn name(&self) -> &'static str;
    fn var(&self, i: usize) -> Self::F;
    fn tru(&self) -> Self::F;
    fn fls(&self) -> Self::F;
    fn and(&self, a: &Self::F, b: &Self::F) -> Self::F;
    fn or(&self, a: &Self::F, b: &Self::F) -> Self::F;
    fn not(&self, a: &Self::F) -> Self::F;
    fn is_sat(&self, a: &Self::F) -> bool;
    fn size(&self, a: &Self::F) -> usize;
}

/// `biodivine-lib-bdd`: pure Rust, single threaded, the default.
pub struct Biodivine {
    set: biodivine_lib_bdd::BddVariableSet,
    vars: Vec<biodivine_lib_bdd::BddVariable>,
}

impl Obdd for Biodivine {
    type F = biodivine_lib_bdd::Bdd;

    fn with_vars(n: usize) -> Result<Biodivine, String> {
        if n > u16::MAX as usize {
            return Err(format!("{n} tick bits, and biodivine-lib-bdd holds at most {}", u16::MAX));
        }
        let set = biodivine_lib_bdd::BddVariableSet::new_anonymous(n as u16);
        let vars = set.variables();
        Ok(Biodivine { set, vars })
    }
    fn name(&self) -> &'static str {
        "biodivine"
    }
    fn var(&self, i: usize) -> Self::F {
        self.set.mk_var(self.vars[i])
    }
    fn tru(&self) -> Self::F {
        self.set.mk_true()
    }
    fn fls(&self) -> Self::F {
        self.set.mk_false()
    }
    fn and(&self, a: &Self::F, b: &Self::F) -> Self::F {
        a.and(b)
    }
    fn or(&self, a: &Self::F, b: &Self::F) -> Self::F {
        a.or(b)
    }
    fn not(&self, a: &Self::F) -> Self::F {
        a.not()
    }
    fn is_sat(&self, a: &Self::F) -> bool {
        !a.is_false()
    }
    fn size(&self, a: &Self::F) -> usize {
        a.size()
    }
}

/// OxiDD: the concurrent framework, used here with one thread, the loop
/// being sequential by design.
pub struct Oxidd {
    manager: oxidd::bdd::BDDManagerRef,
    vars: Vec<oxidd::bdd::BDDFunction>,
}

impl Obdd for Oxidd {
    type F = oxidd::bdd::BDDFunction;

    fn with_vars(n: usize) -> Result<Oxidd, String> {
        use oxidd::{BooleanFunction, Manager, ManagerRef};
        let manager = oxidd::bdd::new_manager(1 << 24, 1 << 20, 1);
        let vars = manager
            .with_manager_exclusive(|m| {
                m.add_vars(n as u32);
                (0..n)
                    .map(|i| oxidd::bdd::BDDFunction::var(m, i as u32))
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(|_| "OxiDD ran out of nodes while declaring the variables".to_string())?;
        Ok(Oxidd { manager, vars })
    }
    fn name(&self) -> &'static str {
        "oxidd"
    }
    fn var(&self, i: usize) -> Self::F {
        self.vars[i].clone()
    }
    fn tru(&self) -> Self::F {
        use oxidd::{BooleanFunction, ManagerRef};
        self.manager.with_manager_shared(|m| oxidd::bdd::BDDFunction::t(m))
    }
    fn fls(&self) -> Self::F {
        use oxidd::{BooleanFunction, ManagerRef};
        self.manager.with_manager_shared(|m| oxidd::bdd::BDDFunction::f(m))
    }
    fn and(&self, a: &Self::F, b: &Self::F) -> Self::F {
        use oxidd::BooleanFunction;
        a.and(b).expect("OxiDD ran out of nodes")
    }
    fn or(&self, a: &Self::F, b: &Self::F) -> Self::F {
        use oxidd::BooleanFunction;
        a.or(b).expect("OxiDD ran out of nodes")
    }
    fn not(&self, a: &Self::F) -> Self::F {
        use oxidd::BooleanFunction;
        a.not().expect("OxiDD ran out of nodes")
    }
    fn is_sat(&self, a: &Self::F) -> bool {
        use oxidd::BooleanFunction;
        a.satisfiable()
    }
    fn size(&self, a: &Self::F) -> usize {
        use oxidd::Function;
        a.node_count()
    }
}

// ---------------------------------------------------------------------------
// thresholds, cones and the loosest point
// ---------------------------------------------------------------------------

/// `t >= a` over the bits `vars`, most significant first. Built from the
/// least significant bit up: the suffix is at least the suffix of `a` when the
/// bit exceeds the bit of `a`, or equals it and the rest is at least the rest.
fn ge<O: Obdd>(o: &O, vars: &[usize], a: &BigUint) -> O::F {
    let b = vars.len() as u64;
    if a.bits() > b {
        return o.fls();
    }
    let mut g = o.tru();
    for j in 0..b {
        let x = o.var(vars[(b - 1 - j) as usize]);
        g = if a.bit(j) { o.and(&x, &g) } else { o.or(&x, &g) };
    }
    g
}

/// `t <= a` over the bits `vars`, most significant first.
fn le<O: Obdd>(o: &O, vars: &[usize], a: &BigUint) -> O::F {
    let b = vars.len() as u64;
    if a.bits() > b {
        return o.tru();
    }
    let mut g = o.tru();
    for j in 0..b {
        let nx = o.not(&o.var(vars[(b - 1 - j) as usize]));
        g = if a.bit(j) { o.or(&nx, &g) } else { o.and(&nx, &g) };
    }
    g
}

/// The grid: the box, the tick, and the bits of every component.
struct Grid {
    eps: BigRational,
    upper: Vec<BigRational>,
    lower: Vec<BigRational>,
    /// `T_i`, the tick of `l_i`: the tightest budget on component `i`.
    ticks: Vec<BigUint>,
    /// The variables of every component, most significant bit first.
    comps: Vec<Vec<usize>>,
}

impl Grid {
    /// The budget at the tick `t` of component `i`: `u_i - t * eps`.
    fn budget(&self, i: usize, t: &BigUint) -> BigRational {
        &self.upper[i] - BigRational::from_integer(BigInt::from(t.clone())) * &self.eps
    }

    /// The loosest tick at or below the value: `floor((u_i - v) / eps)`. Every
    /// budget at least `v` is a grid point of tick at most this one.
    fn tick_floor(&self, i: usize, v: &BigRational) -> BigUint {
        let q = (&self.upper[i] - v) / &self.eps;
        let f = q.floor().to_integer();
        if f.sign() == num_bigint::Sign::Minus {
            BigUint::zero()
        } else {
            f.to_biguint().unwrap_or_default()
        }
    }

    /// The cone of the budgets at least the apex of ticks `ta`: `t <= ta` in
    /// every component.
    fn cone_above<O: Obdd>(&self, o: &O, ta: &[BigUint]) -> O::F {
        let mut f = o.tru();
        for (vars, a) in self.comps.iter().zip(ta) {
            f = o.and(&f, &le(o, vars, a));
        }
        f
    }

    /// The cone of the budgets at most the point of ticks `tx`: `t >= tx` in
    /// every component.
    fn cone_below<O: Obdd>(&self, o: &O, tx: &[BigUint]) -> O::F {
        let mut f = o.tru();
        for (vars, a) in self.comps.iter().zip(tx) {
            f = o.and(&f, &ge(o, vars, a));
        }
        f
    }

    /// The lexicographically smallest tick vector in `phi`, one top-down walk
    /// through the variables in order: the bit is taken false whenever some
    /// point of `phi` agrees with the prefix so far and that bit.
    fn lexmin<O: Obdd>(&self, o: &O, phi: &O::F) -> Vec<BigUint> {
        let mut cur = phi.clone();
        let mut out = Vec::with_capacity(self.comps.len());
        for vars in &self.comps {
            let mut t = BigUint::zero();
            for &v in vars {
                let x = o.var(v);
                let zero = o.and(&cur, &o.not(&x));
                t <<= 1usize;
                if o.is_sat(&zero) {
                    cur = zero;
                } else {
                    cur = o.and(&cur, &x);
                    t += 1u32;
                }
            }
            out.push(t);
        }
        out
    }
}

/// A common denominator of every achievable expected value: the common
/// denominator of the impacts times the recursion over the tree, which
/// multiplies the denominator of every nature node on top of the least common
/// multiple of its branches and the product of the two regions of a sequence
/// or a parallel, every run resolving both.
fn denominator(tree: &Tree) -> BigInt {
    fn rec(tree: &Tree, v: u32) -> BigInt {
        let n = tree.node(v);
        match n.kind {
            Kind::Task => BigInt::one(),
            Kind::Sequence | Kind::Parallel => rec(tree, n.low) * rec(tree, n.high),
            Kind::Choice => rec(tree, n.low).lcm(&rec(tree, n.high)),
            Kind::Nature => tree.prob_exact(v).denom() * rec(tree, n.low).lcm(&rec(tree, n.high)),
        }
    }
    let mut d = BigInt::one();
    for id in 0..tree.nodes.len() as u32 {
        if tree.node(id).kind == Kind::Task {
            for r in tree.impact_exact(id) {
                d = d.lcm(r.denom());
            }
        }
    }
    d * rec(tree, tree.root)
}

fn ceil_to_uint(q: &BigRational) -> BigUint {
    let c = q.ceil().to_integer();
    if c.sign() == num_bigint::Sign::Minus {
        BigUint::zero()
    } else {
        c.to_biguint().unwrap_or_default()
    }
}

fn leq_vec(a: &[BigRational], b: &[BigRational]) -> bool {
    a.iter().zip(b).all(|(x, y)| x <= y)
}

// ---------------------------------------------------------------------------
// the command
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Library {
    Biodivine,
    Oxidd,
}

impl Library {
    pub fn parse(text: &str) -> Option<Library> {
        match text {
            "biodivine" => Some(Library::Biodivine),
            "oxidd" => Some(Library::Oxidd),
            _ => None,
        }
    }
}

pub struct Options {
    /// A budget of wall-clock time for the whole exploration; past it the
    /// loop stops and the frontier is reported as partial.
    pub time_limit: Option<Duration>,
    /// The tick, or `None` for the exact tick `1/D`.
    pub precision: Option<BigRational>,
    /// The components the frontier is taken over, the others left at their
    /// upper bound; `None` for all of them.
    pub components: Option<Vec<usize>>,
    pub library: Library,
    /// The options of `determine`; the threshold is set per query.
    pub search: Config<Exact>,
}

/// Where an apex came from.
#[derive(Clone, Debug)]
pub enum Origin {
    /// `U`, feasible without a query.
    Upper,
    /// `l_i` on axis `i` and `u` elsewhere, feasible by the static strategy of
    /// component `i`.
    Axis(usize),
    /// The accepting sum of the winning frontier of query `n`.
    Query(usize),
}

#[derive(Clone, Debug)]
pub struct Witness {
    pub decisions: Vec<(Vec<u32>, u32)>,
    pub histories: usize,
    pub open: usize,
}

#[derive(Clone, Debug)]
pub struct Point {
    pub value: Vec<BigRational>,
    /// The ticks of the value, the grid point it rounds to on the loose side.
    pub ticks: Vec<BigUint>,
    pub origin: Origin,
    pub witness: Option<Witness>,
}

#[derive(Clone, Debug)]
pub enum Status {
    Complete,
    Timeout,
    Failed(String),
}

pub struct Report {
    pub library: &'static str,
    pub epsilon: BigRational,
    pub denominator: Option<BigInt>,
    pub upper: Vec<BigRational>,
    pub lower: Vec<BigRational>,
    pub ticks: Vec<BigUint>,
    pub bits: Vec<usize>,
    pub components: Vec<usize>,
    pub queries: usize,
    pub accepts: usize,
    pub fails: usize,
    pub witness_queries: usize,
    pub max_size: usize,
    pub search_time: Duration,
    pub obdd_time: Duration,
    pub search: Stats,
    pub status: Status,
    /// The frontier, sorted by the first component.
    pub points: Vec<Point>,
}

pub fn frontier(tree: &Tree, opts: Options) -> Result<Report, String> {
    match opts.library {
        Library::Biodivine => run::<Biodivine>(tree, opts),
        Library::Oxidd => run::<Oxidd>(tree, opts),
    }
}

fn accumulate(total: &mut Stats, st: &Stats) {
    total.expanded += st.expanded;
    total.choice_states += st.choice_states;
    total.histories += st.histories;
    total.macro_steps += st.macro_steps;
    total.outcomes += st.outcomes;
    total.peak_open = total.peak_open.max(st.peak_open);
    total.elapsed += st.elapsed;
}

fn run<O: Obdd>(tree: &Tree, opts: Options) -> Result<Report, String> {
    let started = Instant::now();
    // The time left, if a limit is set: `None` when the limit is spent.
    let remaining = |started: &Instant| -> Option<Option<Duration>> {
        match opts.time_limit {
            None => Some(None),
            Some(limit) => {
                let spent = started.elapsed();
                if spent >= limit {
                    None
                } else {
                    Some(Some(limit - spent))
                }
            }
        }
    };
    let k = tree.k;
    let engine = Engine::new(tree);
    let s0 = engine.initial_state();
    let b = bounds::<Exact>(tree, &s0);
    let upper: Vec<BigRational> = b.upper.into_iter().map(|e| e.0).collect();
    let lower: Vec<BigRational> = b.lower.into_iter().map(|e| e.0).collect();

    let (eps, denominator) = match &opts.precision {
        Some(e) => {
            if e <= &BigRational::zero() {
                return Err("the precision must be positive".to_string());
            }
            (e.clone(), None)
        }
        None => {
            let d = denominator(tree);
            (BigRational::new(BigInt::one(), d.clone()), Some(d))
        }
    };

    let components: Vec<usize> = match &opts.components {
        Some(c) => {
            if c.is_empty() || c.iter().any(|&i| i >= k) {
                return Err(format!("--components names a component outside 0..{k}"));
            }
            let mut c = c.clone();
            c.sort_unstable();
            c.dedup();
            c
        }
        None => (0..k).collect(),
    };
    // A component left out keeps its budget at the upper bound: no tick, no
    // bit, and the frontier is the projection on the components kept.
    let ticks: Vec<BigUint> = (0..k)
        .map(|i| {
            if components.contains(&i) {
                ceil_to_uint(&((&upper[i] - &lower[i]) / &eps))
            } else {
                BigUint::zero()
            }
        })
        .collect();
    let bits: Vec<usize> = ticks.iter().map(|t| t.bits() as usize).collect();
    let mut comps: Vec<Vec<usize>> = Vec::with_capacity(k);
    let mut next = 0usize;
    for &b in &bits {
        comps.push((next..next + b).collect());
        next += b;
    }
    let o = O::with_vars(next)?;
    let grid = Grid {
        eps: eps.clone(),
        upper: upper.clone(),
        lower: lower.clone(),
        ticks: ticks.clone(),
        comps,
    };

    let mut obdd_time = Duration::ZERO;
    let mut search_time = Duration::ZERO;
    let mut total = Stats::default();
    let mut queries = 0usize;
    let mut accepts = 0usize;
    let mut fails = 0usize;
    let mut max_size = 0usize;
    let mut status = Status::Complete;

    // The box, the two cones known without a query, and the unknown rest.
    let t0 = Instant::now();
    let mut boxf = o.tru();
    for (vars, t) in grid.comps.iter().zip(&grid.ticks) {
        boxf = o.and(&boxf, &le(&o, vars, t));
    }
    let zero: Vec<BigUint> = (0..k).map(|_| BigUint::zero()).collect();
    let mut apexes: Vec<Point> = vec![Point {
        value: upper.clone(),
        ticks: zero.clone(),
        origin: Origin::Upper,
        witness: None,
    }];
    let mut sol = grid.cone_above(&o, &zero);
    for i in 0..k {
        let mut ta = zero.clone();
        ta[i] = ticks[i].clone();
        let mut v = upper.clone();
        v[i] = lower[i].clone();
        apexes.push(Point {
            value: v,
            ticks: ta.clone(),
            origin: Origin::Axis(i),
            witness: None,
        });
        sol = o.or(&sol, &grid.cone_above(&o, &ta));
    }
    let mut unknown = o.and(&boxf, &o.not(&sol));
    max_size = max_size.max(o.size(&unknown));
    obdd_time += t0.elapsed();

    loop {
        let t0 = Instant::now();
        if !o.is_sat(&unknown) {
            obdd_time += t0.elapsed();
            break;
        }
        let tx = grid.lexmin(&o, &unknown);
        obdd_time += t0.elapsed();

        // A query never runs past the time left on the whole exploration.
        let left = match remaining(&started) {
            Some(left) => left,
            None => {
                status = Status::Timeout;
                break;
            }
        };
        let budget: Vec<Exact> = (0..k).map(|i| Exact(grid.budget(i, &tx[i]))).collect();
        let mut cfg = opts.search.clone();
        cfg.threshold = budget;
        cfg.record_strategy = true;
        cfg.timeout = match (cfg.timeout, left) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let t1 = Instant::now();
        let (answer, st) = search::search::<Exact>(tree, cfg);
        search_time += t1.elapsed();
        accumulate(&mut total, &st);
        queries += 1;

        let t2 = Instant::now();
        match answer {
            Answer::Yes(strategy) => {
                accepts += 1;
                let value: Vec<BigRational> =
                    strategy.accepting.iter().map(|e| e.0.clone()).collect();
                let ta: Vec<BigUint> = (0..k).map(|i| grid.tick_floor(i, &value[i])).collect();
                let cone = grid.cone_above(&o, &ta);
                sol = o.or(&sol, &cone);
                unknown = o.and(&unknown, &o.not(&cone));
                apexes.push(Point {
                    value,
                    ticks: ta,
                    origin: Origin::Query(queries),
                    witness: Some(Witness {
                        decisions: strategy.decisions,
                        histories: strategy.histories,
                        open: strategy.frontier_size,
                    }),
                });
            }
            Answer::No => {
                fails += 1;
                let cone = grid.cone_below(&o, &tx);
                unknown = o.and(&unknown, &o.not(&cone));
            }
            Answer::Timeout => {
                status = Status::Timeout;
                obdd_time += t2.elapsed();
                break;
            }
            Answer::Failed(e) => {
                status = Status::Failed(e);
                obdd_time += t2.elapsed();
                break;
            }
        }
        max_size = max_size.max(o.size(&unknown));
        obdd_time += t2.elapsed();
    }
    drop(sol);

    // The minimal apexes, compared on the grid: an apex is redundant when
    // another one is at least as tight in every component kept, its cone
    // lying inside the other. At the exact tick this is the comparison of the
    // values; at a coarser one two values rounding to the same grid point
    // count as one, the earlier witness winning. A point known feasible
    // without a query gets its witness now, if it is on the frontier at all,
    // and the accepting sum of that run, at most the point and equal to it on
    // the components of the frontier, replaces it; the filter runs once more
    // after that, a replaced value being able to retire others.
    let key = |p: &Point, i: usize| (p.witness.is_none(), i);
    let minimal_apexes = |apexes: &[Point]| -> Vec<Point> {
        let mut out = Vec::new();
        for (i, a) in apexes.iter().enumerate() {
            let dominated = apexes.iter().enumerate().any(|(j, b)| {
                j != i
                    && components.iter().all(|&c| b.ticks[c] >= a.ticks[c])
                    && (components.iter().any(|&c| b.ticks[c] != a.ticks[c])
                        || key(b, j) < key(a, i))
            });
            if !dominated {
                out.push(a.clone());
            }
        }
        out
    };
    let mut minimal = minimal_apexes(&apexes);
    let mut witness_queries = 0usize;
    if matches!(status, Status::Complete) {
        for p in minimal.iter_mut() {
            if p.witness.is_some() {
                continue;
            }
            let mut cfg = opts.search.clone();
            cfg.threshold = p.value.iter().map(|r| Exact(r.clone())).collect();
            cfg.record_strategy = true;
            let t1 = Instant::now();
            let (answer, st) = search::search::<Exact>(tree, cfg);
            search_time += t1.elapsed();
            accumulate(&mut total, &st);
            witness_queries += 1;
            if let Answer::Yes(strategy) = answer {
                p.value = strategy.accepting.iter().map(|e| e.0.clone()).collect();
                p.ticks = (0..k).map(|i| grid.tick_floor(i, &p.value[i])).collect();
                p.witness = Some(Witness {
                    decisions: strategy.decisions,
                    histories: strategy.histories,
                    open: strategy.frontier_size,
                });
            }
        }
        minimal = minimal_apexes(&minimal);
    }
    let proj = |p: &Point| -> Vec<BigRational> {
        components.iter().map(|&c| p.value[c].clone()).collect()
    };
    minimal.sort_by(|a, b| proj(a).cmp(&proj(b)));

    Ok(Report {
        library: o.name(),
        epsilon: eps,
        denominator,
        upper,
        lower,
        ticks,
        bits,
        components,
        queries,
        accepts,
        fails,
        witness_queries,
        max_size,
        search_time,
        obdd_time,
        search: total,
        status,
        points: minimal,
    })
}

/// A decimal rendering of a rational for the report.
pub fn approx(r: &BigRational) -> f64 {
    r.to_f64().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count<O: Obdd>(o: &O, vars: &[usize], f: &O::F) -> usize {
        // enumerate all assignments of up to 6 bits
        let n = vars.len();
        let mut c = 0;
        for m in 0..(1usize << n) {
            let mut g = f.clone();
            for (j, &v) in vars.iter().enumerate() {
                let bit = (m >> (n - 1 - j)) & 1 == 1;
                let x = o.var(v);
                g = if bit { o.and(&g, &x) } else { o.and(&g, &o.not(&x)) };
            }
            if o.is_sat(&g) {
                c += 1;
            }
        }
        c
    }

    #[test]
    fn thresholds_count_right() {
        let o = Biodivine::with_vars(5).unwrap();
        let vars: Vec<usize> = (0..5).collect();
        for a in 0u32..40 {
            let ge_count = count(&o, &vars, &ge(&o, &vars, &BigUint::from(a)));
            let le_count = count(&o, &vars, &le(&o, &vars, &BigUint::from(a)));
            let expect_ge = if a >= 32 { 0 } else { 32 - a as usize };
            let expect_le = if a >= 32 { 32 } else { a as usize + 1 };
            assert_eq!(ge_count, expect_ge, "ge {a}");
            assert_eq!(le_count, expect_le, "le {a}");
        }
    }

    #[test]
    fn lexmin_walks_to_the_smallest() {
        let o = Biodivine::with_vars(6).unwrap();
        let grid = Grid {
            eps: BigRational::one(),
            upper: vec![BigRational::from_integer(7.into()); 2],
            lower: vec![BigRational::zero(); 2],
            ticks: vec![BigUint::from(7u32); 2],
            comps: vec![vec![0, 1, 2], vec![3, 4, 5]],
        };
        // t0 >= 3 and t1 >= 5, minus (t0 >= 3 and t1 >= 6)
        let a = grid.cone_below(&o, &[BigUint::from(3u32), BigUint::from(5u32)]);
        let b = grid.cone_below(&o, &[BigUint::from(3u32), BigUint::from(6u32)]);
        let phi = o.and(&a, &o.not(&b));
        let m = grid.lexmin(&o, &phi);
        assert_eq!(m, vec![BigUint::from(3u32), BigUint::from(5u32)]);
    }
}
