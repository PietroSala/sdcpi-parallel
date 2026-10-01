//! The tree of a BPMN+CPI process, and the reader of the instance files.
//!
//! The model gives the tree as
//! `T = (V, E_low u E_high, L, P, D, I)`. It is held here in one arena indexed
//! by the identifier of the node, which the instance files carry and which is
//! the in-order numbering of Definition 5: every node of the low subtree has a
//! smaller identifier than the node and every node of the high subtree a larger
//! one. The semantics picks the witness of a trigger as the holder of least
//! identifier, so indexing the arena by the identifier makes the scan for a
//! witness a scan in the order the definition asks for.
//!
//! The reader is written by hand rather than over a YAML library. The instance
//! format is rigid, one key per line in a fixed order, and two of its habits
//! break a generic reader: a node is a one-element block sequence whose kind is
//! carried by the presence of a key that also carries the payload
//! (`task: T12`, `nature: 0.859951`), and 181 220 of the numbers in the grid are
//! written in exponent form without a decimal point (`1e-06`), which YAML 1.1
//! does not classify as a float.

use std::fmt;
use std::path::Path;

use num_rational::BigRational;

use crate::num::parse_decimal;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Task,
    Sequence,
    Parallel,
    Choice,
    Nature,
}

impl Kind {
    pub fn is_xor(self) -> bool {
        matches!(self, Kind::Choice | Kind::Nature)
    }
}

/// One node of the tree. `low` and `high` are `0` at a task, `0` being no node.
#[derive(Clone, Debug)]
pub struct Node {
    pub kind: Kind,
    pub low: u32,
    pub high: u32,
    /// `D(v)` at a task, `0` elsewhere. Integer because every instance of the
    /// grid draws it in `[1, max(2, ceil(log2 t))]`, so it never exceeds 9.
    pub duration: i16,
    /// `P(v)` at a nature node, the probability of the **low** branch. `0.0`
    /// elsewhere, where it is never read.
    pub prob: f64,
}

/// The header of an instance file, kept whole so that a result can name the
/// instance it came from without the caller carrying the path around.
#[derive(Clone, Debug, Default)]
pub struct Meta {
    pub key: String,
    pub nested: u32,
    pub independent: u32,
    pub process_number: u32,
    pub dimensions: usize,
    /// The names of the impact components, in the order of the vectors;
    /// empty when the file does not carry them.
    pub impact_names: Vec<String>,
    pub mode: String,
    pub seed: u64,
    pub tasks: u32,
    pub nodes: u32,
    pub max_duration: u32,
    pub xor_root_kind: String,
    pub source: String,
    pub expression: String,
}

#[derive(Clone)]
pub struct Tree {
    /// One-based: `nodes[0]` is a filler so that `nodes[id]` reads the node of
    /// identifier `id`.
    pub nodes: Vec<Node>,
    pub root: u32,
    pub n_nodes: u32,
    pub n_tasks: u32,
    /// The dimension `k` of the impact vectors.
    pub k: usize,
    /// `(n_nodes + 1) * k` values, the impact of the task of identifier `id`
    /// sitting at `id * k`. The rows of the internal nodes are left at zero and
    /// never read.
    pub impacts: Vec<f64>,
    /// The same impacts as exact rationals, parsed from the decimal text of
    /// the file, one row of `k` per identifier as `impacts`.
    pub impacts_exact: Vec<BigRational>,
    /// The probability of the low branch of the nature node `id` at
    /// `probs_exact[id]`, as an exact rational; zero elsewhere, never read.
    pub probs_exact: Vec<BigRational>,
    /// The identifiers of the tasks, in increasing order.
    pub tasks: Vec<u32>,
    /// The identifiers of the XOR nodes, in increasing order.
    pub xors: Vec<u32>,
    /// The name of the task of identifier `id` at `task_names[id]`, the empty
    /// string at an internal node and at the filler.
    pub task_names: Vec<String>,
    pub meta: Meta,
    /// Whether the time is the component zero of the cost. When it is, `k`
    /// counts it, every impact vector carries a zero in front, the fast
    /// forward pays its advance there and the recursion of the bounds treats
    /// the component apart: see `with_time`.
    pub time: bool,
    /// Whether a component is maximized, one flag per component: its
    /// column of the impacts is negated, so that the engine minimizes it as
    /// every other, and the sign is turned back wherever a value is shown.
    /// The time maximized is paid as minus the advance of the fast forward,
    /// and its two bounds swap.
    pub maximized: Vec<bool>,
}

