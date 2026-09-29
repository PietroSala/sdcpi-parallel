//! `sdcpi serve`: one page on localhost, a mask over the command line.
//!
//! The artifact of a tool paper has to run on the virtual machine the
//! evaluation provides or inside a container, so an installer buys nothing
//! and a page the binary serves itself buys everything: the user installs
//! nothing that is not already there, the three desktop systems come for
//! free, and a container needs one port.
//!
//! The server is written here rather than taken from a crate. It answers one
//! client at a time on the loopback address, serves one page held as a
//! constant and one endpoint, and runs the search in this process rather than
//! shelling out to itself. A hundred and fifty lines of `TcpListener` are
//! enough for that, and the whole of what makes a web server hard, the
//! concurrency, the routing, the static files, the sessions, is absent by
//! construction.
//!
//! What guards it, since it runs a computation on request:
//!   * it binds `127.0.0.1` and never `0.0.0.0`, so nothing outside the
//!     machine reaches it;
//!   * the request body is capped, and a request larger than the cap is
//!     refused before it is read;
//!   * the socket carries a read timeout, so a client that opens a
//!     connection and says nothing does not hold the server;
//!   * the search is given a time limit that the page cannot remove, because
//!     a search with no limit does not have to terminate;
//!   * no path from a request ever reaches the file system: the page is a
//!     constant and the process is parsed by the parser of the crate.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use crate::num::Exact;
use crate::pareto;
use crate::search::Config;
use crate::tree::Tree;
use crate::{parse, parse_args, SearchOptions};

const PAGE: &str = include_str!("serve/page.html");

/// The largest request body accepted, and the largest time limit the page may
/// ask for. A process is text and a generous one is a few kilobytes.
const MAX_BODY: usize = 256 * 1024;
const MAX_LIMIT: f64 = 600.0;

const EXAMPLE: &str = "\
( (Intake, 1, {euro: 10, hours: 2})
, ( (Swap, 2, {euro: 50, hours: 4})
    ^ ( (Mend, 3, {euro: 20, hours: 9})
      , ( (Redo, 2, {euro: 40, hours: 6}) ^[0.3] (Pass, 1, {hours: 1}) ) ) )
, ( (Check, 1, {euro: 6, hours: 1}) || (File, 1, {euro: 4, hours: 1}) )
, ( (Courier, 1, {euro: 26, hours: 1}) ^ (Counter, 2, {euro: 2, hours: 2}) ) )";

pub fn cmd_serve(args: &[String]) -> i32 {
    let args = match parse_args(args) {
        Ok(a) => a,
        Err(e) => return crate::fail(&e),
    };
    let port: u16 = match args.flag("port") {
        None => 8080,
        Some(p) => match p.parse() {
            Ok(v) => v,
            Err(_) => return crate::fail(&format!("--port wants a number, found {p:?}")),
        },
    };
    // the loopback address and never every address: this server runs a
    // computation for whoever asks, and whoever asks has to be this machine
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
        Ok(l) => l,
        Err(e) => return crate::fail(&format!("cannot listen on port {port}: {e}")),
    };
    println!("sdcpi serve: http://127.0.0.1:{port}  (ctrl-c to stop)");
    for stream in listener.incoming() {
        match stream {
            Ok(s) => serve_one(s),
            Err(e) => eprintln!("connection refused itself: {e}"),
        }
    }
    0
}

fn serve_one(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(60)));
    let (method, path, body) = match request(&mut stream) {
        Ok(v) => v,
        Err(e) => return send(&mut stream, 400, "text/plain; charset=utf-8", &e),
    };
    match (method.as_str(), path.as_str()) {
        ("GET", "/") => send(&mut stream, 200, "text/html; charset=utf-8", PAGE),
        ("GET", "/api/hello") => send(&mut stream, 200, "application/json", &hello()),
        ("POST", "/api/frontier") => {
            let out = frontier(&body);
            send(&mut stream, 200, "application/json", &out)
        }
        ("POST", "/api/svg") => {
            let out = diagram(&body);
            send(&mut stream, 200, "application/json", &out)
        }
        _ => send(&mut stream, 404, "text/plain; charset=utf-8", "no such page\n"),
    }
}

