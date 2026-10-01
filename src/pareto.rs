//! The frontier of the achievable expected impacts, one strategy per point.
//!
//! The budget space is the box `[L, U]`, larger meaning looser. Feasibility is
//! closed upward, a budget above a feasible one being feasible, and
//! infeasibility is closed downward. The box is discretised into ticks, one
//! tick per component counting tightness from `u_i` down, and two OBDDs over
//! the bits of the ticks hold the grid points whose status is unknown and the
//! ones known feasible. The loop takes the tightest unknown point, the
//! lexicographically largest tick vector, and puts it to `determine`; the
//! loosest point and a point drawn at random by a walk on the diagram are the
//! alternatives. Every point below the tightest one is known, and known
//! infeasible, so an accept there has the point itself as its apex, a point of
//! the frontier, recorded with the strategy as its witness; in the other modes
//! the apex is the accepting sum of the winning frontier, which every
//! completion of the partial strategy pays at most. A refusal retires the cone
//! below the point and, with the certificate the search returns, every unknown
//! budget outside the cones above the minimal rejecting sums of the exhausted
//! tree. The minimal apexes at the end are the frontier, each born with its
//! strategy.
//!
//! Without a precision the tick is `1/D`, `D` a common denominator of every
//! achievable expected value: every run probability is a product of the
//! nature probabilities resolved in the run, every run cost a sum of impacts,
//! so the denominators of the probabilities along the tree times the common
//! denominator of the impacts is one, and two distinct achievable values are
//! at least `1/D` apart. The frontier is then exact, with no parameter, and
//! the search runs on rationals of arbitrary precision.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

use num_bigint::{BigInt, BigUint};
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, ToPrimitive, Zero};

use crate::anytime::{Progress, Sink};
use crate::bound::bounds;
use crate::num::Exact;
use crate::search::{self, Ablation, Answer, Config, FrontierOracle, Stats, Strategy};
use crate::state::Engine;
use crate::tree::{Kind, Tree};

// ---------------------------------------------------------------------------
// the OBDD library behind an interface of eight operations
// ---------------------------------------------------------------------------

/// What the loop asks of a decision diagram library: variables, constants,
/// the three connectives, a satisfiability test and a size.
pub trait Obdd: Sized + Send + Sync + 'static {
    type F: Clone + Send + Sync + 'static;
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
    /// One descent from the root of `a` to the terminal one, the assignment
    /// it spells out. At a node the descent takes the child `choose` asks
    /// for when that child leads somewhere and the other child otherwise,
    /// and a variable the descent skips takes the value `choose` gives it,
    /// those variables being free. Asking for true everywhere gives the
    /// lexicographically largest satisfying assignment, asking for false
    /// everywhere the smallest, and drawing gives a point of `a` drawn along
    /// the way. The cost is the depth of the diagram, one step per variable,
    /// against the size of the diagram once per variable that the same walk
    /// written with conjunctions would cost.
    fn descend(&self, a: &Self::F, choose: &mut dyn FnMut(usize) -> bool) -> Vec<bool>;
    /// The value of `a` at one assignment, a descent that reads and builds
    /// nothing, safe for many readers at once.
    fn eval(&self, a: &Self::F, bits: &[bool]) -> bool;
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
    fn descend(&self, a: &Self::F, choose: &mut dyn FnMut(usize) -> bool) -> Vec<bool> {
        let n = self.vars.len();
        let mut out = vec![false; n];
        if a.is_false() {
            return out;
        }
        let mut node = a.root_pointer();
        for i in 0..n {
            let want = choose(i);
            if node.is_terminal() || a.var_of(node) != self.vars[i] {
                // the variable is not on the path, so it is free
                out[i] = want;
                continue;
            }
            let (first, second) = if want {
                (a.high_link_of(node), a.low_link_of(node))
            } else {
                (a.low_link_of(node), a.high_link_of(node))
            };
            if first.is_zero() {
                out[i] = !want;
                node = second;
            } else {
                out[i] = want;
                node = first;
            }
        }
        out
    }
    fn eval(&self, a: &Self::F, bits: &[bool]) -> bool {
        a.eval_in(&biodivine_lib_bdd::BddValuation::new(bits.to_vec()))
    }
}

/// OxiDD: the concurrent framework, used here with one thread, the loop
/// being sequential by design.
pub struct Oxidd {
    manager: oxidd::bdd::BDDManagerRef,
    vars: Vec<oxidd::bdd::BDDFunction>,
}

impl Oxidd {
    /// An operation that ran out of nodes is tried again after a collection,
    /// the manager holding a fixed number of them and reclaiming only what
    /// nothing refers to any more. A second refusal is the end of the run.
    fn retry(&self, mut op: impl FnMut() -> oxidd::util::AllocResult<oxidd::bdd::BDDFunction>) -> oxidd::bdd::BDDFunction {
        use oxidd::{Manager, ManagerRef};
        match op() {
            Ok(f) => f,
            Err(_) => {
                self.manager.with_manager_exclusive(|m| m.gc());
                op().expect("OxiDD ran out of nodes and a collection did not free enough")
            }
        }
    }
}

impl Obdd for Oxidd {
    type F = oxidd::bdd::BDDFunction;

