//! Signal rules as text, and the text and the graph kept the same.
//!
//! A rule is a line: a name, `=`, and an expression of other signals.
//!
//! ```text
//! -- Red wins by holding every site for ten minutes without a break.
//! win_after = 600
//! red.clock = timer(red.holds_all and not blue.contesting, red.lost_a_site)
//! red.wins = red.clock >= win_after
//! ```
//!
//! The text and the graph are two views of the same rules. [`Rules`] is the text, read line
//! by line: [`Rules::apply`] makes the graph what the text says, and [`Rules::follow`] makes
//! the text what the graph says, rewriting only the lines whose rules changed and leaving
//! comments, blank lines and every untouched line as they were written.
//! [`App::signal_rules`] keeps a file and the running graph the same from then on: save the
//! file and the game follows; rewire a node in the panel and the file follows.
//!
//! An expression that is more than one operation becomes several nodes, the inner ones named
//! after the rule (`red.clock#1`, …), and such nodes are written back inside their rule's
//! line and not as lines of their own.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use crate::{
    app::{App, Stage},
    asset_server::{stamp, Stamp},
    ecs::World,
    signal::{Compare, Op, Signal, Signals},
};

/// What a rule's line says its signal is.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// Another signal, by name.
    Name(String),
    /// A number or a truth written out.
    Value(Signal),
    /// An operation on other expressions.
    Node(Op, Vec<Expr>),
}

/// Where the text could not be read, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Mistake {
    /// Counted from one.
    pub line: usize,
    pub column: usize,
    pub why: String,
}

impl std::fmt::Display for Mistake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}, column {}: {}", self.line, self.column, self.why)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Name(String),
    Number(f64),
    /// `(`, `)`, `,`, `=`, `+`, and the comparisons.
    Mark(&'static str),
    /// `and`, `or`, `not`, `if`, `then`, `else`, `true`, `false`.
    Word(&'static str),
}

const WORDS: [&str; 8] = ["and", "or", "not", "if", "then", "else", "true", "false"];
const MARKS: [&str; 10] = ["<=", ">=", "==", "(", ")", ",", "=", "+", "<", ">"];

/// What was read, or the column where it could not be, and why.
type Read<T> = Result<T, (usize, String)>;
/// Tokens, each with the column it starts at.
type Tokens = Vec<(Token, usize)>;

fn in_a_name(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '#')
}

/// Splits a line into tokens, each with the column it starts at, and gives the comment the
/// line ends with (from its `--`, with the space before it), if it has one.
fn tokens(line: &str) -> Read<(Tokens, &str)> {
    let mut found = Vec::new();
    let mut rest = line.char_indices().peekable();
    while let Some(&(at, c)) = rest.peek() {
        let after = &line[at..];
        if c.is_whitespace() {
            rest.next();
        } else if after.starts_with("--") {
            let before = line[..at].trim_end().len();
            return Ok((found, &line[before..]));
        } else if c == '`' {
            let Some(end) = after[1..].find('`') else {
                return Err((at + 1, "a name begun with ` is not ended".to_owned()));
            };
            found.push((Token::Name(after[1..1 + end].to_owned()), at + 1));
            while rest.next_if(|&(next, _)| next <= at + 1 + end).is_some() {}
        } else if c.is_ascii_digit() || (c == '-' && after[1..].starts_with(|next: char| next.is_ascii_digit())) {
            let end = 1 + after[1..].find(|next: char| !(next.is_ascii_digit() || next == '.')).unwrap_or(after.len() - 1);
            let number = after[..end].parse().map_err(|_| (at + 1, format!("`{}` is not a number", &after[..end])))?;
            found.push((Token::Number(number), at + 1));
            while rest.next_if(|&(next, _)| next < at + end).is_some() {}
        } else if in_a_name(c) {
            let end = after.find(|next: char| !in_a_name(next)).unwrap_or(after.len());
            let word = &after[..end];
            found.push((WORDS.iter().find(|known| **known == word).map_or_else(|| Token::Name(word.to_owned()), |known| Token::Word(known)), at + 1));
            while rest.next_if(|&(next, _)| next < at + end).is_some() {}
        } else if let Some(mark) = MARKS.iter().find(|mark| after.starts_with(**mark)) {
            found.push((Token::Mark(mark), at + 1));
            while rest.next_if(|&(next, _)| next < at + mark.len()).is_some() {}
        } else {
            return Err((at + 1, format!("`{c}` means nothing here")));
        }
    }
    Ok((found, ""))
}

struct Reader<'a> {
    tokens: &'a [(Token, usize)],
    at: usize,
    /// The column past the end of the line, for a mistake that is the line ending too soon.
    end: usize,
}