impl Tree {
    #[inline]
    pub fn node(&self, id: u32) -> &Node {
        &self.nodes[id as usize]
    }

    #[inline]
    pub fn kind(&self, id: u32) -> Kind {
        self.nodes[id as usize].kind
    }

    #[inline]
    pub fn impact(&self, id: u32) -> &[f64] {
        let a = id as usize * self.k;
        &self.impacts[a..a + self.k]
    }

    #[inline]
    pub fn impact_exact(&self, id: u32) -> &[BigRational] {
        let a = id as usize * self.k;
        &self.impacts_exact[a..a + self.k]
    }

    #[inline]
    pub fn prob_exact(&self, id: u32) -> &BigRational {
        &self.probs_exact[id as usize]
    }

    /// The identifiers of the nature nodes, in increasing order.
    pub fn natures(&self) -> Vec<u32> {
        self.xors.iter().copied().filter(|&v| self.kind(v) == Kind::Nature).collect()
    }

    /// The sum of the impacts of every task, which Corollary 3 gives as a bound
    /// on the value of every policy. Useful as a default threshold scale.
    pub fn total_impact(&self) -> Vec<f64> {
        let mut out = vec![0.0; self.k];
        for &t in &self.tasks {
            let i = self.impact(t);
            for c in 0..self.k {
                out[c] += i[c];
            }
        }
        out
    }

    /// The same process with the expected time as the component zero of the
    /// cost, in front of the impacts, which keep their order at `1..=k`.
    ///
    /// The time is not one more impact of the tasks: a task carries a zero
    /// there, and the component is paid by the fast forward, `t` at every
    /// advance, so that the cost of a run carries its time in front of its
    /// impacts. The recursion of the bounds reads the flag too, a task owing
    /// what is left of its duration and a parallel node the greater of its
    /// two regions from below. The names take `time` in front; an instance
    /// without names gets `impact0..` for its impacts first, so that a name
    /// keeps the index the instance gave the component.
    pub fn with_time(&self) -> Tree {
        if self.time {
            return self.clone();
        }
        let k = self.k + 1;
        let rows = self.nodes.len();
        let mut impacts = vec![0.0; rows * k];
        let mut impacts_exact = vec![BigRational::from_integer(0.into()); rows * k];
        for id in 0..rows {
            for c in 0..self.k {
                impacts[id * k + 1 + c] = self.impacts[id * self.k + c];
                impacts_exact[id * k + 1 + c] = self.impacts_exact[id * self.k + c].clone();
            }
        }
        let mut meta = self.meta.clone();
        if meta.impact_names.is_empty() {
            meta.impact_names = (0..self.k).map(|c| format!("impact{c}")).collect();
        }
        // the clock is named time, unless an impact took the name already:
        // then it is the expected time, so that a name says one component
        let clock = if meta.impact_names.iter().any(|n| n == "time") { "expected time" } else { "time" };
        meta.impact_names.insert(0, clock.to_string());
        meta.dimensions = k;
        let mut maximized = self.maximized.clone();
        maximized.insert(0, false);
        Tree {
            nodes: self.nodes.clone(),
            root: self.root,
            n_nodes: self.n_nodes,
            n_tasks: self.n_tasks,
            k,
            impacts,
            impacts_exact,
            probs_exact: self.probs_exact.clone(),
            tasks: self.tasks.clone(),
            xors: self.xors.clone(),
            task_names: self.task_names.clone(),
            meta,
            time: true,
            maximized,
        }
    }

    /// The same process with these components maximized, named by their
    /// impact name or their index: their columns are negated, so that the
    /// engine minimizes them, and `maximized` remembers which they are. The
    /// time, named once it is in, is maximized by its flag alone, the fast
    /// forward and the bounds reading it.
    pub fn with_directions(&self, names: &[String]) -> Result<Tree, String> {
        let mut tree = self.clone();
        for name in names {
            let n = name.trim();
            if n.is_empty() {
                continue;
            }
            let c = match tree.meta.impact_names.iter().position(|m| m == n) {
                Some(c) => c,
                None => match n.parse::<usize>() {
                    Ok(c) if c < tree.k => c,
                    _ => {
                        return Err(format!(
                            "no impact is named {n:?}; the instance declares [{}]",
                            tree.meta.impact_names.join(", ")
                        ))
                    }
                },
            };
            if tree.maximized[c] {
                continue;
            }
            tree.maximized[c] = true;
            let k = tree.k;
            for id in 0..tree.nodes.len() {
                tree.impacts[id * k + c] = -tree.impacts[id * k + c];
                tree.impacts_exact[id * k + c] = -tree.impacts_exact[id * k + c].clone();
            }
        }
        Ok(tree)
    }

