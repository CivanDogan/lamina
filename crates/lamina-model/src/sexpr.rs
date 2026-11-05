//! S-expressions as KiCad and Specctra use them: a small reader and a
//! writer in KiCad's layout.

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Sx {
    /// A bare symbol or number, or a quoted string (`quoted`).
    Atom {
        text: String,
        quoted: bool,
    },
    List(Vec<Sx>),
}

impl Sx {
    pub fn sym(s: impl Into<String>) -> Sx {
        Sx::Atom {
            text: s.into(),
            quoted: false,
        }
    }

    pub fn str(s: impl Into<String>) -> Sx {
        Sx::Atom {
            text: s.into(),
            quoted: true,
        }
    }

    pub fn num(v: f64) -> Sx {
        Sx::sym(fmt_num(v))
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Sx::Atom { text, .. } => Some(text),
            Sx::List(_) => None,
        }
    }

    pub fn number(&self) -> Option<f64> {
        self.text()?.parse().ok()
    }

    /// Name of a list node: its first atom.
    pub fn name(&self) -> Option<&str> {
        match self {
            Sx::List(items) => items.first()?.text(),
            Sx::Atom { .. } => None,
        }
    }

    pub fn is(&self, name: &str) -> bool {
        self.name() == Some(name)
    }

    /// Elements after the name.
    pub fn args(&self) -> &[Sx] {
        match self {
            Sx::List(items) if !items.is_empty() => &items[1..],
            Sx::List(items) => items,
            Sx::Atom { .. } => &[],
        }
    }

    /// The i-th argument as text (atoms only).
    pub fn arg(&self, i: usize) -> Option<&str> {
        self.args().get(i)?.text()
    }

    pub fn arg_num(&self, i: usize) -> Option<f64> {
        self.args().get(i)?.number()
    }

    /// Leading atom arguments, up to the first sub-list.
    pub fn atoms(&self) -> Vec<&str> {
        self.args()
            .iter()
            .take_while(|a| matches!(a, Sx::Atom { .. }))
            .filter_map(Sx::text)
            .collect()
    }

    /// Child lists named `name`.
    pub fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Sx> + 'a {
        self.args().iter().filter(move |c| c.is(name))
    }

    pub fn child(&self, name: &str) -> Option<&Sx> {
        self.args().iter().find(|c| c.is(name))
    }

    /// First numeric argument of the child `name`, e.g. `(width 0.12)`.
    pub fn child_num(&self, name: &str) -> Option<f64> {
        self.child(name)?.arg_num(0)
    }

    pub fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name)?.arg(0)
    }

    /// `(at x y [rot])` style pairs.
    pub fn xy(&self, name: &str) -> Option<(f64, f64)> {
        let c = self.child(name)?;
        Some((c.arg_num(0)?, c.arg_num(1)?))
    }

    pub fn push(&mut self, child: Sx) {
        if let Sx::List(items) = self {
            items.push(child);
        }
    }
}

/// Builder for writing.
pub struct Node(pub Vec<Sx>);

impl Node {
    pub fn new(name: &str) -> Node {
        Node(vec![Sx::sym(name)])
    }
    pub fn sym(mut self, s: impl Into<String>) -> Node {
        self.0.push(Sx::sym(s));
        self
    }
    pub fn str(mut self, s: impl Into<String>) -> Node {
        self.0.push(Sx::str(s));
        self
    }
    pub fn num(mut self, v: f64) -> Node {
        self.0.push(Sx::num(v));
        self
    }
    pub fn nums(mut self, vs: &[f64]) -> Node {
        for v in vs {
            self.0.push(Sx::num(*v));
        }
        self
    }
    pub fn child(mut self, c: Node) -> Node {
        self.0.push(c.done());
        self
    }
    pub fn child_if(self, cond: bool, c: impl FnOnce() -> Node) -> Node {
        if cond { self.child(c()) } else { self }
    }
    pub fn children(mut self, cs: impl IntoIterator<Item = Node>) -> Node {
        for c in cs {
            self.0.push(c.done());
        }
        self
    }
    pub fn done(self) -> Sx {
        Sx::List(self.0)
    }
}

/// Shorthand: `n("at").num(x).num(y)`.
pub fn n(name: &str) -> Node {
    Node::new(name)
}