impl Reader<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at).map(|(token, _)| token)
    }

    fn column(&self) -> usize {
        self.tokens.get(self.at).map_or(self.end, |(_, column)| *column)
    }

    fn take(&mut self, token: &Token) -> bool {
        let there = self.peek() == Some(token);
        self.at += there as usize;
        there
    }

    fn expect(&mut self, token: Token, what: &str) -> Read<()> {
        if self.take(&token) {
            Ok(())
        } else {
            Err((self.column(), format!("{what} is expected here")))
        }
    }

    /// `if a then b else c`, or anything that binds tighter.
    fn expr(&mut self) -> Read<Expr> {
        if !self.take(&Token::Word("if")) {
            return self.either();
        }
        let when = self.expr()?;
        self.expect(Token::Word("then"), "`then`")?;
        let then = self.expr()?;
        self.expect(Token::Word("else"), "`else`")?;
        Ok(Expr::Node(Op::Select, vec![when, then, self.expr()?]))
    }

    /// A run of the same joining word is one operation with every part as an input.
    fn joined(&mut self, word: Token, op: Op, part: fn(&mut Self) -> Read<Expr>) -> Read<Expr> {
        let first = part(self)?;
        if self.peek() != Some(&word) {
            return Ok(first);
        }
        let mut parts = vec![first];
        while self.take(&word) {
            parts.push(part(self)?);
        }
        Ok(Expr::Node(op, parts))
    }

    fn either(&mut self) -> Read<Expr> {
        self.joined(Token::Word("or"), Op::Or, Self::both)
    }

    fn both(&mut self) -> Read<Expr> {
        self.joined(Token::Word("and"), Op::And, Self::negated)
    }

    fn negated(&mut self) -> Read<Expr> {
        if !self.take(&Token::Word("not")) {
            return self.compared();
        }
        // `not()` and `not(a, b)` are a `not` with the wrong number of inputs, as a graph
        // being rewired can have; `not (a)` is the ordinary one.
        if self.peek() == Some(&Token::Mark("(")) && self.is_a_list() {
            return Ok(Expr::Node(Op::Not, self.list()?));
        }
        Ok(Expr::Node(Op::Not, vec![self.negated()?]))
    }

    /// Whether the bracket here holds nothing, or things with commas between them.
    fn is_a_list(&self) -> bool {
        let mut depth = 0;
        for (place, (token, _)) in self.tokens[self.at..].iter().enumerate() {
            match token {
                Token::Mark("(") => depth += 1,
                Token::Mark(")") if depth == 1 => return place == 1,
                Token::Mark(")") => depth -= 1,
                Token::Mark(",") if depth == 1 => return true,
                _ => {}
            }
        }
        false
    }

    fn compared(&mut self) -> Read<Expr> {
        let left = self.summed()?;
        let compare = match self.peek() {
            Some(Token::Mark("<")) => Compare::Less,
            Some(Token::Mark("<=")) => Compare::LessOrEqual,
            Some(Token::Mark("==")) => Compare::Equal,
            Some(Token::Mark(">=")) => Compare::GreaterOrEqual,
            Some(Token::Mark(">")) => Compare::Greater,
            _ => return Ok(left),
        };
        self.at += 1;
        Ok(Expr::Node(Op::Compare(compare), vec![left, self.summed()?]))
    }

    fn summed(&mut self) -> Read<Expr> {
        self.joined(Token::Mark("+"), Op::Sum, Self::single)
    }

    /// The expressions in a pair of brackets, with commas between.
    fn list(&mut self) -> Read<Vec<Expr>> {
        self.expect(Token::Mark("("), "`(`")?;
        let mut items = Vec::new();
        if self.take(&Token::Mark(")")) {
            return Ok(items);
        }
        loop {
            items.push(self.expr()?);
            if self.take(&Token::Mark(")")) {
                return Ok(items);
            }
            self.expect(Token::Mark(","), "`,` or `)`")?;
        }
    }

    fn single(&mut self) -> Read<Expr> {
        let column = self.column();
        let Some(token) = self.peek().cloned() else {
            return Err((column, "the line ends where a signal or a value is expected".to_owned()));
        };
        self.at += 1;
        match token {
            Token::Number(number) => Ok(Expr::Value(Signal::Number(number))),
            Token::Word("true") => Ok(Expr::Value(Signal::Bool(true))),
            Token::Word("false") => Ok(Expr::Value(Signal::Bool(false))),
            Token::Mark("(") => {
                let inside = self.expr()?;
                self.expect(Token::Mark(")"), "`)`")?;
                Ok(inside)
            }
            Token::Name(name) if self.peek() == Some(&Token::Mark("(")) => {
                let mut inputs = self.list()?;
                let op = match name.as_str() {
                    "all" => Op::And,
                    "any" => Op::Or,
                    // How long is the last thing in the brackets: `held(blue.away, 3)`.
                    "held" => match inputs.pop() {
                        Some(Expr::Value(Signal::Number(seconds))) => Op::HeldFor(seconds),
                        _ => return Err((column, "`held` ends with for how many seconds, as a number: `held(blue.away, 3)`".to_owned())),
                    },
                    "constant" | "held_for" | "and" | "or" | "not" => return Err((column, format!("`{name}` is not something to call"))),
                    other => Op::named(other, None, None).map_err(|_| (column, format!("there is nothing called `{name}` to call; there are timer, held, count, all, any, sum and select")))?,
                };
                Ok(Expr::Node(op, inputs))
            }
            Token::Name(name) => Ok(Expr::Name(name)),
            other => Err((column, format!("a signal or a value is expected here, not {}", shown(&other)))),
        }
    }
}