/// The method, the path and the body of one request. Nothing else of HTTP is
/// read, because nothing else is answered.
fn request(stream: &mut TcpStream) -> Result<(String, String, String), String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("the request line did not arrive: {e}"))?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut length = 0usize;
    loop {
        let mut h = String::new();
        let n = reader.read_line(&mut h).map_err(|e| e.to_string())?;
        if n == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        let lower = h.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            length = v.trim().parse().unwrap_or(0);
        }
    }
    if length > MAX_BODY {
        return Err(format!("the body is larger than the {MAX_BODY} bytes accepted\n"));
    }
    let mut body = vec![0u8; length];
    reader
        .read_exact(&mut body)
        .map_err(|e| format!("the body did not arrive whole: {e}"))?;
    let body = String::from_utf8(body).map_err(|_| "the body is not UTF-8".to_string())?;
    Ok((method, path, body))
}

fn send(stream: &mut TcpStream, code: u16, kind: &str, body: &str) {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

// -- the form ---------------------------------------------------------------

/// One field of an `application/x-www-form-urlencoded` body.
fn field<'a>(body: &'a str, name: &str) -> Option<String> {
    for pair in body.split('&') {
        let (k, v) = pair.split_once('=')?;
        if unescape(k) == name {
            return Some(unescape(v));
        }
    }
    None
}

fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                let hex = |c: u8| match c {
                    b'0'..=b'9' => Some(c - b'0'),
                    b'a'..=b'f' => Some(c - b'a' + 10),
                    b'A'..=b'F' => Some(c - b'A' + 10),
                    _ => None,
                };
                match (hex(b[i + 1]), hex(b[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push(h * 16 + l);
                        i += 3;
                    }
                    _ => {
                        out.push(b[i]);
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One decision of a strategy, written with the names of the instance rather
/// than with the identifiers of its nodes. A decision says which branch to
/// take once the run has reached a point, and the point is the history: the
/// identifiers of the nodes already resolved. A reader wants the names.
fn decision(tree: &Tree, history: &[u32], action: u32) -> String {
    let taken: Vec<String> = history.iter().map(|&id| name_of(tree, id)).collect();
    if taken.is_empty() {
        format!("take {}", name_of(tree, action))
    } else {
        format!(
            "once {} is reached, take {}",
            taken.join(" and "),
            name_of(tree, action)
        )
    }
}

/// The name of a node. A task carries its own; an internal node carries none
/// and takes the name of the first task of its region, which is the task the
/// run reaches first when that branch is taken, so that a decision reads as a
/// sentence about the process and not about the identifiers of its nodes.
fn name_of(tree: &Tree, id: u32) -> String {
    if let Some(n) = tree.task_names.get(id as usize) {
        if !n.is_empty() {
            return n.clone();
        }
    }
    let mut at = id;
    for _ in 0..tree.n_nodes {
        match tree.nodes.get(at as usize) {
            None => break,
            Some(node) => {
                if let Some(n) = tree.task_names.get(at as usize) {
                    if !n.is_empty() {
                        return n.clone();
                    }
                }
                if node.low == 0 || node.low == at {
                    break;
                }
                at = node.low;
            }
        }
    }
    format!("node {id}")
}

// -- the answers ------------------------------------------------------------

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn hello() -> String {
    format!(
        "{{\"grammar\": {}, \"example\": {}}}",
        esc(
            "(name, duration, {impact: value, ...}) is a task, the map naming only the \
             impacts that are not zero; , is a sequence, || a parallel, ^ a choice of \
             the controller and ^[p] one of nature taking its left operand with \
             probability p; brackets group."
        ),
        esc(EXAMPLE)
    )
}

fn err(message: &str) -> String {
    format!("{{\"error\": {}}}", esc(message))
}

/// The diagram of the process as the page has it at the moment, drawn as
/// SVG; parsed by the parser of the crate and nothing else.
fn diagram(body: &str) -> String {
    let text = match field(body, "process") {
        Some(t) if !t.trim().is_empty() => t,
        _ => return err("no process was given"),
    };
    let yaml = match parse::to_yaml(&text) {
        Ok(p) => p.yaml,
        Err(e) => return err(&format!("the process does not parse: {e}")),
    };
    let tree = match Tree::parse(&yaml) {
        Ok(t) => t,
        Err(e) => return err(&format!("the instance does not read: {e}")),
    };
    let detailed = matches!(field(body, "detail").as_deref(), Some("1") | Some("true"));
    format!("{{\"svg\": {}}}", esc(&crate::svg::render(&tree, detailed)))
}

/// Runs one search and answers with the frontier, every point with the
/// strategy that attains it.
fn frontier(body: &str) -> String {
    let text = match field(body, "process") {
        Some(t) if !t.trim().is_empty() => t,
        _ => return err("no process was given"),
    };
    let workers = field(body, "workers").unwrap_or_else(|| "1".into());
    let ablation = field(body, "ablation").unwrap_or_else(|| "both".into());
    let limit = field(body, "limit")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(30.0)
        .clamp(1.0, MAX_LIMIT);

    // the process as the parser of the crate reads it, then the tree; nothing
    // touches the file system on the way
    let yaml = match parse::to_yaml(&text) {
        Ok(p) => p.yaml,
        Err(e) => return err(&format!("the process does not parse: {e}")),
    };
    let tree = match Tree::parse(&yaml) {
        Ok(t) => t,
        Err(e) => return err(&format!("the instance does not read: {e}")),
    };

    // the flags the form gives, read by the very code the command line uses,
    // so that a default is never written twice
    let flags = vec![
        "--workers".to_string(),
        workers,
        "--ablation".to_string(),
        ablation,
    ];
    let args = match parse_args(&flags) {
        Ok(a) => a,
        Err(e) => return err(&e),
    };
    let opts = match SearchOptions::from_args(&args) {
        Ok(o) => o,
        Err(e) => return err(&e),
    };

    let started = Instant::now();
    let report = pareto::frontier(
        &tree,
        pareto::Options {
            onephase: true,
            oracle: pareto::OracleKind::Antichain,
            extract: pareto::Extract::Tightest,
            certificates: pareto::Certificates::Both,
            print_certificate: false,
            // the page cannot take the limit away: a search with none does
            // not have to terminate, and this one answers a browser
            time_limit: Some(Duration::from_secs_f64(limit)),
            anytime: None,
            precision: None,
            components: None,
            library: pareto::Library::Biodivine,
            search: Config {
                certificate: false,
                ..opts.config::<Exact>(Vec::new(), true)
            },
        },
    );
    let report = match report {
        Ok(r) => r,
        Err(e) => return err(&e),
    };
    let seconds = started.elapsed().as_secs_f64();

    let status = match &report.status {
        pareto::Status::Complete => "complete",
        pareto::Status::Timeout => "cut at the limit",
        pareto::Status::Failed(_) => "failed",
    };
    let comps: Vec<String> = report.components.iter().map(|c| c.to_string()).collect();
    // the impacts by the names the process gives them, an index where it
    // gives none
    let names: Vec<String> = report
        .components
        .iter()
        .map(|&c| match tree.meta.impact_names.get(c) {
            Some(n) if !n.is_empty() => esc(n),
            _ => esc(&format!("impact {c}")),
        })
        .collect();
    let mut points = Vec::new();
    for p in &report.points {
        let value: Vec<String> = p.value.iter().map(|v| esc(&v.to_string())).collect();
        let approx: Vec<String> = p
            .value
            .iter()
            .map(|v| format!("{:.6}", pareto::approx(v)))
            .collect();
        let decisions: Vec<String> = match &p.witness {
            None => Vec::new(),
            Some(w) => w
                .decisions
                .iter()
                .map(|(history, action)| esc(&decision(&tree, history, *action)))
                .collect(),
        };
        points.push(format!(
            "{{\"value\": [{}], \"approx\": [{}], \"decisions\": [{}]}}",
            value.join(", "),
            approx.join(", "),
            decisions.join(", ")
        ));
    }
    let note = if matches!(report.status, pareto::Status::Timeout) {
        "The search was cut at the limit, so these points are candidates: they are minimal \
         among what had been found, and a longer run may replace them."
    } else {
        "The search finished, so every point is an optimum and the set is the whole frontier."
    };
    format!(
        "{{\"status\": {}, \"seconds\": {:.6}, \"components\": [{}], \"names\": [{}], \"apexes\": {}, \
         \"expanded\": {}, \"note\": {}, \"points\": [{}]}}",
        esc(status),
        seconds,
        comps.join(", "),
        names.join(", "),
        report.accepts,
        report.search.expanded,
        esc(note),
        points.join(", ")
    )
}