    fn with_vars(n: usize) -> Result<Oxidd, String> {
        use oxidd::{BooleanFunction, Manager, ManagerRef};
        // The index-based manager holds a fixed number of inner nodes and
        // does not grow, so the capacity is the ceiling of a run: dead nodes
        // are reclaimed by the collector below, live ones are not. Two to the
        // twenty-eighth is the largest power of two that leaves room for the
        // terminals under the limit of two to the thirty-second.
        let manager = oxidd::bdd::new_manager(1 << 28, 1 << 22, 1);
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
        self.retry(|| a.and(b))
    }
    fn or(&self, a: &Self::F, b: &Self::F) -> Self::F {
        use oxidd::BooleanFunction;
        self.retry(|| a.or(b))
    }
    fn not(&self, a: &Self::F) -> Self::F {
        use oxidd::BooleanFunction;
        self.retry(|| a.not())
    }
    fn is_sat(&self, a: &Self::F) -> bool {
        use oxidd::BooleanFunction;
        a.satisfiable()
    }
    fn size(&self, a: &Self::F) -> usize {
        use oxidd::Function;
        a.node_count()
    }
    fn descend(&self, a: &Self::F, choose: &mut dyn FnMut(usize) -> bool) -> Vec<bool> {
        use oxidd::util::OptBool;
        use oxidd::BooleanFunction;
        let n = self.vars.len();
        match a.pick_cube(|_m, _e, level| choose(level as usize)) {
            None => vec![false; n],
            Some(cube) => (0..n)
                .map(|i| match cube.get(i) {
                    Some(OptBool::True) => true,
                    Some(OptBool::False) => false,
                    // a don't care, free to take the value asked for
                    _ => choose(i),
                })
                .collect(),
        }
    }
    fn eval(&self, a: &Self::F, bits: &[bool]) -> bool {
        use oxidd::BooleanFunction;
        a.eval(bits.iter().enumerate().map(|(i, &b)| (i as u32, b)))
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

    /// The tightest tick at or above the value: `ceil((u_i - v) / eps)`, the
    /// grid point at or below `v`. Some budget known feasible below that
    /// point is below `v` as well, which is what a membership test needs.
    fn tick_ceil(&self, i: usize, v: &BigRational) -> BigUint {
        ceil_to_uint(&((&self.upper[i] - v) / &self.eps))
    }

    /// The bits of a tick vector, most significant first, or `None` when a
    /// tick does not fit its component, the point lying outside the box.
    fn bits_of(&self, ticks: &[BigUint]) -> Option<Vec<bool>> {
        let n: usize = self.comps.iter().map(|c| c.len()).sum();
        let mut bits = vec![false; n];
        for (vars, t) in self.comps.iter().zip(ticks) {
            let b = vars.len() as u64;
            if t.bits() > b {
                return None;
            }
            for j in 0..b {
                bits[vars[(b - 1 - j) as usize]] = t.bit(j);
            }
        }
        Some(bits)
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

    /// A point of `phi` drawn at random: the walk from the root of the
    /// diagram to the terminal one, taking either child with the same
    /// probability wherever both lead somewhere, the bits of the variables
    /// the walk skips drawn at random. Written variable by variable so as to
    /// need nothing of the library beyond the connectives: at a variable
    /// where both values keep `phi` satisfiable under the prefix the value is
    /// drawn, which is the choice at a node of the diagram when the variable
    /// is there and the draw of a skipped bit when it is not.
    fn random_point<O: Obdd>(&self, o: &O, phi: &O::F, rng: &mut Rng) -> Vec<BigUint> {
        let bits = o.descend(phi, &mut |_| rng.coin());
        self.ticks_of(&bits)
    }

    /// The cube of the ticks of every component but `skip`: `phi` and this
    /// cube is `phi` at those ticks, a function of the bits of `skip` alone.
    fn fix_others<O: Obdd>(&self, o: &O, phi: &O::F, t: &[BigUint], skip: usize) -> O::F {
        let mut cur = phi.clone();
        for (c, vars) in self.comps.iter().enumerate() {
            if c == skip {
                continue;
            }
            let b = vars.len() as u64;
            for j in 0..b {
                let x = o.var(vars[(b - 1 - j) as usize]);
                cur = if t[c].bit(j) { o.and(&cur, &x) } else { o.and(&cur, &o.not(&x)) };
            }
        }
        cur
    }

    /// The smallest tick of component `i` in the function `f` of its bits
    /// alone, the walk of `lexmin` on one component.
    fn least_tick<O: Obdd>(&self, o: &O, f: &O::F, i: usize) -> BigUint {
        let mut cur = f.clone();
        let mut t = BigUint::zero();
        for &v in &self.comps[i] {
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
        t
    }

    /// A maximal unknown point above `t`: the components are loosened one at
    /// a time, each to the least tick that keeps the point in `phi` with the
    /// others fixed, in a random order, until no component moves. The result
    /// is above `t` in every component and no point of `phi` is looser than
    /// it in every component, so a refusal there retires a corner and not a
    /// speck.
    fn loosen<O: Obdd>(&self, o: &O, phi: &O::F, mut t: Vec<BigUint>, rng: &mut Rng) -> Vec<BigUint> {
        let k = self.comps.len();
        let mut order: Vec<usize> = (0..k).collect();
        loop {
            // a random order of the components, by a Fisher-Yates shuffle
            for a in (1..k).rev() {
                let b = (rng.next() % (a as u64 + 1)) as usize;
                order.swap(a, b);
            }
            let mut moved = false;
            for &i in &order {
                if self.comps[i].is_empty() {
                    continue;
                }
                let f = self.fix_others(o, phi, &t, i);
                let least = self.least_tick(o, &f, i);
                if least < t[i] {
                    t[i] = least;
                    moved = true;
                }
            }
            if !moved {
                return t;
            }
        }
    }

    /// The ticks of an assignment over the bits, most significant first.
    fn ticks_of(&self, bits: &[bool]) -> Vec<BigUint> {
        self.comps
            .iter()
            .map(|vars| {
                let mut t = BigUint::zero();
                for &v in vars {
                    t <<= 1usize;
                    if bits[v] {
                        t += 1u32;
                    }
                }
                t
            })
            .collect()
    }

    /// The lexicographically largest tick vector in `phi`, one descent taking
    /// the bit true whenever it can.
    fn lexmax<O: Obdd>(&self, o: &O, phi: &O::F) -> Vec<BigUint> {
        let bits = o.descend(phi, &mut |_| true);
        self.ticks_of(&bits)
    }

    /// The lexicographically smallest tick vector in `phi`, one descent
    /// taking the bit false whenever it can.
    fn lexmin<O: Obdd>(&self, o: &O, phi: &O::F) -> Vec<BigUint> {
        let bits = o.descend(phi, &mut |_| false);
        self.ticks_of(&bits)
    }
}

/// A small generator for the draws of the extraction, splitmix64, seeded from
/// the seed of the search so that a run is reproducible.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn coin(&mut self) -> bool {
        self.next() & 1 == 1
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

/// How the next unknown point is chosen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Extract {
    /// A point drawn at random by a walk on the diagram of the unknown
    /// points, then loosened to a maximal unknown point above it: a point
    /// no other unknown point is looser than in every component.
    Random,
    /// The point drawn by the walk itself, not loosened.
    Draw,
    /// The loosest one, the lexicographically smallest tick vector.
    Loosest,
    /// The tightest one, the lexicographically largest tick vector, a
    /// minimal unknown point: every point below it is known, so an accept
    /// there has the point itself as its apex, a point of the frontier.
    Tightest,
}

impl Extract {
    pub fn parse(text: &str) -> Option<Extract> {
        match text {
            "random" => Some(Extract::Random),
            "draw" => Some(Extract::Draw),
            "loosest" => Some(Extract::Loosest),
            "tightest" => Some(Extract::Tightest),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Extract::Random => "random",
            Extract::Draw => "draw",
            Extract::Loosest => "loosest",
            Extract::Tightest => "tightest",
        }
    }
}

/// Which of the two certificates the loop reads. The one above turns an
/// accept into the value the winning frontier pays, which is at most the
/// budget asked and often far below it, and retires the cone above that
/// value; without it the accept says only yes, and the cone retired is the
/// one above the budget asked. The one below turns a refusal into the
/// minimal rejecting sums of the exhausted tree, every strategy paying at
/// least one of them, and retires everything outside the cones above them;
/// without it the refusal retires the cone below the budget asked alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Certificates {
    Both,
    Above,
    Below,
    None,
}

impl Certificates {
    pub fn parse(text: &str) -> Option<Certificates> {
        match text {
            "both" | "true" => Some(Certificates::Both),
            "above" => Some(Certificates::Above),
            "below" => Some(Certificates::Below),
            "none" | "false" => Some(Certificates::None),
            _ => None,
        }
    }
    pub fn above(self) -> bool {
        matches!(self, Certificates::Both | Certificates::Above)
    }
    pub fn below(self) -> bool {
        matches!(self, Certificates::Both | Certificates::Below)
    }
    pub fn name(self) -> &'static str {
        match self {
            Certificates::Both => "both",
            Certificates::Above => "above",
            Certificates::Below => "below",
            Certificates::None => "none",
        }
    }
}

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