fn shown(token: &Token) -> String {
    match token {
        Token::Name(name) => format!("`{name}`"),
        Token::Number(number) => format!("`{number}`"),
        Token::Mark(mark) | Token::Word(mark) => format!("`{mark}`"),
    }
}

/// How tightly each kind of expression binds, loosest first: one that binds less tightly
/// than where it stands is written in brackets.
fn binding(expr: &Expr) -> u8 {
    match expr {
        Expr::Node(Op::Select, inputs) if inputs.len() == 3 => 0,
        Expr::Node(Op::Or, inputs) if inputs.len() >= 2 => 1,
        Expr::Node(Op::And, inputs) if inputs.len() >= 2 => 2,
        Expr::Node(Op::Not, inputs) if inputs.len() == 1 => 3,
        Expr::Node(Op::Compare(_), inputs) if inputs.len() == 2 => 4,
        Expr::Node(Op::Sum, inputs) if inputs.len() >= 2 => 5,
        _ => 6,
    }
}

fn write_name(name: &str) -> String {
    let plain = !name.is_empty() && name.chars().all(in_a_name) && !WORDS.contains(&name) && !name.starts_with(|c: char| c.is_ascii_digit());
    if plain {
        name.to_owned()
    } else {
        format!("`{name}`")
    }
}

fn write_value(value: Signal) -> String {
    match value {
        Signal::Bool(truth) => truth.to_string(),
        Signal::Number(number) => number.to_string(),
    }
}

/// An expression as text, in brackets if it binds less tightly than `least`.
fn write(expr: &Expr, least: u8) -> String {
    let own = binding(expr);
    let list = |inputs: &[Expr]| inputs.iter().map(|input| write(input, 0)).collect::<Vec<_>>().join(", ");
    let joined = |inputs: &[Expr], with: &str| inputs.iter().map(|input| write(input, own + 1)).collect::<Vec<_>>().join(with);
    let text = match expr {
        Expr::Name(name) => write_name(name),
        Expr::Value(value) => write_value(*value),
        Expr::Node(op, inputs) => match (op, own) {
            (Op::Select, 0) => format!("if {} then {} else {}", write(&inputs[0], 1), write(&inputs[1], 1), write(&inputs[2], 0)),
            (Op::Or, 1) => joined(inputs, " or "),
            (Op::And, 2) => joined(inputs, " and "),
            (Op::Not, 3) => format!("not {}", write(&inputs[0], 3)),
            (Op::Sum, 5) => joined(inputs, " + "),
            (Op::Compare(compare), 4) => {
                let mark = match compare {
                    Compare::Less => "<",
                    Compare::LessOrEqual => "<=",
                    Compare::Equal => "==",
                    Compare::GreaterOrEqual => ">=",
                    Compare::Greater => ">",
                };
                format!("{} {mark} {}", write(&inputs[0], 5), write(&inputs[1], 5))
            }
            // What is left is called by name: operations that always are, and those above
            // with a number of inputs their own way of writing has no place for.
            (Op::Constant(value), _) => write_value(*value),
            (Op::HeldFor(seconds), _) if inputs.is_empty() => format!("held({seconds})"),
            (Op::HeldFor(seconds), _) => format!("held({}, {seconds})", list(inputs)),
            (Op::And, _) => format!("all({})", list(inputs)),
            (Op::Or, _) => format!("any({})", list(inputs)),
            (Op::Not, _) => format!("not({})", list(inputs)),
            (other, _) => format!("{}({})", other.name(), list(inputs)),
        },
    };
    if own < least {
        format!("({text})")
    } else {
        text
    }
}

