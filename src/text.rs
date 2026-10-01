//! The process as text, in the grammar of `parse`, and the six ways of
//! drawing its impacts again.
//!
//! An instance file holds the tree; the page holds the process as text. This
//! writes the text of a tree so that an instance of the benchmark can be put
//! before the user, who reads it, edits it and sends it back through the
//! parser, which assigns the identifiers afresh. A chain of sequences is
//! written flat, `( a , b , c )`, which the parser folds back to the left as
//! the chain was.
//!
//! The impacts can be drawn again with one of the six modes of the benchmark
//! of Workneh, Sala, Rizzi and Cristani, reproduced here in their meaning and
//! not in their numbers: the benchmark draws from the generator of numpy and
//! this from a generator of its own, so the vectors differ and the shapes of
//! the modes are the same.

use crate::num::decimal;
use crate::tree::{Kind, Tree};

pub const MODES: [&str; 6] = [
    "random",
    "bagging_divide",
    "bagging_remove",
    "bagging_remove_divide",
    "bagging_remove_reverse",
    "bagging_remove_reverse_divide",
];

/// A vector as the grammar writes it: six decimals at most, trailing zeros
/// gone, as the generator of the instances writes them.
fn number(v: f64) -> String {
    // six decimals, as the generator writes them; a value that six
    // decimals would print as zero keeps its exponent form, so that the
    // text read back has every impact the instance has
    if v != 0.0 && v.abs() < 5e-7 {
        return format!("{v:e}");
    }
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// The impact map of one task: the names with a value that is not zero.
fn map_text(names: &[String], values: &[String]) -> String {
    let parts: Vec<String> = values
        .iter()
        .enumerate()
        .filter(|(_, v)| v.as_str() != "0")
        .map(|(j, v)| format!("{}: {}", names[j], v))
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!(", {{{}}}", parts.join(", "))
    }
}

struct Writer<'a> {
    tree: &'a Tree,
    names: Vec<String>,
    /// Impacts to write in place of the ones of the tree, by node, or none.
    fresh: Option<&'a [Vec<f64>]>,
}

impl Writer<'_> {
    fn task(&self, id: u32) -> String {
        let n = self.tree.node(id);
        let name = self
            .tree
            .task_names
            .get(id as usize)
            .cloned()
            .unwrap_or_else(|| format!("T{id}"));
        let values: Vec<String> = match self.fresh {
            Some(f) => f[id as usize].iter().map(|&v| number(v)).collect(),
            None => self.tree.impact_exact(id).iter().map(|r| decimal(r, 6)).collect(),
        };
        format!("({}, {}{})", name, n.duration, map_text(&self.names, &values))
    }

    /// The regions of a chain of sequences, left to right.
    fn chain(&self, id: u32, out: &mut Vec<u32>) {
        let n = self.tree.node(id);
        if n.kind == Kind::Sequence {
            self.chain(n.low, out);
            out.push(n.high);
        } else {
            out.push(id);
        }
    }

    fn write(&self, id: u32, indent: usize) -> String {
        let n = self.tree.node(id);
        if n.kind == Kind::Task {
            return self.task(id);
        }
        let pad = " ".repeat(indent);
        if n.kind == Kind::Sequence {
            let mut regions = Vec::new();
            self.chain(id, &mut regions);
            let all_tasks = regions.iter().all(|&r| self.tree.node(r).kind == Kind::Task);
            let parts: Vec<String> = regions.iter().map(|&r| self.write(r, indent + 2)).collect();
            return if all_tasks {
                format!("( {} )", parts.join(" , "))
            } else {
                format!("( {} )", parts.join(&format!("\n{pad}, ")))
            };
        }
        let op = match n.kind {
            Kind::Parallel => "||".to_string(),
            Kind::Choice => "^".to_string(),
            _ => format!("^[{}]", decimal(self.tree.prob_exact(id), 6)),
        };
        let both_tasks = self.tree.node(n.low).kind == Kind::Task
            && self.tree.node(n.high).kind == Kind::Task;
        let low = self.write(n.low, indent + 2);
        let high = self.write(n.high, indent + 2 + op.len() + 1);
        if both_tasks {
            format!("( {low} {op} {high} )")
        } else {
            format!("( {low}\n{pad}{op} {high} )")
        }
    }
}

/// The text of the tree, with the impacts of `fresh` in place of its own
/// when given.
pub fn to_text(tree: &Tree, fresh: Option<&[Vec<f64>]>) -> String {
    let names: Vec<String> = if tree.meta.impact_names.len() == tree.k {
        tree.meta.impact_names.clone()
    } else {
        (0..tree.k).map(|j| format!("impact{j}")).collect()
    };
    let w = Writer { tree, names, fresh };
    let mut text = w.write(tree.root, 0);
    text.push('\n');
    text
}

/// A small generator, splitmix64, for the draws of the impacts.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in `[0, 1)`.
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Uniform in `0..n`.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// New impact vectors for every task of the tree, `dim` wide, by one of the
/// six modes, indexed by node; a node that is not a task gets none.
pub fn regenerate(tree: &Tree, dim: usize, mode: &str, seed: u64) -> Result<Vec<Vec<f64>>, String> {
    if !MODES.contains(&mode) {
        return Err(format!("no mode {mode}; the modes are {}", MODES.join(", ")));
    }
    if dim == 0 {
        return Err("at least one impact is needed".to_string());
    }
    let mut rng = Rng(seed ^ 0x5DEE_CE66_D1CE_4E5F);
    let mut out = vec![Vec::new(); tree.nodes.len()];
    for id in 0..tree.nodes.len() as u32 {
        if tree.node(id).kind != Kind::Task {
            continue;
        }
        let mut v: Vec<f64> = (0..dim).map(|_| rng.unit()).collect();
        if mode != "random" {
            let indexes: Vec<usize> = (0..dim).map(|_| rng.below(dim)).collect();
            match mode {
                "bagging_divide" => {
                    for &i in &indexes {
                        v[i] /= 10.0;
                    }
                }
                "bagging_remove" => {
                    let mut nv = vec![0.0; dim];
                    for &i in &indexes {
                        nv[i] = v[i];
                    }
                    v = nv;
                }
                "bagging_remove_divide" => {
                    let mut nv = vec![0.0; dim];
                    for &i in &indexes {
                        nv[i] = v[i] / 10.0;
                    }
                    v = nv;
                }
                "bagging_remove_reverse" => {
                    for &i in &indexes {
                        v[i] = 0.0;
                    }
                }
                _ => {
                    // bagging_remove_reverse_divide
                    for &i in &indexes {
                        v[i] = 0.0;
                    }
                    let nonzero: Vec<usize> = (0..dim).filter(|&j| v[j] != 0.0).collect();
                    if !nonzero.is_empty() {
                        for _ in 0..dim {
                            let j = nonzero[rng.below(nonzero.len())];
                            v[j] /= 10.0;
                        }
                    }
                }
            }
        }
        out[id as usize] = v.iter().map(|x| (x * 1e6).round() / 1e6).collect();
    }
    Ok(out)
}