/// What holds the budgets known feasible in the one-phase search.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OracleKind {
    /// The antichain of the apexes themselves, a list of vectors: a budget
    /// is known feasible when some apex is below it, one scan of the list.
    Antichain,
    /// The diagram of the grid points above some apex, the union of the
    /// cones, a membership test being one descent.
    Obdd,
}

impl OracleKind {
    pub fn parse(text: &str) -> Option<OracleKind> {
        match text {
            "antichain" => Some(OracleKind::Antichain),
            "obdd" => Some(OracleKind::Obdd),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            OracleKind::Antichain => "antichain",
            OracleKind::Obdd => "obdd",
        }
    }
}

pub struct Options {
    /// The frontier in one phase: one search of the whole tree against a
    /// shared record of the budgets known feasible, in place of the loop of
    /// decision queries.
    pub onephase: bool,
    pub oracle: OracleKind,
    pub extract: Extract,
    pub certificates: Certificates,
    /// Whether every refusal prints the budget it was asked and the vectors
    /// of its certificate, which is how a certificate is looked at.
    pub print_certificate: bool,
    /// A budget of wall-clock time for the whole exploration; past it the
    /// loop stops and the frontier is reported as partial.
    pub time_limit: Option<Duration>,
    /// The anytime buffer, where every apex is written as it is recorded, so
    /// that a run killed before it ends leaves its work on disk.
    pub anytime: Option<Arc<Sink>>,
    /// The tick, or `None` for the exact tick `1/D`.
    pub precision: Option<BigRational>,
    /// The components the frontier is taken over, the others left at their
    /// upper bound; `None` for all of them.
    pub components: Option<Vec<usize>>,
    /// A window on the expected values, per component the least and the
    /// greatest asked, either side open: the frontier is searched among
    /// the strategies whose expected values lie within it.
    pub window: Option<Vec<(Option<BigRational>, Option<BigRational>)>>,
    /// Where the search shows its apexes while it runs, for a snapshot
    /// taken from another thread; the antichain oracle writes it.
    pub live: Option<Arc<Live>>,
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
    /// Whether the point is a point of the whole frontier for certain: always
    /// when the search ended, and after a cut when no lower bound of an open
    /// node lies below it, in one phase, or no unknown budget lies below it,
    /// in two.
    pub certified: bool,
}