/// Numbers as KiCad writes them: up to 6 decimals, no trailing zeros.
pub fn fmt_num(v: f64) -> String {
    let r = (v * 1e6).round() / 1e6;
    let r = if r == 0.0 { 0.0 } else { r };
    let s = format!("{r:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".into()
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------- parsing

pub fn parse(src: &str) -> Result<Sx, String> {
    let mut p = Parser {
        b: src.as_bytes(),
        i: 0,
        line: 1,
    };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i < p.b.len() {
        return Err(format!("line {}: text after the document", p.line));
    }
    Ok(v)
}

/// Every top-level value in a file (a `.kicad_mod` has one, libraries may
/// concatenate several).
pub fn parse_all(src: &str) -> Result<Vec<Sx>, String> {
    let mut p = Parser {
        b: src.as_bytes(),
        i: 0,
        line: 1,
    };
    let mut out = Vec::new();
    loop {
        p.ws();
        if p.i >= p.b.len() {
            return Ok(out);
        }
        out.push(p.value()?);
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
    line: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'\n' => {
                    self.line += 1;
                    self.i += 1;
                }
                b' ' | b'\t' | b'\r' => self.i += 1,
                _ => return,
            }
        }
    }

    fn value(&mut self) -> Result<Sx, String> {
        match self.b.get(self.i) {
            None => Err(format!("line {}: unexpected end of file", self.line)),
            Some(b'(') => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.ws();
                    match self.b.get(self.i) {
                        None => {
                            return Err(format!("line {}: missing `)`", self.line));
                        }
                        Some(b')') => {
                            self.i += 1;
                            return Ok(Sx::List(items));
                        }
                        // Specctra's `(string_quote ")` names the quote character itself.
                        Some(b'"')
                            if items.len() == 1 && items[0].text() == Some("string_quote") =>
                        {
                            self.i += 1;
                            items.push(Sx::sym("\""));
                        }
                        _ => items.push(self.value()?),
                    }
                }
            }
            Some(b')') => Err(format!("line {}: unexpected `)`", self.line)),
            Some(b'"') => {
                self.i += 1;
                let mut out = Vec::new();
                loop {
                    let Some(&c) = self.b.get(self.i) else {
                        return Err(format!("line {}: unterminated string", self.line));
                    };
                    self.i += 1;
                    match c {
                        b'"' => break,
                        b'\\' => {
                            let Some(&e) = self.b.get(self.i) else { break };
                            self.i += 1;
                            out.push(match e {
                                b'n' => b'\n',
                                b't' => b'\t',
                                b'r' => b'\r',
                                other => other,
                            });
                        }
                        b'\n' => {
                            self.line += 1;
                            out.push(c);
                        }
                        _ => out.push(c),
                    }
                }
                Ok(Sx::Atom {
                    text: String::from_utf8_lossy(&out).into_owned(),
                    quoted: true,
                })
            }
            Some(_) => {
                let s = self.i;
                while self.i < self.b.len()
                    && !matches!(
                        self.b[self.i],
                        b'(' | b')' | b'"' | b' ' | b'\t' | b'\r' | b'\n'
                    )
                {
                    self.i += 1;
                }
                Ok(Sx::Atom {
                    text: String::from_utf8_lossy(&self.b[s..self.i]).into_owned(),
                    quoted: false,
                })
            }
        }
    }
}

// ---------------------------------------------------------------- writing

/// Write in KiCad's style: one node per line, tabs for depth, atoms inline.
pub fn write(sx: &Sx) -> String {
    let mut out = String::new();
    write_into(&mut out, sx, 0);
    out.push('\n');
    out
}

fn write_into(out: &mut String, sx: &Sx, depth: usize) {
    match sx {
        Sx::Atom { text, quoted } => {
            if *quoted {
                out.push('"');
                for c in text.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        c => out.push(c),
                    }
                }
                out.push('"');
            } else {
                out.push_str(text);
            }
        }
        Sx::List(items) => {
            out.push('(');
            let mut inline = true;
            for (k, it) in items.iter().enumerate() {
                match it {
                    Sx::Atom { .. } if inline => {
                        if k > 0 {
                            out.push(' ');
                        }
                        write_into(out, it, depth + 1);
                    }
                    _ => {
                        inline = false;
                        out.push('\n');
                        for _ in 0..=depth {
                            out.push('\t');
                        }
                        write_into(out, it, depth + 1);
                    }
                }
            }
            if !inline {
                out.push('\n');
                for _ in 0..depth {
                    out.push('\t');
                }
            }
            out.push(')');
        }
    }
}

/// Short form `(name a b c)` on one line, for compact output.
pub fn write_inline(sx: &Sx) -> String {
    let mut s = String::new();
    match sx {
        Sx::Atom { .. } => write_into(&mut s, sx, 0),
        Sx::List(items) => {
            s.push('(');
            for (k, it) in items.iter().enumerate() {
                if k > 0 {
                    s.push(' ');
                }
                let _ = write!(s, "{}", write_inline(it));
            }
            s.push(')');
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_query() {
        let src = r#"(pad "1" smd roundrect (at -0.825 0 90) (size 0.8 0.95)
            (layers "F.Cu" "F.Mask" "F.Paste") (roundrect_rratio 0.25) (uuid "x"))"#;
        let p = parse(src).unwrap();
        assert!(p.is("pad"));
        assert_eq!(p.arg(0), Some("1"));
        assert_eq!(p.atoms(), vec!["1", "smd", "roundrect"]);
        assert_eq!(p.xy("at"), Some((-0.825, 0.0)));
        assert_eq!(p.child("at").unwrap().arg_num(2), Some(90.0));
        assert_eq!(p.child_num("roundrect_rratio"), Some(0.25));
        let layers: Vec<&str> = p.child("layers").unwrap().atoms();
        assert_eq!(layers, vec!["F.Cu", "F.Mask", "F.Paste"]);
    }

    #[test]
    fn strings_escape_and_round_trip() {
        let src = "(gr_text \"say \\\"hi\\\"\\nnext\" (at 1 2))";
        let p = parse(src).unwrap();
        assert_eq!(p.arg(0), Some("say \"hi\"\nnext"));
        let out = write(&p);
        assert_eq!(parse(&out).unwrap(), p);
        assert!(out.starts_with("(gr_text \"say \\\"hi\\\"\\nnext\"\n\t(at 1 2)\n)"));
    }

    #[test]
    fn errors_name_the_line() {
        assert!(parse("(a\n(b").unwrap_err().contains("line 2"));
        assert!(parse("(a))").unwrap_err().contains("after the document"));
    }

    #[test]
    fn numbers() {
        assert_eq!(fmt_num(1.0), "1");
        assert_eq!(fmt_num(-0.0000001), "0");
        assert_eq!(fmt_num(0.825), "0.825");
        assert_eq!(fmt_num(12.3456789), "12.345679");
        let node = n("at").num(1.5).num(-2.0).done();
        assert_eq!(write_inline(&node), "(at 1.5 -2)");
    }
}
