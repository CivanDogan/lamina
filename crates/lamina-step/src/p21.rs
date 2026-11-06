//! ISO 10303-21 (STEP physical file) parser for the DATA section.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Num(f64),
    Str(String),
    Enum(String),
    Ref(u32),
    List(Vec<Arg>),
    /// `$` or `*`
    Null,
    /// A typed value, for example `LENGTH_MEASURE(1.0)`.
    Typed(String, Vec<Arg>),
}

impl Arg {
    pub fn num(&self) -> Option<f64> {
        match self {
            Arg::Num(v) => Some(*v),
            Arg::Typed(_, a) => a.first().and_then(|x| x.num()),
            _ => None,
        }
    }
    pub fn id(&self) -> Option<u32> {
        match self {
            Arg::Ref(r) => Some(*r),
            _ => None,
        }
    }
    pub fn list(&self) -> &[Arg] {
        match self {
            Arg::List(v) => v,
            _ => &[],
        }
    }
    pub fn boolean(&self) -> Option<bool> {
        match self {
            Arg::Enum(e) => match e.as_str() {
                "T" => Some(true),
                "F" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }
    pub fn enum_name(&self) -> Option<&str> {
        match self {
            Arg::Enum(e) => Some(e),
            _ => None,
        }
    }
    pub fn text(&self) -> Option<&str> {
        match self {
            Arg::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// An entity instance: one or more (TYPE, args) records. A complex entity
/// has more than one.
#[derive(Debug, Clone)]
pub struct Entity {
    pub parts: Vec<(String, Vec<Arg>)>,
}

impl Entity {
    pub fn kind(&self) -> &str {
        &self.parts[0].0
    }
    pub fn args(&self) -> &[Arg] {
        &self.parts[0].1
    }
    pub fn part(&self, name: &str) -> Option<&[Arg]> {
        self.parts
            .iter()
            .find(|p| p.0 == name)
            .map(|p| p.1.as_slice())
    }
    pub fn is(&self, name: &str) -> bool {
        self.parts.iter().any(|p| p.0 == name)
    }
}

struct Lexer<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn skip_ws(&mut self) {
        loop {
            while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.b[self.i..].starts_with(b"/*") {
                match find(&self.b[self.i + 2..], b"*/") {
                    Some(k) => self.i += k + 4,
                    None => self.i = self.b.len(),
                }
            } else {
                return;
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.b.get(self.i).copied()
    }

    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("'{}' is missing at byte {}", c as char, self.i))
        }
    }

    fn ident(&mut self) -> String {
        self.skip_ws();
        let s = self.i;
        while self.i < self.b.len()
            && (self.b[self.i].is_ascii_alphanumeric() || self.b[self.i] == b'_')
        {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.b[s..self.i]).to_ascii_uppercase()
    }

    fn args(&mut self) -> Result<Vec<Arg>, String> {
        self.expect(b'(')?;
        let mut out = Vec::new();
        if self.peek() == Some(b')') {
            self.i += 1;
            return Ok(out);
        }
        loop {
            out.push(self.arg()?);
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b')') => {
                    self.i += 1;
                    return Ok(out);
                }
                _ => return Err(format!("the argument list at byte {} is not valid", self.i)),
            }
        }
    }

    fn arg(&mut self) -> Result<Arg, String> {
        match self.peek() {
            Some(b'#') => {
                self.i += 1;
                let s = self.i;
                while self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
                    self.i += 1;
                }
                let v = std::str::from_utf8(&self.b[s..self.i])
                    .ok()
                    .and_then(|x| x.parse().ok())
                    .ok_or("a reference is not valid")?;
                Ok(Arg::Ref(v))
            }
            Some(b'\'') => {
                self.i += 1;
                let mut s = Vec::new();
                loop {
                    let Some(&c) = self.b.get(self.i) else {
                        return Err("a string has no end quote".into());
                    };
                    self.i += 1;
                    if c == b'\'' {
                        if self.b.get(self.i) == Some(&b'\'') {
                            s.push(b'\'');
                            self.i += 1;
                        } else {
                            break;
                        }
                    } else {
                        s.push(c);
                    }
                }
                Ok(Arg::Str(String::from_utf8_lossy(&s).into_owned()))
            }
            Some(b'.') => {
                self.i += 1;
                let s = self.i;
                while self.i < self.b.len() && self.b[self.i] != b'.' {
                    self.i += 1;
                }
                let e = String::from_utf8_lossy(&self.b[s..self.i]).into_owned();
                self.i += 1;
                Ok(Arg::Enum(e))
            }
            Some(b'(') => Ok(Arg::List(self.args()?)),
            Some(b'$') | Some(b'*') => {
                self.i += 1;
                Ok(Arg::Null)
            }
            Some(b'"') => {
                // Skip a binary literal.
                self.i += 1;
                while self.i < self.b.len() && self.b[self.i] != b'"' {
                    self.i += 1;
                }
                self.i += 1;
                Ok(Arg::Null)
            }
            Some(c) if c == b'-' || c == b'+' || c.is_ascii_digit() => {
                let s = self.i;
                self.i += 1;
                while self.i < self.b.len()
                    && matches!(
                        self.b[self.i],
                        b'0'..=b'9' | b'.' | b'E' | b'e' | b'-' | b'+'
                    )
                {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.b[s..self.i]).map_err(|e| e.to_string())?;
                Ok(Arg::Num(
                    t.parse()
                        .map_err(|_| format!("the number {t} is not valid"))?,
                ))
            }
            Some(c) if c.is_ascii_alphabetic() => {
                let name = self.ident();
                let a = self.args()?;
                Ok(Arg::Typed(name, a))
            }
            other => Err(format!(
                "{:?} at byte {} is not valid here",
                other.map(|c| c as char),
                self.i
            )),
        }
    }
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// Parse all `#id = ...;` instances of the DATA section.
pub fn parse(bytes: &[u8]) -> Result<HashMap<u32, Entity>, String> {
    let start = find(bytes, b"DATA;").ok_or("the file has no DATA section")? + 5;
    let mut lx = Lexer { b: bytes, i: start };
    let mut out = HashMap::new();
    loop {
        match lx.peek() {
            None => break,
            Some(b'#') => {}
            Some(_) => {
                if lx.b[lx.i..].starts_with(b"ENDSEC") {
                    break;
                }
                // Skip all other text up to the next ';'.
                while lx.i < lx.b.len() && lx.b[lx.i] != b';' {
                    lx.i += 1;
                }
                lx.i += 1;
                continue;
            }
        }
        lx.i += 1;
        let s = lx.i;
        while lx.i < lx.b.len() && lx.b[lx.i].is_ascii_digit() {
            lx.i += 1;
        }
        let id: u32 = std::str::from_utf8(&lx.b[s..lx.i])
            .ok()
            .and_then(|x| x.parse().ok())
            .ok_or("an entity id is not valid")?;
        lx.expect(b'=')?;
        let parts = if lx.peek() == Some(b'(') {
            lx.i += 1;
            let mut parts = Vec::new();
            while lx.peek() != Some(b')') {
                let n = lx.ident();
                if n.is_empty() {
                    return Err(format!("the complex entity #{id} is not valid"));
                }
                parts.push((n, lx.args()?));
            }
            lx.i += 1;
            parts
        } else {
            let n = lx.ident();
            vec![(n, lx.args()?)]
        };
        lx.expect(b';')?;
        out.insert(id, Entity { parts });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_and_complex() {
        let src =
            b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=CARTESIAN_POINT('a',(1.,-2.5E-1,3.));\n\
#2=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );\n#3=DIRECTION('',(0.,0.,1.));\n\
#4=PLANE('it''s',#5);\nENDSEC;\nEND-ISO-10303-21;\n";
        let m = parse(src).unwrap();
        assert_eq!(m.len(), 4);
        assert_eq!(m[&1].args()[1].list()[1].num(), Some(-0.25));
        assert!(m[&2].is("SI_UNIT"));
        assert_eq!(m[&2].part("SI_UNIT").unwrap()[0].enum_name(), Some("MILLI"));
        assert_eq!(m[&4].args()[0].text(), Some("it's"));
    }
}