#[derive(Clone, Debug)]
pub enum Status {
    Complete,
    Timeout,
    Failed(String),
}

pub struct Report {
    pub library: &'static str,
    pub onephase: bool,
    pub oracle: OracleKind,
    /// One phase only: the membership tests the search made and the nodes it
    /// closed on them.
    pub oracle_reads: usize,
    pub oracle_pruned: usize,
    /// After a cut: the lower bounds of the open nodes, in one phase, that
    /// certify the points below none of them; and how many points are.
    pub open_bounds: usize,
    pub certified_points: usize,
    pub extract: Extract,
    pub certificates: Certificates,
    /// Which of the two early tests the search made, the record being unable
    /// to say so otherwise.
    pub ablation: Ablation,
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
    /// The refusals that carried a certificate, and the vectors in them.
    pub certified: usize,
    pub cert_vectors: usize,
    /// The cones built and the ones found already built.
    pub cone_built: usize,
    pub cone_hits: usize,
    pub max_size: usize,
    pub search_time: Duration,
    pub obdd_time: Duration,
    /// The diagram time split three ways: choosing the next point, building
    /// the cones of an answer, and merging them into the two diagrams.
    pub extract_time: Duration,
    pub cone_time: Duration,
    pub merge_time: Duration,
    pub search: Stats,
    pub status: Status,
    /// The frontier, sorted by the first component.
    pub points: Vec<Point>,
    /// The window the frontier was searched in, every side filled with the
    /// bound at the initial state where it was open, and how many points
    /// found fell outside it and were left out.
    pub window: Option<Vec<(BigRational, BigRational)>>,
    pub outside: usize,
}

/// The window of the options with its open sides filled by the two bounds
/// at the initial state, as the search reads it.
fn window_filled(
    opts: &Options,
    lower: &[BigRational],
    upper: &[BigRational],
) -> Option<Vec<(Exact, Exact)>> {
    opts.window.as_ref().map(|w| {
        (0..lower.len())
            .map(|c| {
                let lo = w.get(c).and_then(|p| p.0.clone()).unwrap_or_else(|| lower[c].clone());
                let hi = w.get(c).and_then(|p| p.1.clone()).unwrap_or_else(|| upper[c].clone());
                (Exact(lo), Exact(hi))
            })
            .collect()
    })
}

/// The points within the window, and how many were not.
fn within_window(points: Vec<Point>, window: Option<&[(Exact, Exact)]>) -> (Vec<Point>, usize) {
    match window {
        None => (points, 0),
        Some(w) => {
            let before = points.len();
            let kept: Vec<Point> = points
                .into_iter()
                .filter(|p| (0..w.len()).all(|c| w[c].0 .0 <= p.value[c] && p.value[c] <= w[c].1 .0))
                .collect();
            let outside = before - kept.len();
            (kept, outside)
        }
    }
}

pub fn frontier(tree: &Tree, opts: Options) -> Result<Report, String> {
    match (opts.library, opts.onephase) {
        (Library::Biodivine, false) => run::<Biodivine>(tree, opts),
        (Library::Oxidd, false) => run::<Oxidd>(tree, opts),
        (Library::Biodivine, true) => run_onephase::<Biodivine>(tree, opts),
        (Library::Oxidd, true) => run_onephase::<Oxidd>(tree, opts),
    }
}

/// The minimal apexes, compared on the grid: an apex is redundant when
/// another one is at least as tight in every component kept, its cone lying
/// inside the other. Among equal ticks the one with a witness wins, then the
/// earlier one.
pub fn minimal_apexes(apexes: &[Point], components: &[usize]) -> Vec<Point> {
    let key = |p: &Point, i: usize| (p.witness.is_none(), i);
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
}

/// The grid and the box of one instance, shared by the two modes.
struct Setup<O: Obdd> {
    upper: Vec<BigRational>,
    lower: Vec<BigRational>,
    eps: BigRational,
    denominator: Option<BigInt>,
    ticks: Vec<BigUint>,
    bits: Vec<usize>,
    components: Vec<usize>,
    grid: Grid,
    o: O,
}

fn setup<O: Obdd>(tree: &Tree, opts: &Options) -> Result<Setup<O>, String> {
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
    Ok(Setup {
        upper,
        lower,
        eps,
        denominator,
        ticks,
        bits,
        components,
        grid,
        o,
    })
}

/// The shared diagram of the budgets known feasible, read by every worker of
/// the one-phase search at once and written by one at a time.
struct Oracle<O: Obdd> {
    o: Arc<O>,
    grid: Arc<Grid>,
    sol: RwLock<O::F>,
    apexes: Mutex<Vec<Point>>,
    reads: AtomicUsize,
    pruned: AtomicUsize,
    sink: Option<Arc<Sink>>,
}