impl Expr {
    /// Reads one expression.
    pub fn read(text: &str) -> Result<Expr, Mistake> {
        let wrong = |(column, why)| Mistake { line: 1, column, why };
        let (tokens, _) = tokens(text).map_err(wrong)?;
        let mut reader = Reader { tokens: &tokens, at: 0, end: text.trim_end().len() + 1 };
        let expr = reader.expr().map_err(wrong)?;
        match reader.peek() {
            None => Ok(expr),
            Some(extra) => Err(wrong((reader.column(), format!("{} is more than the expression", shown(extra))))),
        }
    }

    /// The expression as text, which [`read`](Self::read) reads back as the same expression.
    pub fn to_text(&self) -> String {
        write(self, 0)
    }

    /// The nodes the expression is in a graph, the signal `name` among them: each
    /// operation a node, and each inner one named after `name` with a number.
    fn nodes(&self, name: &str) -> Vec<(String, Op, Vec<String>)> {
        fn node(expr: &Expr, name: String, root: &str, numbered: &mut usize, out: &mut Vec<(String, Op, Vec<String>)>) {
            let (op, inputs) = match expr {
                Expr::Value(value) => (Op::Constant(*value), &[][..]),
                // A rule that is only another signal: the one thing that is true when it is.
                Expr::Name(_) => (Op::Or, std::slice::from_ref(expr)),
                Expr::Node(op, inputs) => (*op, &inputs[..]),
            };
            let place = out.len();
            out.push((name, op, Vec::new()));
            for input in inputs {
                let named = match input {
                    Expr::Name(named) => named.clone(),
                    inner => {
                        *numbered += 1;
                        let named = format!("{root}#{numbered}");
                        node(inner, named.clone(), root, numbered, out);
                        named
                    }
                };
                out[place].2.push(named);
            }
        }
        let mut out = Vec::new();
        node(self, name.to_owned(), name, &mut 0, &mut out);
        out
    }
}

/// The graph's rules as the expressions a text of them would have.
struct Drawn<'a> {
    signals: &'a Signals,
    inputs: HashMap<String, Vec<String>>,
    /// The nodes that read each signal, once for each input that does.
    read_by: HashMap<String, Vec<String>>,
}

impl<'a> Drawn<'a> {
    fn new(signals: &'a Signals) -> Self {
        let (mut inputs, mut read_by) = (HashMap::new(), HashMap::<String, Vec<String>>::new());
        for node in signals.graph() {
            for input in &node.inputs {
                read_by.entry(input.clone()).or_default().push(node.name.clone());
            }
            inputs.insert(node.name, node.inputs);
        }
        Self { signals, inputs, read_by }
    }

    /// Whether a node is part of a rule's line and not a line of its own: it is named after
    /// the rule, and read once, by the rule or by another part of it.
    fn within<'n>(&self, name: &'n str, depth: usize) -> Option<&'n str> {
        let (root, _) = name.rsplit_once('#')?;
        let [reader] = self.read_by.get(name)?.as_slice() else { return None };
        self.signals.op(name)?;
        self.signals.op(root)?;
        let by_the_rule = reader == root || (depth < 64 && reader.starts_with(root) && self.within(reader, depth + 1) == Some(root));
        by_the_rule.then_some(root)
    }

    fn expr(&self, name: &str, root: &str, depth: usize) -> Option<Expr> {
        let op = self.signals.op(name)?;
        if let Op::Constant(value) = op {
            return Some(Expr::Value(value));
        }
        let inputs = self.inputs.get(name)?.iter().map(|input| match self.within(input, 0) {
            Some(of) if of == root && depth < 64 => self.expr(input, root, depth + 1).unwrap_or_else(|| Expr::Name(input.clone())),
            _ => Expr::Name(input.clone()),
        });
        Some(Expr::Node(op, inputs.collect()))
    }

}

#[derive(Clone, Debug, PartialEq)]
enum Line {
    /// A comment or nothing at all, kept as written.
    Other(String),
    Rule {
        name: String,
        expr: Expr,
        /// The whole line as written, or as last rewritten.
        text: String,
        /// The comment it ends with, with the space before it.
        comment: String,
    },
}

/// A text of rules, a line each, with its comments.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rules {
    lines: Vec<Line>,
    /// The signals the text made, or was made from: those taken away again if their line is.
    made: HashSet<String>,
}

