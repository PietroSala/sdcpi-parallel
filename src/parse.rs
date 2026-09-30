//! The grammar of a process, and the writer of the instance file.
//!
//!     process ::= region ("," region)* | region op region
//!     region  ::= task | "(" process ")" | "@" saved ("[" shown "]")?
//!     op      ::= "||" | "^" | "^[" prob "]"
//!     task    ::= "(" name "," duration ")"
//!             |   "(" name "," duration "," "{" impacts? "}" ")"
//!     impacts ::= name ":" number ("," name ":" number)*
//!
//! `,` between regions is a sequence and associates to the left: `A, B, C`
//! reads `(A, B), C`, the tree staying binary, and the chain needs no
//! parentheses of its own; the comma is unambiguous because a task is
//! recognised by the name after its parenthesis and the commas of the tuple
//! are consumed inside it. `||` is a parallel composition, `^` a choice and
//! `^[p]` a nature node whose left operand is taken with probability `p` in
//! (0,1); each of these three carries its own parentheses, one operator per
//! pair, so no precedence has to be fixed, and mixing the sequence comma
//! with another operator demands the parentheses too. The outermost
//! parentheses of the whole term are optional. A duration is a
//! positive integer. The impact map of a task names only the impacts that are
//! strictly positive; a task may omit the map, or write `{}`, both meaning
//! every impact zero. The names are collected over the whole process in the
//! order of their first appearance, and that order is the meaning of the
//! vectors: `impact_names` in the emitted file lists them, and entry `i` of
//! every `impact` vector is the value of name `i`, zero where the task does
//! not name it. `#` starts a comment that runs to the end of the line.
//!
//! Identifiers are assigned by the in-order traversal, one-based: every node
//! of the low subtree numbers below its parent, every node of the high
//! subtree above, which is the numbering the reader of `tree.rs` checks.
//!
//! A reference `@saved` stands for a saved process, spliced in place as a
//! region, which the single-entry single-exit shape of every process allows;
//! `@saved[shown]` shows it under another name. The saved text may refer to
//! other saved processes in turn, to any depth, and never to itself through
//! any chain: a cycle is an error. The impacts are merged by name, one table
//! over the whole assembly. The engine sees the assembled process, with its
//! identifiers in-order over the whole; beside them every node gets the
//! identifier it shows, the own identifier in the process that wrote it
//! prefixed by the identifiers of the references that led there, `7-3` for
//! the node 3 of the process the leaf 7 refers to. Who resolves a name is
//! the caller, through `to_yaml_with`; `to_yaml` resolves nothing and fails
//! on a reference.

/// One node of the parsed process, before identifiers exist.
enum Ast {
    Task {
        name: String,
        duration: i64,
        /// Indices into the collected name table, with the value of each.
        impact: Vec<(usize, f64)>,
    },
    Op {
        kind: &'static str,
        prob: Option<f64>,
        low: Box<Ast>,
        high: Box<Ast>,
    },
    /// A saved process spliced in place of a reference: one leaf of the
    /// process that names it, a whole subtree of the assembled one.
    Sub {
        name: String,
        shown: String,
        inner: Box<Ast>,
    },
}

#[derive(Debug)]
pub struct Parsed {
    pub yaml: String,
    /// The identifier every node shows, by assembled identifier, index 0 a
    /// filler: the own identifier of the node in the process that wrote it,
    /// prefixed by the identifiers of the references that led there.
    pub labels: Vec<String>,
    /// The saved processes spliced in, parents before their children.
    pub subs: Vec<Sub>,
}

/// One saved process spliced into the assembly.
#[derive(Clone, Debug)]
pub struct Sub {
    /// The assembled identifier of its root.
    pub root: u32,
    /// The identifier it shows, that of the leaf that refers to it.
    pub label: String,
    /// The name it is saved under.
    pub name: String,
    /// The name it is shown under, the saved name unless `[shown]` was given.
    pub shown: String,
    /// How many references lead to it, zero for one of the top process.
    pub depth: usize,
}

/// What a parse carries beside the scanner: the table of the impact names,
/// one over every process spliced in so that one name is one impact
/// everywhere, the resolver of a reference, and the chain of the references
/// being resolved, against a cycle.
struct Ctx<'r> {
    names: Vec<String>,
    resolver: Option<&'r mut dyn FnMut(&str) -> Result<String, String>>,
    chain: Vec<String>,
}