impl<O: Obdd> FrontierOracle<Exact> for Oracle<O> {
    fn known(&self, v: &[Exact]) -> bool {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let k = self.grid.comps.len();
        let ticks: Vec<BigUint> = (0..k).map(|i| self.grid.tick_ceil(i, &v[i].0)).collect();
        let bits = match self.grid.bits_of(&ticks) {
            Some(b) => b,
            None => return false,
        };
        let known = {
            let sol = self.sol.read().unwrap();
            self.o.eval(&sol, &bits)
        };
        if known {
            self.pruned.fetch_add(1, Ordering::Relaxed);
        }
        known
    }

    fn add(&self, v: &[Exact], strategy: Strategy<Exact>) {
        let k = self.grid.comps.len();
        let value: Vec<BigRational> = v.iter().map(|e| e.0.clone()).collect();
        let ta: Vec<BigUint> = (0..k).map(|i| self.grid.tick_floor(i, &value[i])).collect();
        // built outside the lock: the readers go on while the cone is made
        let cone = self.grid.cone_above(&*self.o, &ta);
        {
            let mut sol = self.sol.write().unwrap();
            *sol = self.o.or(&sol, &cone);
        }
        let mut apexes = self.apexes.lock().unwrap();
        let n = apexes.len() + 1;
        if let Some(sink) = &self.sink {
            // under the lock of the apexes, which is not the lock the workers
            // read: the lines come out in the order of the indices they carry
            sink.point(n, &value, &ta, strategy.decisions.len(), strategy.histories,
                       strategy.frontier_size);
        }
        apexes.push(Point {
            value,
            ticks: ta,
            origin: Origin::Query(n),
            witness: Some(Witness {
                decisions: strategy.decisions,
                histories: strategy.histories,
                open: strategy.frontier_size,
            }),
            certified: true,
        });
    }
}

impl<O: Obdd> Progress for Oracle<O> {
    fn snapshot(&self) -> (usize, usize, usize) {
        let apexes = self.apexes.lock().map(|a| a.len()).unwrap_or(0);
        (apexes, self.reads.load(Ordering::Relaxed), self.pruned.load(Ordering::Relaxed))
    }
}

/// The budgets known feasible as the antichain of the apexes: a list of
/// vectors kept free of dominated members, read by every worker at once and
/// written by one at a time. A budget is known feasible when some apex is
/// below it, which is one scan of the list with exact comparisons, and the
/// list is the frontier when the search is over.
struct Antichain {
    set: RwLock<Vec<Vec<Exact>>>,
    apexes: Mutex<Vec<Point>>,
    grid: Arc<Grid>,
    reads: AtomicUsize,
    pruned: AtomicUsize,
    sink: Option<Arc<Sink>>,
    live: Option<Arc<Live>>,
}

/// What a search running in the background shows while it runs: every
/// apex as it is recorded, with its witness, and the two counters of the
/// oracle as they stood at the last apex. A snapshot is the minimal apexes
/// of the moment.
pub struct Live {
    pub apexes: Mutex<Vec<Point>>,
    pub reads: AtomicUsize,
    pub pruned: AtomicUsize,
}

impl Live {
    pub fn new() -> Live {
        Live {
            apexes: Mutex::new(Vec::new()),
            reads: AtomicUsize::new(0),
            pruned: AtomicUsize::new(0),
        }
    }
}

impl Default for Live {
    fn default() -> Live {
        Live::new()
    }
}

fn below(a: &[Exact], v: &[Exact]) -> bool {
    a.iter().zip(v).all(|(x, y)| x.0 <= y.0)
}

impl FrontierOracle<Exact> for Antichain {
    fn known(&self, v: &[Exact]) -> bool {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let known = self.set.read().unwrap().iter().any(|a| below(a, v));
        if known {
            self.pruned.fetch_add(1, Ordering::Relaxed);
        }
        known
    }