impl Rules {
    /// Reads a text of rules. Nothing is read if any line cannot be.
    pub fn read(text: &str) -> Result<Rules, Mistake> {
        let mut lines = Vec::new();
        let mut names = HashSet::new();
        for (number, line) in text.lines().enumerate() {
            let wrong = |(column, why)| Mistake { line: number + 1, column, why };
            let (tokens, comment) = tokens(line).map_err(wrong)?;
            if tokens.is_empty() {
                lines.push(Line::Other(line.to_owned()));
                continue;
            }
            let (Some((Token::Name(name), _)), Some((Token::Mark("="), _))) = (tokens.first(), tokens.get(1)) else {
                return Err(wrong((1, "a rule is a name, `=`, and what the signal is".to_owned())));
            };
            if !names.insert(name.clone()) {
                return Err(wrong((1, format!("`{name}` has a rule already, further up"))));
            }
            let mut reader = Reader { tokens: &tokens, at: 2, end: line.len() - comment.len() + 1 };
            let expr = reader.expr().map_err(wrong)?;
            if let Some(extra) = reader.peek() {
                return Err(wrong((reader.column(), format!("{} is more than the rule", shown(extra)))));
            }
            // A rule that is only another signal is one node reading it.
            let expr = match expr {
                Expr::Name(other) => Expr::Node(Op::Or, vec![Expr::Name(other)]),
                expr => expr,
            };
            lines.push(Line::Rule { name: name.clone(), expr, text: line.to_owned(), comment: comment.to_owned() });
        }
        Ok(Rules { lines, made: HashSet::new() })
    }

    /// The rules of a graph as a text, with no comments, in the order they were defined.
    pub fn of(signals: &Signals) -> Rules {
        let mut rules = Rules::default();
        rules.follow(signals);
        rules
    }

    /// The text.
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        for line in &self.lines {
            text += match line {
                Line::Other(line) | Line::Rule { text: line, .. } => line,
            };
            text.push('\n');
        }
        text
    }

    /// What a rule's line says it is.
    pub fn rule(&self, name: &str) -> Option<&Expr> {
        self.lines.iter().find_map(|line| match line {
            Line::Rule { name: named, expr, .. } if named == name => Some(expr),
            _ => None,
        })
    }

    /// Makes the graph what the text says: every rule defined, and the signals an earlier
    /// reading of the text made that it no longer has taken away. A rule that is as the
    /// graph has it already is left alone, with whatever it has timed. Nothing is changed
    /// if the text gives a rule to a source. Returns how many signals were defined anew.
    pub fn apply(&mut self, signals: &mut Signals) -> Result<usize, Mistake> {
        let mut nodes = Vec::new();
        for (number, line) in self.lines.iter().enumerate() {
            let Line::Rule { name, expr, .. } = line else { continue };
            if signals.get(name).is_some() && signals.op(name).is_none() {
                return Err(Mistake { line: number + 1, column: 1, why: format!("`{name}` is a source, which the game's code works out") });
            }
            for node in expr.nodes(name) {
                // The parts of a rule are named after it, and a rule may happen to have such a name.
                if nodes.iter().any(|(named, ..): &(String, Op, Vec<String>)| *named == node.0) {
                    let why = format!("`{}` is a rule, and is also what a part of the rule `{name}` is called; give the rule another name", node.0);
                    return Err(Mistake { line: number + 1, column: 1, why });
                }
                nodes.push(node);
            }
        }
        let made: HashSet<String> = nodes.iter().map(|(name, ..)| name.clone()).collect();
        for gone in self.made.difference(&made) {
            signals.remove(gone);
        }
        let mut defined = 0;
        for (name, op, inputs) in nodes {
            if !signals.is_defined_as(&name, op, &inputs) {
                signals.define(&name, op, inputs);
                defined += 1;
            }
        }
        self.made = made;
        Ok(defined)
    }

    /// Makes the text what the graph says. A line whose rule the graph has as written is
    /// left exactly as it is; one whose rule has changed is written again, keeping its
    /// comment; one whose signal is gone is taken out; and a rule the text has no line for
    /// is given one at the end. Returns whether the text changed.
    pub fn follow(&mut self, signals: &Signals) -> bool {
        let drawn = Drawn::new(signals);
        let graph = signals.graph();
        let rules: Vec<&str> = graph.iter().map(|node| node.name.as_str()).filter(|name| signals.op(name).is_some() && drawn.within(name, 0).is_none()).collect();
        let before = self.lines.len();
        let mut changed = false;
        let mut written = HashSet::new();
        self.lines.retain_mut(|line| {
            let Line::Rule { name, expr, text, comment } = line else { return true };
            let Some(now) = rules.contains(&name.as_str()).then(|| drawn.expr(name, name, 0)).flatten() else { return false };
            written.insert(name.clone());
            if now != *expr {
                // A rule that is only another signal is written as that signal.
                let said = match &now {
                    Expr::Node(Op::Or, inputs) if inputs.len() == 1 => write(&inputs[0], 0),
                    now => write(now, 0),
                };
                *text = format!("{} = {said}{comment}", write_name(name));
                *expr = now;
                changed = true;
            }
            true
        });
        changed |= self.lines.len() != before;
        for name in rules {
            if written.contains(name) {
                continue;
            }
            let Some(expr) = drawn.expr(name, name, 0) else { continue };
            let said = match &expr {
                Expr::Node(Op::Or, inputs) if inputs.len() == 1 => write(&inputs[0], 0),
                expr => write(expr, 0),
            };
            self.lines.push(Line::Rule { name: name.to_owned(), text: format!("{} = {said}", write_name(name)), expr, comment: String::new() });
            changed = true;
        }
        self.made = graph.iter().filter(|node| signals.op(&node.name).is_some()).map(|node| node.name.clone()).collect();
        changed
    }
}