struct Scanner<'a> {
    text: &'a [u8],
    at: usize,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str) -> Self {
        Scanner { text: text.as_bytes(), at: 0 }
    }

    fn skip(&mut self) {
        loop {
            while self.at < self.text.len() && self.text[self.at].is_ascii_whitespace() {
                self.at += 1;
            }
            if self.at < self.text.len() && self.text[self.at] == b'#' {
                while self.at < self.text.len() && self.text[self.at] != b'\n' {
                    self.at += 1;
                }
            } else {
                return;
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.text.get(self.at).copied()
    }

    fn eat(&mut self, c: u8) -> Result<(), String> {
        self.skip();
        if self.text.get(self.at) == Some(&c) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!(
                "expected {:?} at byte {}, found {}",
                c as char,
                self.at,
                self.found(self.at)
            ))
        }
    }

    /// What sits at a byte position, for an error message: the whole UTF-8
    /// character, or the end of the input.
    fn found(&self, at: usize) -> String {
        match self.text.get(at) {
            None => "the end of the input".to_string(),
            Some(_) => {
                let end = (at + 4).min(self.text.len());
                let c = String::from_utf8_lossy(&self.text[at..end])
                    .chars()
                    .next()
                    .unwrap_or('\u{FFFD}');
                format!("{c:?}")
            }
        }
    }

    fn name(&mut self) -> Result<String, String> {
        self.skip();
        let start = self.at;
        while self.at < self.text.len()
            && (self.text[self.at].is_ascii_alphanumeric() || self.text[self.at] == b'_')
        {
            self.at += 1;
        }
        if self.at == start {
            return Err(format!("expected a name at byte {start}"));
        }
        Ok(String::from_utf8_lossy(&self.text[start..self.at]).into_owned())
    }

    /// The name of a saved process after `@`: letters, digits, `_`, `.` and
    /// `-`, or anything at all between double quotes.
    fn refname(&mut self) -> Result<String, String> {
        let start = self.at;
        if self.text.get(self.at) == Some(&b'"') {
            self.at += 1;
            let from = self.at;
            while self.at < self.text.len() && self.text[self.at] != b'"' {
                self.at += 1;
            }
            if self.at >= self.text.len() {
                return Err(format!("the quoted name at byte {start} never closes"));
            }
            let name = String::from_utf8_lossy(&self.text[from..self.at]).trim().to_string();
            self.at += 1;
            if name.is_empty() {
                return Err(format!("the quoted name at byte {start} is empty"));
            }
            return Ok(name);
        }
        while self.at < self.text.len()
            && (self.text[self.at].is_ascii_alphanumeric()
                || matches!(self.text[self.at], b'_' | b'.' | b'-'))
        {
            self.at += 1;
        }
        if self.at == start {
            return Err(format!("expected the name of a saved process after @ at byte {start}"));
        }
        Ok(String::from_utf8_lossy(&self.text[start..self.at]).into_owned())
    }

    /// The text up to the closing bracket of `[shown]`, the bracket consumed.
    fn until_bracket(&mut self) -> Result<String, String> {
        let start = self.at;
        while self.at < self.text.len() && self.text[self.at] != b']' && self.text[self.at] != b'\n' {
            self.at += 1;
        }
        if self.text.get(self.at) != Some(&b']') {
            return Err(format!("the [shown] name opened at byte {start} never closes on its line"));
        }
        let shown = String::from_utf8_lossy(&self.text[start..self.at]).trim().to_string();
        self.at += 1;
        Ok(shown)
    }

    fn number(&mut self) -> Result<f64, String> {
        self.skip();
        let start = self.at;
        while self.at < self.text.len()
            && (self.text[self.at].is_ascii_digit()
                || matches!(self.text[self.at], b'.' | b'e' | b'E' | b'+' | b'-'))
        {
            // a minus is part of an exponent alone; the grammar has no
            // negative quantities
            if self.text[self.at] == b'-'
                && !matches!(self.text.get(self.at.wrapping_sub(1)), Some(b'e') | Some(b'E'))
            {
                break;
            }
            self.at += 1;
        }
        let s = std::str::from_utf8(&self.text[start..self.at]).unwrap_or("");
        s.parse::<f64>().map_err(|_| {
            if s.is_empty() {
                format!("expected a number at byte {start}, found {}", self.found(start))
            } else {
                format!("expected a number at byte {start}, found {s:?}")
            }
        })
    }

    /// A positive integer written in digits alone: no sign, no point, no
    /// exponent, and it must fit, so nothing is rounded or saturated behind
    /// the writer of the text.
    fn integer(&mut self) -> Result<i64, String> {
        self.skip();
        let start = self.at;
        while self.at < self.text.len() && self.text[self.at].is_ascii_digit() {
            self.at += 1;
        }
        if self.at == start {
            return Err(format!(
                "expected an integer at byte {start}, found {}",
                self.found(start)
            ));
        }
        if matches!(self.text.get(self.at), Some(b'.') | Some(b'e') | Some(b'E')) {
            return Err(format!(
                "the integer at byte {start} carries a point or an exponent, and a duration is written in digits alone"
            ));
        }
        std::str::from_utf8(&self.text[start..self.at])
            .unwrap_or("")
            .parse::<i64>()
            .map_err(|_| format!("the integer at byte {start} does not fit in 64 bits"))
    }
}