    fn add(&self, v: &[Exact], strategy: Strategy<Exact>) {
        {
            let mut set = self.set.write().unwrap();
            // another worker may have recorded a budget below this one since
            // the test that brought us here
            if set.iter().any(|a| below(a, v)) {
                return;
            }
            set.retain(|a| !below(v, a));
            set.push(v.to_vec());
        }
        let k = self.grid.comps.len();
        let value: Vec<BigRational> = v.iter().map(|e| e.0.clone()).collect();
        let ta: Vec<BigUint> = (0..k).map(|i| self.grid.tick_floor(i, &value[i])).collect();
        let mut apexes = self.apexes.lock().unwrap();
        let n = apexes.len() + 1;
        if let Some(sink) = &self.sink {
            sink.point(n, &value, &ta, strategy.decisions.len(), strategy.histories,
                       strategy.frontier_size);
        }
        let point = Point {
            value,
            ticks: ta,
            origin: Origin::Query(n),
            witness: Some(Witness {
                decisions: strategy.decisions,
                histories: strategy.histories,
                open: strategy.frontier_size,
            }),
            certified: true,
        };
        if let Some(live) = &self.live {
            live.apexes.lock().unwrap().push(point.clone());
            live.reads.store(self.reads.load(Ordering::Relaxed), Ordering::Relaxed);
            live.pruned.store(self.pruned.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        apexes.push(point);
    }
}

impl Progress for Antichain {
    fn snapshot(&self) -> (usize, usize, usize) {
        let apexes = self.apexes.lock().map(|a| a.len()).unwrap_or(0);
        (apexes, self.reads.load(Ordering::Relaxed), self.pruned.load(Ordering::Relaxed))
    }
}

fn run_onephase<O: Obdd>(tree: &Tree, opts: Options) -> Result<Report, String> {
    let started = Instant::now();
    if let Some(sink) = &opts.anytime {
        sink.start(opts.search.workers);
    }
    let set = setup::<O>(tree, &opts)?;
    let o = Arc::new(set.o);
    let grid = Arc::new(set.grid);
    let obdd_oracle = Arc::new(Oracle {
        o: o.clone(),
        grid: grid.clone(),
        sol: RwLock::new(o.fls()),
        apexes: Mutex::new(Vec::new()),
        reads: AtomicUsize::new(0),
        pruned: AtomicUsize::new(0),
        sink: opts.anytime.clone(),
    });
    let chain_oracle = Arc::new(Antichain {
        set: RwLock::new(Vec::new()),
        apexes: Mutex::new(Vec::new()),
        grid: grid.clone(),
        reads: AtomicUsize::new(0),
        pruned: AtomicUsize::new(0),
        sink: opts.anytime.clone(),
        live: opts.live.clone(),
    });
    let mut cfg = opts.search.clone();
    cfg.threshold = set.upper.iter().cloned().map(Exact).collect();
    let window = window_filled(&opts, &set.lower, &set.upper);
    cfg.window = window.clone();
    cfg.record_strategy = true;
    cfg.certificate = false;
    cfg.oracle = Some(match opts.oracle {
        OracleKind::Obdd => obdd_oracle.clone() as Arc<dyn FrontierOracle<Exact>>,
        OracleKind::Antichain => chain_oracle.clone() as Arc<dyn FrontierOracle<Exact>>,
    });
    cfg.timeout = match (cfg.timeout, opts.time_limit) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    // the counters once a second, so that a run killed before it ends keeps
    // them: the record at the end is the one thing a kill loses. The wait is
    // on a condition and not a sleep, so that the end of the search stops the
    // thread at once: a sleep would hold every run back by its own period,
    // which on the instances that finish in microseconds is the whole run.
    let beat_stop = Arc::new((Mutex::new(false), Condvar::new()));
    let beat = opts.anytime.as_ref().map(|sink| {
        let sink = sink.clone();
        let stop = beat_stop.clone();
        let progress: Arc<dyn Progress> = match opts.oracle {
            OracleKind::Obdd => obdd_oracle.clone(),
            OracleKind::Antichain => chain_oracle.clone(),
        };
        std::thread::spawn(move || {
            let (lock, cv) = &*stop;
            let mut done = lock.lock().unwrap();
            while !*done {
                let (guard, waited) = cv.wait_timeout(done, Duration::from_secs(1)).unwrap();
                done = guard;
                if waited.timed_out() && !*done {
                    let (a, r, p) = progress.snapshot();
                    sink.stat(a, r, p);
                }
            }
        })
    });
    if let Some(sink) = &opts.anytime {
        sink.search(&set.components);
    }
    let t1 = Instant::now();
    let (answer, st) = search::search::<Exact>(tree, cfg);
    let search_time = t1.elapsed();
    {
        let (lock, cv) = &*beat_stop;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
    if let Some(handle) = beat {
        let _ = handle.join();
    }
    let (status, open) = match answer {
        Answer::Timeout(open) => (Status::Timeout, open),
        Answer::Failed(e) => (Status::Failed(e), Vec::new()),
        _ => (Status::Complete, Vec::new()),
    };
    let (apexes, reads, pruned, sol_size) = match opts.oracle {
        OracleKind::Obdd => (
            std::mem::take(&mut *obdd_oracle.apexes.lock().unwrap()),
            obdd_oracle.reads.load(Ordering::Relaxed),
            obdd_oracle.pruned.load(Ordering::Relaxed),
            o.size(&obdd_oracle.sol.read().unwrap()),
        ),
        OracleKind::Antichain => (
            std::mem::take(&mut *chain_oracle.apexes.lock().unwrap()),
            chain_oracle.reads.load(Ordering::Relaxed),
            chain_oracle.pruned.load(Ordering::Relaxed),
            chain_oracle.set.read().unwrap().len(),
        ),
    };
    let added = apexes.len();
    if let Some(sink) = &opts.anytime {
        sink.end(
            match status {
                Status::Complete => "complete",
                Status::Timeout => "timeout",
                Status::Failed(_) => "failed",
            },
            added,
            search_time.as_secs_f64(),
        );
    }
    let mut minimal = minimal_apexes(&apexes, &set.components);
    let components = set.components.clone();
    let proj = |p: &Point| -> Vec<BigRational> {
        components.iter().map(|&c| p.value[c].clone()).collect()
    };
    minimal.sort_by(|a, b| proj(a).cmp(&proj(b)));
    // after a cut a point is final when no open lower bound lies below it on
    // the components of the frontier: a hidden value that dominated it would
    // pass through an open node whose lower bound lies below it
    if matches!(status, Status::Timeout) {
        for p in minimal.iter_mut() {
            p.certified = !open
                .iter()
                .any(|v| components.iter().all(|&c| v[c].0 <= p.value[c]));
        }
    }
    let certified_points = minimal.iter().filter(|p| p.certified).count();
    if let Some(sink) = &opts.anytime {
        sink.minimal(minimal.len());
    }
    let elapsed = started.elapsed();
    let (minimal, outside) = within_window(minimal, window.as_deref());
    Ok(Report {
        library: o.name(),
        onephase: true,
        oracle: opts.oracle,
        oracle_reads: reads,
        oracle_pruned: pruned,
        open_bounds: open.len(),
        certified_points,
        extract: opts.extract,
        certificates: opts.certificates,
        ablation: opts.search.ablation,
        epsilon: set.eps,
        denominator: set.denominator,
        upper: set.upper,
        lower: set.lower,
        ticks: set.ticks,
        bits: set.bits,
        components: set.components,
        queries: 0,
        accepts: added,
        fails: 0,
        witness_queries: 0,
        certified: 0,
        cert_vectors: 0,
        cone_built: added,
        cone_hits: 0,
        max_size: sol_size,
        search_time,
        obdd_time: elapsed.saturating_sub(search_time),
        extract_time: Duration::ZERO,
        cone_time: Duration::ZERO,
        merge_time: Duration::ZERO,
        search: st,
        status,
        points: minimal,
        window: window.map(|w| w.into_iter().map(|(a, b)| (a.0, b.0)).collect()),
        outside,
    })
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
    let window = window_filled(&opts, &lower, &upper);

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
    let mut extract_time = Duration::ZERO;
    let mut cone_time = Duration::ZERO;
    let mut merge_time = Duration::ZERO;
    let mut search_time = Duration::ZERO;
    let mut total = Stats::default();
    let mut queries = 0usize;
    let mut accepts = 0usize;
    let mut fails = 0usize;
    let mut certified = 0usize;
    let mut cert_vectors = 0usize;
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
        certified: true,
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
            certified: true,
        });
        sol = o.or(&sol, &grid.cone_above(&o, &ta));
    }
    let mut unknown = o.and(&boxf, &o.not(&sol));
    max_size = max_size.max(o.size(&unknown));
    obdd_time += t0.elapsed();
    let mut rng = Rng::new(opts.search.seed);
    // The cones of the certificates repeat: the antichain of one refusal
    // shares vectors with the antichain of the next, the search having
    // pruned the same leaves. A cone is kept under its ticks and built once.
    let mut cones: HashMap<Vec<BigUint>, O::F> = HashMap::new();
    let mut cone_hits = 0usize;
    let mut cone_built = 0usize;