    /// The same process with no direction: the negated columns turned back
    /// and every flag cleared, the instance as it was written, for the
    /// commands that print or draw it.
    pub fn undirected(&self) -> Tree {
        let mut tree = self.clone();
        let k = tree.k;
        for c in 0..k {
            if tree.maximized[c] {
                for id in 0..tree.nodes.len() {
                    tree.impacts[id * k + c] = -tree.impacts[id * k + c];
                    tree.impacts_exact[id * k + c] = -tree.impacts_exact[id * k + c].clone();
                }
                tree.maximized[c] = false;
            }
        }
        tree
    }

    /// The names of the maximized components.
    pub fn maximized_names(&self) -> Vec<String> {
        (0..self.k)
            .filter(|&c| self.maximized[c])
            .map(|c| match self.meta.impact_names.get(c) {
                Some(n) if !n.is_empty() => n.clone(),
                _ => c.to_string(),
            })
            .collect()
    }

    /// A value as it is shown: the sign turned back on a maximized component.
    pub fn shown_vec(&self, v: &[BigRational]) -> Vec<BigRational> {
        v.iter()
            .enumerate()
            .map(|(c, x)| if self.maximized.get(c).copied().unwrap_or(false) { -x.clone() } else { x.clone() })
            .collect()
    }

    /// Two bounds as they are shown: on a maximized component the upper
    /// bound of the negation is minus the lower bound of the value, and the
    /// other way round.
    pub fn shown_bounds(&self, upper: &[BigRational], lower: &[BigRational]) -> (Vec<BigRational>, Vec<BigRational>) {
        let mut u = Vec::with_capacity(upper.len());
        let mut l = Vec::with_capacity(lower.len());
        for c in 0..upper.len() {
            if self.maximized.get(c).copied().unwrap_or(false) {
                u.push(-lower[c].clone());
                l.push(-upper[c].clone());
            } else {
                u.push(upper[c].clone());
                l.push(lower[c].clone());
            }
        }
        (u, l)
    }

    /// The same on doubles.
    pub fn shown_bounds_f64(&self, upper: &[f64], lower: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let mut u = Vec::with_capacity(upper.len());
        let mut l = Vec::with_capacity(lower.len());
        for c in 0..upper.len() {
            if self.maximized.get(c).copied().unwrap_or(false) {
                u.push(-lower[c]);
                l.push(-upper[c]);
            } else {
                u.push(upper[c]);
                l.push(lower[c]);
            }
        }
        (u, l)
    }

    pub fn read(path: &Path) -> Result<Tree, ParseError> {
        // the caller names the path when it reports, so the error does not
        let text = std::fs::read_to_string(path)
            .map_err(|e| ParseError::new(0, e.to_string()))?;
        Tree::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Tree, ParseError> {
        Parser::new(text).run()
    }
}

#[derive(Debug)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl ParseError {
    fn new(line: usize, message: String) -> ParseError {
        ParseError { line, message }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.message)
        } else {
            write!(f, "line {}: {}", self.line, self.message)
        }
    }
}

impl std::error::Error for ParseError {}

// ---------------------------------------------------------------------------
// the reader
// ---------------------------------------------------------------------------

struct Parser<'a> {
    lines: Vec<&'a str>,
    at: usize,
    meta: Meta,
    // Filled as the tree is walked; `nodes[0]` is the filler.
    nodes: Vec<Node>,
    impacts: Vec<f64>,
    impacts_exact: Vec<BigRational>,
    probs_exact: Vec<BigRational>,
    names: Vec<String>,
}