/// The whole term, parentheses and sequence chain together, may reach this
/// deep and no deeper: the traversals of the tree recurse, and a bound here
/// turns what would abort the process into an error message.
const MAX_DEPTH: usize = 2000;

fn parse_process(
    sc: &mut Scanner,
    ctx: &mut Ctx<'_>,
    depth: usize,
) -> Result<Ast, String> {
    let mut low = parse_region(sc, ctx, depth)?;
    sc.skip();
    if sc.text.get(sc.at) == Some(&b',') {
        // a chain of sequences, folded to the left; every fold deepens the
        // left spine of the tree, so the folds count against the depth too
        let mut folds = 0usize;
        while sc.text.get(sc.at) == Some(&b',') {
            sc.at += 1;
            folds += 1;
            if depth + folds > MAX_DEPTH {
                return Err(format!("the term is deeper than {MAX_DEPTH}, parentheses and sequence folds together"));
            }
            let high = parse_region(sc, ctx, depth + folds)?;
            low = Ast::Op {
                kind: "sequence",
                prob: None,
                low: Box::new(low),
                high: Box::new(high),
            };
            sc.skip();
        }
        return Ok(low);
    }
    let (kind, prob) = if sc.text.get(sc.at..sc.at + 2) == Some(b"||") {
        sc.at += 2;
        ("parallel", None)
    } else if sc.text.get(sc.at) == Some(&b'^') {
        sc.at += 1;
        if sc.text.get(sc.at) == Some(&b'[') {
            sc.at += 1;
            let p = sc.number()?;
            if !(p > 0.0 && p < 1.0) {
                return Err(format!("a nature node carries {p}, and a probability lies in (0,1)"));
            }
            sc.eat(b']')?;
            ("nature", Some(p))
        } else {
            ("choice", None)
        }
    } else {
        // a single region: the whole process may be one task, and a
        // parenthesised region may wrap another
        return Ok(low);
    };
    let high = parse_region(sc, ctx, depth)?;
    Ok(Ast::Op { kind, prob, low: Box::new(low), high: Box::new(high) })
}

/// A reference `@saved` or `@saved[shown]`: the saved text is parsed in
/// place, through the resolver, its impacts into the one table, its
/// references resolved in turn.
fn parse_reference(sc: &mut Scanner, ctx: &mut Ctx<'_>, depth: usize) -> Result<Ast, String> {
    sc.eat(b'@')?;
    let name = sc.refname()?;
    let shown = if sc.text.get(sc.at) == Some(&b'[') {
        sc.at += 1;
        let shown = sc.until_bracket()?;
        if shown.is_empty() { name.clone() } else { shown }
    } else {
        name.clone()
    };
    // a reference carries neither a duration nor impacts, it takes them from
    // the saved process: `(@name, 3, {...})` is refused with the reason and
    // not with a puzzle about a parenthesis
    let mark = sc.at;
    sc.skip();
    if sc.text.get(sc.at) == Some(&b',') {
        let mut j = sc.at + 1;
        while j < sc.text.len() && sc.text[j].is_ascii_whitespace() {
            j += 1;
        }
        if sc.text.get(j).map_or(false, |c| c.is_ascii_digit()) {
            return Err(format!(
                "@{name} is followed by a duration, and a reference carries neither a duration nor impacts, taking both from the saved process: write @{name} alone"
            ));
        }
    }
    sc.at = mark;
    if ctx.chain.iter().any(|c| c == &name) {
        let mut cycle = ctx.chain.clone();
        cycle.push(name.clone());
        return Err(format!("the references form a cycle: {}", cycle.join(" > ")));
    }
    let text = match ctx.resolver.as_mut() {
        Some(resolve) => resolve(&name)?,
        None => {
            return Err(format!(
                "the process refers to @{name}, and a reference is resolved against the saved processes of the page alone"
            ))
        }
    };
    ctx.chain.push(name.clone());
    let mut inner = Scanner::new(&text);
    let ast = parse_process(&mut inner, ctx, depth + 1)
        .map_err(|e| format!("in the saved process {name}: {e}"))?;
    inner.skip();
    if inner.at != inner.text.len() {
        return Err(format!("in the saved process {name}: trailing input at byte {}", inner.at));
    }
    ctx.chain.pop();
    Ok(Ast::Sub { name, shown, inner: Box::new(ast) })
}