    loop {
        let t0 = Instant::now();
        if !o.is_sat(&unknown) {
            obdd_time += t0.elapsed();
            break;
        }
        let tx = match opts.extract {
            Extract::Random => {
                let drawn = grid.random_point(&o, &unknown, &mut rng);
                grid.loosen(&o, &unknown, drawn, &mut rng)
            }
            Extract::Draw => grid.random_point(&o, &unknown, &mut rng),
            Extract::Loosest => grid.lexmin(&o, &unknown),
            Extract::Tightest => grid.lexmax(&o, &unknown),
        };
        extract_time += t0.elapsed();
        obdd_time += t0.elapsed();

        // A query never runs past the time left on the whole exploration.
        let left = match remaining(&started) {
            Some(left) => left,
            None => {
                status = Status::Timeout;
                break;
            }
        };
        let asked: Vec<BigRational> = (0..k).map(|i| grid.budget(i, &tx[i])).collect();
        let mut cfg = opts.search.clone();
        cfg.window = window.clone();
        cfg.threshold = asked.iter().cloned().map(Exact).collect();
        cfg.record_strategy = true;
        cfg.certificate = opts.certificates.below();
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
                // With the certificate above, the apex is what the winning
                // frontier pays, at most the budget asked; without it the
                // accept says only that the budget asked is met, and the
                // budget asked is the apex.
                let value: Vec<BigRational> = if opts.certificates.above() {
                    strategy.accepting.iter().map(|e| e.0.clone()).collect()
                } else {
                    asked.clone()
                };
                let ta: Vec<BigUint> = (0..k).map(|i| grid.tick_floor(i, &value[i])).collect();
                let tc = Instant::now();
                let cone = grid.cone_above(&o, &ta);
                cone_time += tc.elapsed();
                let tm = Instant::now();
                sol = o.or(&sol, &cone);
                unknown = o.and(&unknown, &o.not(&cone));
                merge_time += tm.elapsed();
                apexes.push(Point {
                    value,
                    ticks: ta,
                    origin: Origin::Query(queries),
                    witness: Some(Witness {
                        decisions: strategy.decisions,
                        histories: strategy.histories,
                        open: strategy.frontier_size,
                    }),
                    certified: true,
                });
            }
            Answer::No(cert) => {
                fails += 1;
                if opts.print_certificate {
                    let show = |v: &[BigRational]| -> String {
                        let parts: Vec<String> =
                            v.iter().map(|r| format!("{:.6}", r.to_f64().unwrap_or(f64::NAN))).collect();
                        format!("[{}]", parts.join(", "))
                    };
                    println!("  refusal at query {queries}");
                    println!("    budget asked {}", show(&asked));
                    println!("    certificate  {} vectors, every strategy pays at least one", cert.len());
                    for v in &cert {
                        let vals: Vec<BigRational> = v.iter().map(|e| e.0.clone()).collect();
                        let over: Vec<String> = (0..k)
                            .filter(|&i| vals[i] > asked[i])
                            .map(|i| i.to_string())
                            .collect();
                        println!("      {} over budget on {}", show(&vals),
                                 if over.is_empty() { "no component".to_string() } else { over.join(", ") });
                    }
                }
                let tc = Instant::now();
                let cone = grid.cone_below(&o, &tx);
                cone_time += tc.elapsed();
                let tm = Instant::now();
                unknown = o.and(&unknown, &o.not(&cone));
                merge_time += tm.elapsed();
                // Every strategy pays at least one of the vectors of the
                // certificate, so a feasible budget lies above one of them:
                // the unknown points outside every such cone are infeasible.
                if opts.certificates.below() && !cert.is_empty() {
                    certified += 1;
                    cert_vectors += cert.len();
                    let tc = Instant::now();
                    let mut layer: Vec<O::F> = Vec::with_capacity(cert.len());
                    for v in &cert {
                        let tv: Vec<BigUint> =
                            (0..k).map(|i| grid.tick_floor(i, &v[i].0)).collect();
                        layer.push(match cones.get(&tv) {
                            Some(c) => {
                                cone_hits += 1;
                                c.clone()
                            }
                            None => {
                                cone_built += 1;
                                let c = grid.cone_above(&o, &tv);
                                cones.insert(tv, c.clone());
                                c
                            }
                        });
                    }
                    // The union is taken in a balanced tree: a chain would
                    // carry a growing diagram through every step.
                    while layer.len() > 1 {
                        let mut next = Vec::with_capacity(layer.len().div_ceil(2));
                        let mut it = layer.chunks(2);
                        while let Some(pair) = it.next() {
                            next.push(match pair {
                                [a, b] => o.or(a, b),
                                [a] => a.clone(),
                                _ => unreachable!(),
                            });
                        }
                        layer = next;
                    }
                    let allowed = layer.pop().unwrap_or_else(|| o.fls());
                    cone_time += tc.elapsed();
                    let tm = Instant::now();
                    unknown = o.and(&unknown, &allowed);
                    merge_time += tm.elapsed();
                }
            }
            Answer::Timeout(_) => {
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
    let mut minimal = minimal_apexes(&apexes, &components);
    let mut witness_queries = 0usize;
    if matches!(status, Status::Complete) {
        for p in minimal.iter_mut() {
            if p.witness.is_some() {
                continue;
            }
            let mut cfg = opts.search.clone();
            cfg.window = window.clone();
            cfg.threshold = p.value.iter().map(|r| Exact(r.clone())).collect();
            cfg.record_strategy = true;
            cfg.certificate = false;
            let t1 = Instant::now();
            let (answer, st) = search::search::<Exact>(tree, cfg);
            search_time += t1.elapsed();
            accumulate(&mut total, &st);
            witness_queries += 1;
            if let Answer::Yes(strategy) = answer {
                if opts.certificates.above() {
                    p.value = strategy.accepting.iter().map(|e| e.0.clone()).collect();
                    p.ticks = (0..k).map(|i| grid.tick_floor(i, &p.value[i])).collect();
                }
                p.witness = Some(Witness {
                    decisions: strategy.decisions,
                    histories: strategy.histories,
                    open: strategy.frontier_size,
                });
            }
        }
        minimal = minimal_apexes(&minimal, &components);
    }
    let proj = |p: &Point| -> Vec<BigRational> {
        components.iter().map(|&c| p.value[c].clone()).collect()
    };
    minimal.sort_by(|a, b| proj(a).cmp(&proj(b)));
    // after a cut a point is final when no unknown budget lies below it: a
    // hidden point of the frontier that dominated it would be such a budget
    if matches!(status, Status::Timeout) {
        for p in minimal.iter_mut() {
            let below = grid.cone_below(&o, &p.ticks);
            p.certified = !o.is_sat(&o.and(&unknown, &below));
        }
    }
    let certified_points = minimal.iter().filter(|p| p.certified).count();

    let (minimal, outside) = within_window(minimal, window.as_deref());
    Ok(Report {
        library: o.name(),
        onephase: false,
        oracle: opts.oracle,
        oracle_reads: 0,
        oracle_pruned: 0,
        open_bounds: 0,
        certified_points,
        extract: opts.extract,
        certificates: opts.certificates,
        ablation: opts.search.ablation,
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
        certified,
        cert_vectors,
        cone_built,
        cone_hits,
        max_size,
        search_time,
        obdd_time,
        extract_time,
        cone_time,
        merge_time,
        search: total,
        status,
        points: minimal,
        window: window.map(|w| w.into_iter().map(|(a, b)| (a.0, b.0)).collect()),
        outside,
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
    fn random_points_lie_in_phi() {
        let o = Biodivine::with_vars(6).unwrap();
        let grid = Grid {
            eps: BigRational::one(),
            upper: vec![BigRational::from_integer(7.into()); 2],
            lower: vec![BigRational::zero(); 2],
            ticks: vec![BigUint::from(7u32); 2],
            comps: vec![vec![0, 1, 2], vec![3, 4, 5]],
        };
        let a = grid.cone_below(&o, &[BigUint::from(3u32), BigUint::from(5u32)]);
        let b = grid.cone_below(&o, &[BigUint::from(3u32), BigUint::from(6u32)]);
        let phi = o.and(&a, &o.not(&b));
        let mut rng = Rng::new(7);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            let t = grid.random_point(&o, &phi, &mut rng);
            assert!(t[0] >= BigUint::from(3u32) && t[0] <= BigUint::from(7u32));
            assert_eq!(t[1], BigUint::from(5u32));
            seen.insert(t[0].clone());
        }
        assert_eq!(seen.len(), 5, "every point of phi is drawn");
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
