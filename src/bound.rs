//! The two bounds of Definition 12: `U_s` from above and `L_s` from below.
//!
//! One traversal of the tree computes both. Every case is shared but one, the
//! choice whose two branches are both idle, where the upper bound takes the
//! componentwise maximum of its branches and the lower bound their componentwise
//! minimum. The recursion reads the state only through the three phases of a
//! node and never through the elapsed progress, which is what lets it be
//! computed once per choice state and reused by every history that ends there.
//!
//! The guard of the sequence case is `s(high(v)) != -1` and not
//! `s(high(v)) >= 0`. The narrow guard leaves `U_s` an upper bound, the surplus
//! being non-negative, and destroys `L_s` outright: on `a -> b`, which carries no
//! XOR node at all, the state where the root runs, `a` is idle and `b` is done
//! would be charged the impact of `a` although the only continuation is the
//! closing move, which pays nothing.
//!
//! The arithmetic is the one of the type parameter, see `num.rs`: the
//! floating-point runs of the paper and the exact runs of the frontier take the
//! same traversal.

use crate::num::Num;
use crate::state::{DONE, IDLE};
use crate::tree::{Kind, Tree};

/// The pair `(U_s, L_s)`, each of `k` components.
pub struct Bounds<N: Num> {
    pub upper: Vec<N>,
    pub lower: Vec<N>,
}

/// Computes `U_s` and `L_s` at the root of the tree.
pub fn bounds<N: Num>(tree: &Tree, s: &[i8]) -> Bounds<N> {
    let k = tree.k;
    let mut stack: Vec<N> = Vec::with_capacity(8 * k);
    rec(tree, s, tree.root, &mut stack);
    debug_assert_eq!(stack.len(), 2 * k);
    let lower = stack.split_off(k);
    Bounds {
        upper: stack,
        lower,
    }
}

/// Pushes the `2k` values `U_s(v)` then `L_s(v)` on `st`.
///
/// The scratch stack keeps the recursion free of allocation: a combinator reads
/// the `4k` values its two children left and folds them into the `2k` values it
/// returns.
fn rec<N: Num>(t: &Tree, s: &[i8], v: u32, st: &mut Vec<N>) {
    let k = t.k;

    // s(v) = -2: the node is done and nothing is left to pay.
    if s[v as usize] == DONE {
        for _ in 0..2 * k {
            st.push(N::zero());
        }
        return;
    }

    let n = t.node(v);
    match n.kind {
        // A task owes its whole impact whatever its elapsed progress: the fast
        // forward pays it at the moment it completes and nothing before.
        Kind::Task => {
            let i = N::impact_of(t, v);
            st.extend(i.iter().cloned());
            st.extend(i);
        }

        // The first region of a sequence is spent as soon as the second one has
        // left -1, the up clause retiring it at the hand-over and no clause
        // restoring it.
        Kind::Sequence => {
            if s[n.high as usize] != IDLE {
                rec(t, s, n.high, st);
            } else {
                rec(t, s, n.low, st);
                rec(t, s, n.high, st);
                fold_sum(st, k);
            }
        }

        // A parallel node pays both of its branches, a branch that is done
        // contributing the zero vector by the first case.
        Kind::Parallel => {
            rec(t, s, n.low, st);
            rec(t, s, n.high, st);
            fold_sum(st, k);
        }

        Kind::Choice | Kind::Nature => {
            let lo = s[n.low as usize];
            let hi = s[n.high as usize];
            if lo >= 0 {
                // The branch has been taken already, and it is the only one that
                // is not idle, by the branch invariant.
                rec(t, s, n.low, st);
            } else if hi >= 0 {
                rec(t, s, n.high, st);
            } else if lo == IDLE && hi == IDLE {
                rec(t, s, n.low, st);
                rec(t, s, n.high, st);
                if n.kind == Kind::Choice {
                    fold_choice(st, k);
                } else {
                    let p = N::prob_of(t, v);
                    fold_nature(st, k, &p);
                }
            } else {
                // The taken branch is done and the closing move has not been
                // taken yet. Stated for both kinds of XOR node, so that the
                // recursion is total.
                for _ in 0..2 * k {
                    st.push(N::zero());
                }
            }
        }
    }
}

// Each fold reads the `4k` values the two children left on the stack, writes the
// `2k` values of the node over the first half, and drops the second: the stack
// grows by `2k` per node and by nothing else.

/// `X(v) = X(low) + X(high)`, on both bounds.
#[inline]
fn fold_sum<N: Num>(st: &mut Vec<N>, k: usize) {
    let base = st.len() - 4 * k;
    for j in 0..2 * k {
        st[base + j] = st[base + j].add(&st[base + 2 * k + j]);
    }
    st.truncate(base + 2 * k);
}

/// The one case where the two bounds part company: the componentwise maximum
/// for `U`, which no policy can exceed whichever branch it takes, and the
/// componentwise minimum for `L`, which no policy can undercut.
#[inline]
fn fold_choice<N: Num>(st: &mut Vec<N>, k: usize) {
    let base = st.len() - 4 * k;
    for j in 0..k {
        st[base + j] = st[base + j].max(&st[base + 2 * k + j]);
        st[base + k + j] = st[base + k + j].min(&st[base + 3 * k + j]);
    }
    st.truncate(base + 2 * k);
}

/// A nature node contributes the expectation of its branches, weighted by the
/// probability its low branch carries, in both bounds.
#[inline]
fn fold_nature<N: Num>(st: &mut Vec<N>, k: usize, p: &N::Prob) {
    let base = st.len() - 4 * k;
    for j in 0..2 * k {
        st[base + j] = N::expect(p, &st[base + j], &st[base + 2 * k + j]);
    }
    st.truncate(base + 2 * k);
}