fn parse_region(
    sc: &mut Scanner,
    ctx: &mut Ctx<'_>,
    depth: usize,
) -> Result<Ast, String> {
    if depth > MAX_DEPTH {
        return Err(format!("the term is deeper than {MAX_DEPTH}, parentheses and sequence folds together"));
    }
    // a reference is a region of its own, with no parentheses of its own
    if sc.peek() == Some(b'@') {
        return parse_reference(sc, ctx, depth);
    }
    sc.eat(b'(')?;
    // a task opens on a name, a wrapped process on another parenthesis or on
    // a reference
    if matches!(sc.peek(), Some(b'(') | Some(b'@')) {
        let inner = parse_process(sc, ctx, depth + 1)?;
        sc.eat(b')')?;
        return Ok(inner);
    }
    // a task: name, duration, and an optional impact map
    let name = sc.name()?;
    sc.eat(b',')?;
    let d = sc.integer()?;
    if d <= 0 {
        return Err(format!("the task {name} carries the duration {d}, and a duration is a positive integer"));
    }
    let mut impact: Vec<(usize, f64)> = Vec::new();
    sc.skip();
    if sc.peek() == Some(b',') {
        sc.eat(b',')?;
        sc.eat(b'{')?;
        if sc.peek() != Some(b'}') {
            loop {
                let iname = sc.name()?;
                sc.eat(b':')?;
                let v = sc.number()?;
                if v <= 0.0 {
                    return Err(format!(
                        "the impact {iname} of {name} is {v}, and the map names only what is strictly positive; leave a zero out"
                    ));
                }
                let idx = match ctx.names.iter().position(|n| n == &iname) {
                    Some(i) => i,
                    None => {
                        ctx.names.push(iname.clone());
                        ctx.names.len() - 1
                    }
                };
                if impact.iter().any(|(i, _)| *i == idx) {
                    return Err(format!("the task {name} names the impact {iname} twice"));
                }
                impact.push((idx, v));
                if sc.peek() == Some(b',') {
                    sc.eat(b',')?;
                } else {
                    break;
                }
            }
        }
        sc.eat(b'}')?;
    }
    sc.eat(b')')?;
    Ok(Ast::Task { name, duration: d, impact })
}

fn count(ast: &Ast) -> (u32, u32) {
    match ast {
        Ast::Sub { inner, .. } => count(inner),
        Ast::Task { .. } => (1, 1),
        Ast::Op { low, high, .. } => {
            let (nl, tl) = count(low);
            let (nh, th) = count(high);
            (nl + nh + 1, tl + th)
        }
    }
}