/// A file of rules and the running graph, kept the same. A resource, there once
/// [`App::signal_rules`] has named the file.
pub struct RulesFile {
    path: PathBuf,
    rules: Rules,
    /// The file as it was when last read or written.
    seen: Option<Stamp>,
    /// The graph's changes as of when the two were last made the same.
    revision: Option<u64>,
    /// Why the file cannot be read as it stands. While it cannot, the graph is as the file
    /// last made it, and nothing is written over what is being typed.
    pub problem: Option<Mistake>,
}

impl RulesFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), rules: Rules::default(), seen: None, revision: None, problem: None }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The rules as they stand.
    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    /// Makes the two the same: the graph follows the file if the file has been saved since
    /// they last were, and otherwise the file follows the graph if that has changed.
    pub fn keep(&mut self, signals: &mut Signals) {
        let now = stamp(&self.path);
        if now.is_some() && now != self.seen {
            self.seen = now;
            let read = std::fs::read_to_string(&self.path).map_err(|err| Mistake { line: 1, column: 1, why: err.to_string() }).and_then(|text| Rules::read(&text));
            let made = std::mem::take(&mut self.rules.made);
            let applied = read.and_then(|mut rules| {
                rules.made = made.clone();
                rules.apply(signals).map(|_| rules)
            });
            match applied {
                Ok(rules) => {
                    self.rules = rules;
                    if self.problem.take().is_some() {
                        log::info!("{} can be read again", self.path.display());
                    }
                }
                Err(mistake) => {
                    self.rules.made = made;
                    log::error!("{}: {mistake}", self.path.display());
                    self.problem = Some(mistake);
                }
            }
            self.revision = Some(signals.revision());
            return;
        }
        if self.problem.is_some() || self.revision == Some(signals.revision()) {
            return;
        }
        self.revision = Some(signals.revision());
        if self.rules.follow(signals) {
            // Beside the file and then into its place, so that nothing reads half of it.
            let beside = self.path.with_extension("writing");
            let wrote = std::fs::write(&beside, self.rules.to_text()).and_then(|()| std::fs::rename(&beside, &self.path));
            match wrote {
                Ok(()) => self.seen = stamp(&self.path),
                Err(err) => log::error!("can't write {}: {err}", self.path.display()),
            }
        }
    }
}

fn keep_rules(world: &mut World) {
    if !world.contains_resource::<RulesFile>() {
        return;
    }
    world.resource_scope(|world, file: &mut RulesFile| {
        if let Some(signals) = world.get_resource_mut::<Signals>() {
            file.keep(signals);
        }
    });
}