/// The indentation of a line, in spaces, and its content with the indentation
/// stripped. A line of only spaces counts as blank.
fn split_indent(line: &str) -> (usize, &str) {
    let n = line.len() - line.trim_start_matches(' ').len();
    (n, &line[n..])
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Parser<'a> {
        Parser {
            lines: text.lines().collect(),
            at: 0,
            meta: Meta::default(),
            nodes: Vec::new(),
            impacts: Vec::new(),
            impacts_exact: Vec::new(),
            probs_exact: Vec::new(),
            names: Vec::new(),
        }
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError::new(self.at + 1, message.into()))
    }

    fn run(mut self) -> Result<Tree, ParseError> {
        // Line 1 is the comment carrying the key of the instance.
        if let Some(first) = self.lines.first() {
            if let Some(rest) = first.strip_prefix("# ") {
                self.meta.key = rest.trim().to_string();
                self.at = 1;
            }
        }
        self.header()?;

        // The header stops on `tree:`; the root follows.
        let n_nodes = self.meta.nodes as usize;
        let k = self.meta.dimensions;
        if k == 0 {
            return self.err("the header carries no dimensions");
        }
        self.nodes = vec![
            Node {
                kind: Kind::Task,
                low: 0,
                high: 0,
                duration: 0,
                prob: 0.0,
            };
            n_nodes + 1
        ];
        self.impacts = vec![0.0; (n_nodes + 1) * k];
        self.impacts_exact = vec![BigRational::from_integer(0.into()); (n_nodes + 1) * k];
        self.probs_exact = vec![BigRational::from_integer(0.into()); n_nodes + 1];
        self.names = vec![String::new(); n_nodes + 1];

        let root = self.node(0)?;

        let mut tree = Tree {
            nodes: std::mem::take(&mut self.nodes),
            root,
            n_nodes: self.meta.nodes,
            n_tasks: self.meta.tasks,
            k,
            impacts: std::mem::take(&mut self.impacts),
            impacts_exact: std::mem::take(&mut self.impacts_exact),
            probs_exact: std::mem::take(&mut self.probs_exact),
            tasks: Vec::new(),
            xors: Vec::new(),
            task_names: std::mem::take(&mut self.names),
            meta: std::mem::take(&mut self.meta),
            time: false,
            maximized: vec![false; k],
        };
        for id in 1..=tree.n_nodes {
            match tree.kind(id) {
                Kind::Task => tree.tasks.push(id),
                Kind::Choice | Kind::Nature => tree.xors.push(id),
                _ => {}
            }
        }
        self.check(&tree)?;
        Ok(tree)
    }

    fn header(&mut self) -> Result<(), ParseError> {
        while self.at < self.lines.len() {
            let line = self.lines[self.at];
            if line.trim().is_empty() {
                self.at += 1;
                continue;
            }
            let (indent, body) = split_indent(line);
            if indent != 0 {
                return self.err("a header key must sit at column zero");
            }
            if body == "tree:" {
                self.at += 1;
                return Ok(());
            }
            let (key, value) = match body.split_once(':') {
                Some((k, v)) => (k.trim(), v.trim()),
                None => return self.err(format!("no key in {body:?}")),
            };
            let num = |p: &Parser<'a>, v: &str| -> Result<u64, ParseError> {
                v.parse::<u64>()
                    .map_err(|_| ParseError::new(p.at + 1, format!("{v:?} is not a number")))
            };
            match key {
                "nested" => self.meta.nested = num(self, value)? as u32,
                "independent" => self.meta.independent = num(self, value)? as u32,
                "process_number" => self.meta.process_number = num(self, value)? as u32,
                "dimensions" => self.meta.dimensions = num(self, value)? as usize,
                "impact_names" => {
                    self.meta.impact_names = value
                        .trim_start_matches('[')
                        .trim_end_matches(']')
                        .split(',')
                        .map(|n| n.trim().to_string())
                        .filter(|n| !n.is_empty())
                        .collect();
                }
                "mode" => self.meta.mode = value.to_string(),
                "seed" => self.meta.seed = num(self, value)?,
                "tasks" => self.meta.tasks = num(self, value)? as u32,
                "nodes" => self.meta.nodes = num(self, value)? as u32,
                "max_duration" => self.meta.max_duration = num(self, value)? as u32,
                "xor_root_kind" => self.meta.xor_root_kind = value.to_string(),
                "source" => self.meta.source = value.to_string(),
                "expression" => {
                    self.meta.expression = value.trim_matches('"').to_string();
                }
                other => return self.err(format!("unknown header key {other:?}")),
            }
            self.at += 1;
        }
        self.err("the file ends before `tree:`")
    }

    /// Reads one node, whose opening line `- <kind>:` sits at an indentation
    /// greater than `parent_indent`, and returns its identifier.
    fn node(&mut self, parent_indent: usize) -> Result<u32, ParseError> {
        self.skip_blank();
        if self.at >= self.lines.len() {
            return self.err("the file ends where a node was expected");
        }
        let (indent, body) = split_indent(self.lines[self.at]);
        if indent <= parent_indent && parent_indent != 0 {
            return self.err("a node is indented no deeper than its parent");
        }
        let head = match body.strip_prefix("- ") {
            Some(h) => h,
            None => return self.err(format!("a node must open on `- `, found {body:?}")),
        };
        let (kind_key, kind_value) = match head.split_once(':') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => return self.err(format!("no kind in {head:?}")),
        };
        let kind = match kind_key {
            "task" => Kind::Task,
            "sequence" => Kind::Sequence,
            "parallel" => Kind::Parallel,
            "choice" => Kind::Choice,
            "nature" => Kind::Nature,
            other => return self.err(format!("unknown node kind {other:?}")),
        };
        let task_name = if kind == Kind::Task {
            kind_value.to_string()
        } else {
            String::new()
        };
        let (prob, prob_exact) = if kind == Kind::Nature {
            let p: f64 = kind_value
                .parse()
                .map_err(|_| ParseError::new(self.at + 1, format!("{kind_value:?} is not a probability")))?;
            if !(p > 0.0 && p < 1.0) {
                return self.err(format!(
                    "a nature node carries {p}, and Definition 3 asks for a probability in (0,1)"
                ));
            }
            let exact = match parse_decimal(kind_value) {
                Some(r) => r,
                None => return self.err(format!("{kind_value:?} is not a decimal probability")),
            };
            (p, Some(exact))
        } else {
            (0.0, None)
        };
        self.at += 1;

        // The fields of the node sit two columns deeper than its opening line.
        let field_indent = indent + 2;
        let mut id: Option<u32> = None;
        let mut duration: i16 = 0;
        let mut impact: Option<Vec<f64>> = None;
        let mut impact_exact: Option<Vec<BigRational>> = None;
        let mut low = 0u32;
        let mut high = 0u32;

        loop {
            self.skip_blank();
            if self.at >= self.lines.len() {
                break;
            }
            let (i, body) = split_indent(self.lines[self.at]);
            if i < field_indent {
                break;
            }
            let (key, value) = match body.split_once(':') {
                Some((k, v)) => (k.trim(), v.trim()),
                None => return self.err(format!("no key in {body:?}")),
            };
            match key {
                "id" => {
                    id = Some(value.parse().map_err(|_| {
                        ParseError::new(self.at + 1, format!("{value:?} is not an identifier"))
                    })?);
                    self.at += 1;
                }
                "duration" => {
                    let d: i32 = value.parse().map_err(|_| {
                        ParseError::new(self.at + 1, format!("{value:?} is not a duration"))
                    })?;
                    if d < 1 {
                        return self.err(format!("a task carries the duration {d}, which is not positive"));
                    }
                    // The state holds the elapsed progress of a task in an `i8`,
                    // whose room is what keeps a choice state one byte per node.
                    // Every instance of the grid draws a duration in
                    // `[1, max(2, ceil(log2 t))]`, so the largest is 9.
                    if d > i8::MAX as i32 {
                        return self.err(format!(
                            "the duration {d} does not fit the state encoding, which holds a progress in an i8"
                        ));
                    }
                    duration = d as i16;
                    self.at += 1;
                }
                "impact" => {
                    impact = Some(self.vector(value)?);
                    impact_exact = Some(self.vector_exact(value)?);
                    self.at += 1;
                }
                "low" => {
                    self.at += 1;
                    low = self.node(i)?;
                }
                "high" => {
                    self.at += 1;
                    high = self.node(i)?;
                }
                other => return self.err(format!("unknown node key {other:?}")),
            }
        }

        let id = match id {
            Some(v) => v,
            None => return self.err("a node carries no identifier"),
        };
        if id == 0 || id as usize >= self.nodes.len() {
            return self.err(format!(
                "the identifier {id} falls outside 1..{}, which the header declares",
                self.nodes.len() - 1
            ));
        }
        if kind == Kind::Task {
            let vector = match impact {
                Some(v) => v,
                None => return self.err("a task carries no impact"),
            };
            if vector.len() != self.meta.dimensions {
                return self.err(format!(
                    "an impact of {} components against {} declared dimensions",
                    vector.len(),
                    self.meta.dimensions
                ));
            }
            if duration == 0 {
                return self.err("a task carries no duration");
            }
            let a = id as usize * self.meta.dimensions;
            self.impacts[a..a + self.meta.dimensions].copy_from_slice(&vector);
            let exact = impact_exact.unwrap_or_default();
            for (c, r) in exact.into_iter().enumerate() {
                self.impacts_exact[a + c] = r;
            }
        } else if low == 0 || high == 0 {
            return self.err("an internal node is missing a branch");
        }

        self.names[id as usize] = task_name;
        if let Some(r) = prob_exact {
            self.probs_exact[id as usize] = r;
        }
        self.nodes[id as usize] = Node {
            kind,
            low,
            high,
            duration,
            prob,
        };
        Ok(id)
    }

    fn vector(&self, value: &str) -> Result<Vec<f64>, ParseError> {
        let inner = value
            .trim()
            .strip_prefix('[')
            .and_then(|v| v.strip_suffix(']'));
        let inner = match inner {
            Some(v) => v,
            None => return Err(ParseError::new(self.at + 1, format!("{value:?} is not a vector"))),
        };
        let mut out = Vec::with_capacity(self.meta.dimensions);
        for piece in inner.split(',') {
            let piece = piece.trim();
            if piece.is_empty() {
                continue;
            }
            // `f64::from_str` takes the exponent form without a decimal point,
            // `1e-06`, which is where a YAML 1.1 reader hands back a string.
            let x: f64 = piece
                .parse()
                .map_err(|_| ParseError::new(self.at + 1, format!("{piece:?} is not a number")))?;
            if !(x >= 0.0) {
                return Err(ParseError::new(
                    self.at + 1,
                    format!("the impact {x} is negative, and Definition 3 asks for a vector of R>=0^k"),
                ));
            }
            out.push(x);
        }
        Ok(out)
    }

    fn vector_exact(&self, value: &str) -> Result<Vec<BigRational>, ParseError> {
        let inner = value
            .trim()
            .strip_prefix('[')
            .and_then(|v| v.strip_suffix(']'))
            .ok_or_else(|| ParseError::new(self.at + 1, format!("{value:?} is not a vector")))?;
        let mut out = Vec::with_capacity(self.meta.dimensions);
        for piece in inner.split(',') {
            let piece = piece.trim();
            if piece.is_empty() {
                continue;
            }
            match parse_decimal(piece) {
                Some(r) => out.push(r),
                None => {
                    return Err(ParseError::new(
                        self.at + 1,
                        format!("{piece:?} is not a decimal number"),
                    ))
                }
            }
        }
        Ok(out)
    }

    fn skip_blank(&mut self) {
        while self.at < self.lines.len() && self.lines[self.at].trim().is_empty() {
            self.at += 1;
        }
    }

    /// The invariants the instance files carry, checked rather than assumed: a
    /// reader that trusts them silently turns a malformed file into a wrong
    /// answer, and the numbers of this binary are recorded and compared.
    fn check(&self, tree: &Tree) -> Result<(), ParseError> {
        if tree.tasks.len() as u32 != tree.n_tasks {
            return Err(ParseError::new(
                0,
                format!(
                    "the header declares {} tasks and the tree holds {}",
                    tree.n_tasks,
                    tree.tasks.len()
                ),
            ));
        }
        if tree.n_nodes != 2 * tree.n_tasks - 1 {
            return Err(ParseError::new(
                0,
                format!(
                    "{} nodes against {} tasks, and a full binary tree has 2t-1",
                    tree.n_nodes, tree.n_tasks
                ),
            ));
        }
        // Every identifier is used exactly once, and the numbering is in-order:
        // `low` subtree < node < `high` subtree. Checked by one walk, which also
        // catches a cycle, the walk visiting every identifier once.
        let mut seen = vec![false; tree.n_nodes as usize + 1];
        let mut stack = vec![(tree.root, 1u32, tree.n_nodes)];
        let mut visited = 0u32;
        while let Some((id, lo, hi)) = stack.pop() {
            if id < lo || id > hi {
                return Err(ParseError::new(
                    0,
                    format!("the identifier {id} falls outside the in-order window {lo}..{hi}"),
                ));
            }
            if seen[id as usize] {
                return Err(ParseError::new(0, format!("the identifier {id} is used twice")));
            }
            seen[id as usize] = true;
            visited += 1;
            let n = tree.node(id);
            if n.kind == Kind::Task {
                if lo != id || hi != id {
                    return Err(ParseError::new(
                        0,
                        format!("the task {id} does not fill its in-order window {lo}..{hi}"),
                    ));
                }
            } else {
                stack.push((n.low, lo, id - 1));
                stack.push((n.high, id + 1, hi));
            }
        }
        if visited != tree.n_nodes {
            return Err(ParseError::new(
                0,
                format!("the walk reaches {visited} nodes of the {} declared", tree.n_nodes),
            ));
        }
        Ok(())
    }
}