fn fmt_number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn emit(ast: &Ast, k: usize, next: &mut u32, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    match ast {
        // a spliced process is its tree: the engine never sees the seam
        Ast::Sub { inner, .. } => emit(inner, k, next, indent, out),
        Ast::Task { name, duration, impact } => {
            let mut v = vec![0.0f64; k];
            for (i, x) in impact {
                v[*i] = *x;
            }
            let list: Vec<String> = v.iter().map(|x| fmt_number(*x)).collect();
            out.push_str(&format!("{pad}- task: {name}\n"));
            out.push_str(&format!("{pad}  id: {}\n", next_id(next)));
            out.push_str(&format!("{pad}  duration: {duration}\n"));
            out.push_str(&format!("{pad}  impact: [{}]\n", list.join(", ")));
        }
        Ast::Op { kind, prob, low, high } => {
            // in-order: the low subtree first, the node, the high subtree;
            // the node line prints before its children, so its identifier is
            // reserved by counting the low subtree first
            let (low_nodes, _) = count(low);
            let my_id = *next + low_nodes + 1;
            match prob {
                Some(p) => out.push_str(&format!("{pad}- {kind}: {p}\n")),
                None => out.push_str(&format!("{pad}- {kind}:\n")),
            }
            out.push_str(&format!("{pad}  id: {my_id}\n"));
            out.push_str(&format!("{pad}  low:\n"));
            emit(low, k, next, indent + 2, out);
            *next += 1; // the node itself, between the two subtrees
            out.push_str(&format!("{pad}  high:\n"));
            emit(high, k, next, indent + 2, out);
        }
    }
}

fn next_id(next: &mut u32) -> u32 {
    *next += 1;
    *next
}

/// The identifier every node shows and the sub-processes spliced in, by one
/// walk that numbers the assembled tree exactly as `emit` does, in-order,
/// and beside it every process by its own nodes, a spliced process counting
/// one leaf of the process that refers to it.
fn walk(
    ast: &Ast,
    next: &mut u32,
    own: &mut u32,
    prefix: &str,
    depth: usize,
    labels: &mut Vec<String>,
    subs: &mut Vec<Sub>,
) -> u32 {
    match ast {
        Ast::Task { .. } => {
            *next += 1;
            *own += 1;
            labels[*next as usize] = format!("{prefix}{own}");
            *next
        }
        Ast::Op { low, high, .. } => {
            walk(low, next, own, prefix, depth, labels, subs);
            *next += 1;
            *own += 1;
            let id = *next;
            labels[id as usize] = format!("{prefix}{own}");
            walk(high, next, own, prefix, depth, labels, subs);
            id
        }
        Ast::Sub { name, shown, inner } => {
            *own += 1;
            let label = format!("{prefix}{own}");
            let at = subs.len();
            subs.push(Sub {
                root: 0,
                label: label.clone(),
                name: name.clone(),
                shown: shown.clone(),
                depth,
            });
            let mut inner_own = 0u32;
            let root = walk(inner, next, &mut inner_own, &format!("{label}-"), depth + 1, labels, subs);
            subs[at].root = root;
            root
        }
    }
}

fn parse_all(
    text: &str,
    name: Option<&str>,
    resolver: Option<&mut dyn FnMut(&str) -> Result<String, String>>,
) -> Result<Parsed, String> {
    let mut sc = Scanner::new(text);
    // the process being written, when it has a name, heads the chain: a
    // reference to its own name is a cycle at once, saved or not
    let chain = name.map(|n| vec![n.to_string()]).unwrap_or_default();
    let mut ctx = Ctx { names: Vec::new(), resolver, chain };
    let ast = parse_process(&mut sc, &mut ctx, 0)?;
    sc.skip();
    if sc.at != sc.text.len() {
        return Err(format!("trailing input at byte {}", sc.at));
    }
    let names = ctx.names;
    if names.is_empty() {
        return Err("no impact is named anywhere; name at least one, the dimension being at least one".to_string());
    }
    let (nodes, tasks) = count(&ast);
    let k = names.len();
    let mut out = String::new();
    out.push_str(&format!("impact_names: [{}]\n", names.join(", ")));
    out.push_str(&format!("dimensions: {k}\n"));
    out.push_str(&format!("tasks: {tasks}\n"));
    out.push_str(&format!("nodes: {nodes}\n"));
    out.push_str("tree:\n");
    let mut next = 0u32;
    emit(&ast, k, &mut next, 1, &mut out);
    let mut labels = vec![String::new(); nodes as usize + 1];
    let mut subs = Vec::new();
    let (mut next, mut own) = (0u32, 0u32);
    walk(&ast, &mut next, &mut own, "", 0, &mut labels, &mut subs);
    debug_assert_eq!(next, nodes);
    Ok(Parsed { yaml: out, labels, subs })
}

/// Parses the grammar and writes the instance file, identifiers in-order and
/// one-based, `impact_names` in the order of first appearance. A reference
/// to a saved process is an error here: see `to_yaml_with`.
pub fn to_yaml(text: &str) -> Result<Parsed, String> {
    parse_all(text, None, None)
}