impl App {
    /// Keeps a file of rules and the graph the same from now on: the file is read at once
    /// if it is there, and written when the graph first has rules if it is not.
    pub fn signal_rules(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        let mut file = RulesFile::new(path);
        file.keep(self.world.resource_mut::<Signals>());
        let first = !self.world.contains_resource::<RulesFile>();
        self.world.insert_resource(file);
        if first {
            self.add_systems(Stage::First, keep_rules);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Expr {
        Expr::read(text).unwrap_or_else(|mistake| panic!("{text}: {mistake}"))
    }

    #[test]
    fn an_expression_is_written_as_it_is_read() {
        for text in [
            "a",
            "600",
            "-2.5",
            "true",
            "not a",
            "a and b and c",
            "a or b and not c",
            "(a or b) and c",
            "a and (b and c)",
            "red.clock >= win_after",
            "a + b + 3 < c + 1",
            "not a < b",
            "(not a) + b",
            "if a then b else c",
            "if a and b then 1 else if c then 2 else 3",
            "(if a then b else c) + 1",
            "timer(red.holds_all and not blue.contesting, red.lost_a_site)",
            "timer(a)",
            "held(blue.away, 3)",
            "held(a or b, 0.5)",
            "count(a, b, not c)",
            "red.clock#1 or `a name with spaces` or `and`",
            // Operations with a number of inputs their own way of writing has no place for.
            "all()",
            "all(a)",
            "any()",
            "sum(a)",
            "not()",
            "not(a, b)",
            "select(a, b)",
            "greater(a)",
            "held(3)",
        ] {
            let expr = read(text);
            assert_eq!(expr.to_text(), text);
            assert_eq!(read(&expr.to_text()), expr, "{text}");
        }
        // Written other ways, they are the same expressions.
        assert_eq!(read("((a))"), read("a"));
        assert_eq!(read("a  and(b)"), read("a and b"));
        assert_eq!(read("not (a)"), read("not a"));
        assert_eq!(read("not (a) < b"), read("not a < b"));
        assert_eq!(read("a and b -- both"), read("a and b"));
        assert_eq!(read("a and b and c"), Expr::Node(Op::And, vec![Expr::Name("a".into()), Expr::Name("b".into()), Expr::Name("c".into())]));
    }

    #[test]
    fn a_mistake_is_told_with_its_place() {
        let told = |text: &str| Expr::read(text).unwrap_err();
        assert_eq!(told("a and").column, 6);
        assert_eq!(told("a and and b").column, 7);
        assert_eq!(told("a b").column, 3);
        assert_eq!(told("(a or b").column, 8);
        assert_eq!(told("a $ b").column, 3);
        assert!(told("held(a)").why.contains("how many seconds"));
        assert!(told("frobnicate(a)").why.contains("timer"));
        assert!(told("if a then b").why.contains("else"));
        let told = Rules::read("a = 1\n\nb = a +\n").unwrap_err();
        assert_eq!((told.line, told.column), (3, 8));
        assert!(Rules::read("a = 1\na = 2").unwrap_err().why.contains("already"));
        assert!(Rules::read("just words").unwrap_err().why.contains("a name, `=`"));
    }

    const SITES: &str = "\
-- Red wins by holding every site for ten minutes without a break.
win_after = 600

red.clock = timer(red.holds_all and not blue.contesting, red.lost_a_site)  -- runs only while held
red.lost_a_site   =   not red.holds_all
red.wins = red.clock >= win_after
";

    fn sites() -> (Signals, Rules) {
        let mut signals = Signals::default();
        signals.source("red.holds_all", || true);
        signals.source("blue.contesting", || false);
        let mut rules = Rules::read(SITES).unwrap();
        rules.apply(&mut signals).unwrap();
        (signals, rules)
    }

    #[test]
    fn the_text_makes_the_graph() {
        let (mut signals, mut rules) = sites();
        assert_eq!(signals.constant("win_after"), Some(Signal::Number(600.0)));
        assert_eq!(signals.op("red.clock"), Some(Op::Timer));
        // The inner operations are nodes of their own, named after the rule.
        assert!(signals.is_defined_as("red.clock", Op::Timer, &["red.clock#1".into(), "red.lost_a_site".into()]));
        assert!(signals.is_defined_as("red.clock#1", Op::And, &["red.holds_all".into(), "red.clock#2".into()]));
        assert!(signals.is_defined_as("red.clock#2", Op::Not, &["blue.contesting".into()]));
        // And it works: the clock runs, and red has not yet won.
        let mut world = World::new();
        signals.update(&mut world, 5.0);
        signals.update(&mut world, 5.0);
        assert_eq!(signals.number("red.clock"), 10.0);
        assert!(!signals.is_true("red.wins"));

        // Read again unchanged, nothing is defined anew, and the clock keeps its time.
        assert_eq!(rules.apply(&mut signals).unwrap(), 0);
        // A line changed: that rule only. A line gone: its signals with it, inner ones too.
        let mut rules = {
            let made = rules.made.clone();
            let mut edited = Rules::read(&SITES.replace("win_after = 600", "win_after = 8").replace("red.clock = timer(red.holds_all and not blue.contesting, red.lost_a_site)", "red.clock = timer(red.holds_all)")).unwrap();
            edited.made = made;
            edited
        };
        assert_eq!(rules.apply(&mut signals).unwrap(), 2);
        assert!(signals.get("red.clock#1").is_none() && signals.get("red.clock#2").is_none());
        signals.update(&mut world, 1.0);
        assert_eq!(signals.number("red.clock"), 11.0, "the clock kept its time through the change");
        assert!(signals.is_true("red.wins"));
        // A source is the game's, and no rule may be given to it.
        let mut over = Rules::read("red.holds_all = true").unwrap();
        assert!(over.apply(&mut signals).unwrap_err().why.contains("source"));
        // A rule named as a part of another would be two things at once.
        let mut twice = Rules::read("a#1 = true\na = not (b and c)").unwrap();
        assert!(twice.apply(&mut Signals::default()).unwrap_err().why.contains("another name"));
    }

    #[test]
    fn the_text_follows_the_graph_and_keeps_what_was_not_changed() {
        let (mut signals, mut rules) = sites();
        // Nothing changed in the graph: nothing changes in the text, not even its spacing.
        assert!(!rules.follow(&signals));
        assert_eq!(rules.to_text(), SITES);

        // In the graph: a constant set, an input rewired, a rule added, a rule taken away.
        signals.set("win_after", 30.0);
        signals.connect("red.clock#1", 1, "always");
        signals.define("blue.calm", Op::HeldFor(3.0), ["blue.away"]);
        signals.remove("red.lost_a_site");
        assert!(rules.follow(&signals));
        assert_eq!(
            rules.to_text(),
            "\
-- Red wins by holding every site for ten minutes without a break.
win_after = 30

red.clock = timer(red.holds_all and always, red.lost_a_site)  -- runs only while held
red.wins = red.clock >= win_after
red.clock#2 = not blue.contesting
blue.calm = held(blue.away, 3)
"
        );
        // The inner `not` is read by nothing now. It is still in the graph, so it has a line
        // of its own; taken out of the graph, it is gone from the text too.
        signals.remove("red.clock#2");
        assert!(rules.follow(&signals) && !rules.to_text().contains("red.clock#2"), "{}", rules.to_text());

        // And the text it wrote makes the same graph again.
        let mut again = Signals::default();
        Rules::read(&rules.to_text()).unwrap().apply(&mut again).unwrap();
        assert_eq!(Rules::of(&again).to_text().lines().filter(|line| !line.is_empty()).count(), 4);
        assert!(again.is_defined_as("red.clock#1", Op::And, &["red.holds_all".into(), "always".into()]));
    }

    #[test]
    fn a_graph_made_elsewhere_can_be_written_down() {
        let mut signals = Signals::default();
        signals.source("ready", || true);
        signals.set("limit", 4.0);
        signals.define("go", Op::And, ["ready", "go#1"]);
        signals.define("go#1", Op::Compare(Compare::Less), ["seen", "limit"]);
        // Named as a part, but read twice: a rule of its own.
        signals.define("go#2", Op::Not, ["ready"]);
        signals.define("stop", Op::Or, ["go#2", "stop#1"]);
        signals.define("stop#1", Op::Not, ["go#2"]);
        signals.define("same", Op::Or, ["go"]);
        assert_eq!(Rules::of(&signals).to_text(), "limit = 4\ngo = ready and seen < limit\ngo#2 = not ready\nstop = go#2 or not go#2\nsame = go\n");
        // Sources are the game's and are not in it.
        assert!(Rules::of(&signals).rule("ready").is_none());
    }

    #[test]
    fn a_file_and_the_graph_are_kept_the_same() {
        let folder = std::env::temp_dir().join(format!("mira-rules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("sites.rules");
        // Saved at a time surely later than the last: two saves can fall in one tick of a
        // file system's clock.
        let mut later = 0;
        let mut save = |text: &str| {
            later += 2;
            std::fs::write(&path, text).unwrap();
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(later)).unwrap();
        };

        let mut signals = Signals::default();
        signals.source("red.holds_all", || true);
        let mut file = RulesFile::new(&path);
        // No file yet, and no rules: none is written.
        file.keep(&mut signals);
        assert!(!path.exists());
        // The graph has a rule: the file is written.
        signals.set("win_after", 600.0);
        file.keep(&mut signals);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "win_after = 600\n");

        // The file is saved with more in it: the graph follows.
        save("win_after = 600  -- ten minutes\n-- how long red has held\nred.clock = timer(red.holds_all)\n");
        file.keep(&mut signals);
        assert_eq!(signals.op("red.clock"), Some(Op::Timer));
        // The graph is changed: the file follows, comments and all.
        signals.set("win_after", 30.0);
        file.keep(&mut signals);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "win_after = 30  -- ten minutes\n-- how long red has held\nred.clock = timer(red.holds_all)\n");

        // Saved half typed: the graph stays as it was, the mistake is told, and what is
        // being typed is not written over though the graph changes meanwhile.
        save("win_after = 30\nred.clock = timer(red.holds_all\n");
        file.keep(&mut signals);
        assert_eq!(file.problem.as_ref().map(|mistake| mistake.line), Some(2));
        assert_eq!(signals.op("red.clock"), Some(Op::Timer));
        signals.set("win_after", 31.0);
        file.keep(&mut signals);
        assert!(std::fs::read_to_string(&path).unwrap().ends_with("timer(red.holds_all\n"));
        // Finished: read, and the rule a line was taken away from is gone from the graph.
        save("red.clock = timer(red.holds_all)\n");
        file.keep(&mut signals);
        assert!(file.problem.is_none() && signals.get("win_after").is_none());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