/// The same, with every reference `@saved` resolved through `resolver`, which
/// gives the text saved under a name, and spliced in place.
pub fn to_yaml_with(
    text: &str,
    resolver: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Result<Parsed, String> {
    parse_all(text, None, Some(resolver))
}

/// The same for a process being written under `name`: a reference to that
/// name is a cycle at once, whatever the store holds under it.
pub fn to_yaml_as(
    text: &str,
    name: &str,
    resolver: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Result<Parsed, String> {
    parse_all(text, Some(name), Some(resolver))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> Result<String, String> {
        match name {
            "Repair" => Ok("( (Mend, 3, {euro: 20}) ^ (Redo, 2, {euro: 40, hours: 6}) )".to_string()),
            "Ship" => Ok("(Pack, 1, {hours: 1}) , @Repair[Again]".to_string()),
            "Loop" => Ok("(A, 1, {x: 1}) , @Loop".to_string()),
            _ => Err(format!("no process is saved as {name:?}")),
        }
    }

    #[test]
    fn a_reference_splices_the_saved_process_and_labels_its_nodes() {
        let mut r = store;
        let p = to_yaml_with("( (Intake, 1, {euro: 10}) , @Repair[Fix] , @Ship )", &mut r).unwrap();
        // the assembled tree: Intake(1) seq(2) [Mend(3) choice(4) Redo(5)] seq(6) [Pack(7) seq(8) [Mend(9) choice(10) Redo(11)]]
        assert!(p.yaml.contains("nodes: 11"));
        assert_eq!(p.labels[1], "1");
        assert_eq!(p.labels[2], "2");
        assert_eq!(p.labels[3], "3-1");
        assert_eq!(p.labels[4], "3-2");
        assert_eq!(p.labels[5], "3-3");
        assert_eq!(p.labels[6], "4");
        assert_eq!(p.labels[7], "5-1");
        assert_eq!(p.labels[8], "5-2");
        assert_eq!(p.labels[9], "5-3-1");
        assert_eq!(p.labels[11], "5-3-3");
        let subs: Vec<(String, String, String, u32, usize)> = p
            .subs
            .iter()
            .map(|s| (s.label.clone(), s.name.clone(), s.shown.clone(), s.root, s.depth))
            .collect();
        assert_eq!(subs[0], ("3".to_string(), "Repair".to_string(), "Fix".to_string(), 4, 0));
        assert_eq!(subs[1], ("5".to_string(), "Ship".to_string(), "Ship".to_string(), 8, 0));
        assert_eq!(subs[2], ("5-3".to_string(), "Repair".to_string(), "Again".to_string(), 10, 1));
        // the impacts merged by name, in the order of first appearance
        assert!(p.yaml.starts_with("impact_names: [euro, hours]\n"));
    }

    #[test]
    fn a_reference_with_a_duration_is_refused_with_the_reason() {
        let mut r = store;
        let e = to_yaml_with("(@Repair, 3, {euro: 1})", &mut r).unwrap_err();
        assert!(e.contains("neither a duration nor impacts"), "{e}");
        assert!(to_yaml_with("(@Repair), (Check, 1, {euro: 1})", &mut r).is_ok());
        assert!(to_yaml_with("@Repair, @Ship", &mut r).is_ok());
    }

    #[test]
    fn a_process_referring_to_its_own_name_is_a_cycle_before_it_is_saved() {
        let mut r = store;
        let e = to_yaml_as("(A, 1, {x: 1}) , @Repair", "Repair", &mut r).unwrap_err();
        assert!(e.contains("cycle") && e.contains("Repair > Repair"), "{e}");
        assert!(to_yaml_as("(A, 1, {x: 1}) , @Repair", "Other", &mut r).is_ok());
    }

    #[test]
    fn a_cycle_and_an_unresolved_reference_are_errors() {
        let mut r = store;
        let e = to_yaml_with("@Loop", &mut r).unwrap_err();
        assert!(e.contains("cycle"), "{e}");
        let e = to_yaml("(A, 1, {x: 1}) , @Repair").unwrap_err();
        assert!(e.contains("saved processes"), "{e}");
        let e = to_yaml_with("@Nowhere", &mut r).unwrap_err();
        assert!(e.contains("Nowhere"), "{e}");
    }
}
