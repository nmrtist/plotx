use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const MAX_SOURCE_BYTES: usize = 64 * 1024;
const MAX_TOKENS: usize = 16_384;
const MAX_DEPTH: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourcePosition {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceError {
    pub message: String,
    pub position: SourcePosition,
}

impl SourceError {
    fn new(message: impl Into<String>, position: SourcePosition) -> Self {
        Self {
            message: message.into(),
            position,
        }
    }
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}",
            self.message, self.position.line, self.position.column
        )
    }
}

impl std::error::Error for SourceError {}

#[derive(Debug, Clone, PartialEq)]
pub struct Expression {
    pub(crate) kind: ExprKind,
    pub(crate) position: SourcePosition,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ExprKind {
    Number(f64),
    Symbol(String),
    Unary(UnaryOp, Box<Expression>),
    Binary(BinaryOp, Box<Expression>, Box<Expression>),
    Call(String, Vec<Expression>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnaryOp {
    Positive,
    Negative,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Pow,
    LParen,
    RParen,
    Comma,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
    Not,
    End,
}

#[derive(Debug, Clone, PartialEq)]
struct Token {
    kind: TokenKind,
    position: SourcePosition,
}

pub fn parse_expression(source: &str) -> Result<Expression, SourceError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(SourceError::new(
            "expression exceeds the 64 KiB resource limit",
            SourcePosition { line: 1, column: 1 },
        ));
    }
    let tokens = lex(source)?;
    let mut parser = Parser {
        tokens,
        cursor: 0,
        depth: 0,
    };
    let expression = parser.parse_bp(0)?;
    if !matches!(parser.current().kind, TokenKind::End) {
        return Err(SourceError::new(
            "unexpected token after expression",
            parser.current().position,
        ));
    }
    Ok(expression)
}

/// Finds identifiers that are not deterministic functions or mathematical constants.
pub fn discover_symbols(source: &str) -> Result<Vec<String>, SourceError> {
    let expression = parse_expression(source)?;
    let mut symbols = BTreeSet::new();
    expression.collect_symbols(&mut symbols);
    Ok(symbols.into_iter().collect())
}

impl Expression {
    pub fn evaluate(&self, values: &BTreeMap<String, f64>) -> Result<f64, SourceError> {
        self.eval(&|name| values.get(name).copied())
    }

    pub(crate) fn eval(&self, lookup: &impl Fn(&str) -> Option<f64>) -> Result<f64, SourceError> {
        let value = match &self.kind {
            ExprKind::Number(value) => *value,
            ExprKind::Symbol(name) => match name.as_str() {
                "pi" => std::f64::consts::PI,
                "e" => std::f64::consts::E,
                _ => lookup(name).ok_or_else(|| {
                    SourceError::new(format!("unknown symbol '{name}'"), self.position)
                })?,
            },
            ExprKind::Unary(op, value) => {
                let value = value.eval(lookup)?;
                match op {
                    UnaryOp::Positive => value,
                    UnaryOp::Negative => -value,
                    UnaryOp::Not => bool_num(!truthy(value)),
                }
            }
            ExprKind::Binary(op, left, right) => {
                let left = left.eval(lookup)?;
                if *op == BinaryOp::And && !truthy(left) {
                    return Ok(0.0);
                }
                if *op == BinaryOp::Or && truthy(left) {
                    return Ok(1.0);
                }
                let right = right.eval(lookup)?;
                match op {
                    BinaryOp::Add => left + right,
                    BinaryOp::Sub => left - right,
                    BinaryOp::Mul => left * right,
                    BinaryOp::Div => left / right,
                    BinaryOp::Pow => left.powf(right),
                    BinaryOp::Lt => bool_num(left < right),
                    BinaryOp::Le => bool_num(left <= right),
                    BinaryOp::Gt => bool_num(left > right),
                    BinaryOp::Ge => bool_num(left >= right),
                    BinaryOp::Eq => bool_num(left == right),
                    BinaryOp::Ne => bool_num(left != right),
                    BinaryOp::And => bool_num(truthy(right)),
                    BinaryOp::Or => bool_num(truthy(right)),
                }
            }
            ExprKind::Call(name, arguments) => {
                evaluate_call(name, arguments, lookup, self.position)?
            }
        };
        if value.is_nan() {
            return Err(SourceError::new(
                "expression is outside its mathematical domain",
                self.position,
            ));
        }
        Ok(value)
    }

    pub(crate) fn collect_symbols(&self, output: &mut BTreeSet<String>) {
        match &self.kind {
            ExprKind::Symbol(name) if name != "pi" && name != "e" => {
                output.insert(name.clone());
            }
            ExprKind::Unary(_, value) => value.collect_symbols(output),
            ExprKind::Binary(_, left, right) => {
                left.collect_symbols(output);
                right.collect_symbols(output);
            }
            ExprKind::Call(_, args) => {
                for arg in args {
                    arg.collect_symbols(output);
                }
            }
            _ => {}
        }
    }
}

fn evaluate_call(
    name: &str,
    args: &[Expression],
    lookup: &impl Fn(&str) -> Option<f64>,
    position: SourcePosition,
) -> Result<f64, SourceError> {
    if name == "log" {
        return Err(SourceError::new(
            "'log' is ambiguous; use 'ln' or 'log10'",
            position,
        ));
    }
    let expected = match name {
        "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh" | "cosh" | "tanh" | "exp"
        | "ln" | "log10" | "sqrt" | "abs" | "erf" | "gamma" | "Gamma" => 1,
        "min" | "max" | "pow" => 2,
        "if" => 3,
        _ => {
            return Err(SourceError::new(
                format!("unknown function '{name}'"),
                position,
            ));
        }
    };
    if args.len() != expected {
        return Err(SourceError::new(
            format!("function '{name}' expects {expected} argument(s)"),
            position,
        ));
    }
    if name == "if" {
        let condition = args[0].eval(lookup)?;
        return if truthy(condition) {
            args[1].eval(lookup)
        } else {
            args[2].eval(lookup)
        };
    }
    let a = args[0].eval(lookup)?;
    let b = || args[1].eval(lookup);
    Ok(match name {
        "sin" => a.sin(),
        "cos" => a.cos(),
        "tan" => a.tan(),
        "asin" => a.asin(),
        "acos" => a.acos(),
        "atan" => a.atan(),
        "sinh" => a.sinh(),
        "cosh" => a.cosh(),
        "tanh" => a.tanh(),
        "exp" => a.exp(),
        "ln" => a.ln(),
        "log10" => a.log10(),
        "sqrt" => a.sqrt(),
        "abs" => a.abs(),
        "erf" => statrs::function::erf::erf(a),
        "gamma" | "Gamma" => statrs::function::gamma::gamma(a),
        "min" => a.min(b()?),
        "max" => a.max(b()?),
        "pow" => a.powf(b()?),
        _ => unreachable!(),
    })
}

fn truthy(value: f64) -> bool {
    value != 0.0 && !value.is_nan()
}
fn bool_num(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    depth: usize,
}

impl Parser {
    fn current(&self) -> &Token {
        &self.tokens[self.cursor]
    }
    fn advance(&mut self) -> Token {
        let token = self.tokens[self.cursor].clone();
        self.cursor += 1;
        token
    }

    fn parse_bp(&mut self, min_bp: u8) -> Result<Expression, SourceError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(SourceError::new(
                "expression nesting exceeds the resource limit",
                self.current().position,
            ));
        }
        let token = self.advance();
        let mut left = match token.kind {
            TokenKind::Number(value) => Expression {
                kind: ExprKind::Number(value),
                position: token.position,
            },
            TokenKind::Ident(name) => {
                if matches!(self.current().kind, TokenKind::LParen) {
                    self.advance();
                    let mut args = Vec::new();
                    if !matches!(self.current().kind, TokenKind::RParen) {
                        loop {
                            args.push(self.parse_bp(0)?);
                            if matches!(self.current().kind, TokenKind::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    if !matches!(self.current().kind, TokenKind::RParen) {
                        return Err(SourceError::new("expected ')'", self.current().position));
                    }
                    self.advance();
                    Expression {
                        kind: ExprKind::Call(name, args),
                        position: token.position,
                    }
                } else {
                    Expression {
                        kind: ExprKind::Symbol(name),
                        position: token.position,
                    }
                }
            }
            TokenKind::Plus | TokenKind::Minus | TokenKind::Not => {
                let op = match token.kind {
                    TokenKind::Plus => UnaryOp::Positive,
                    TokenKind::Minus => UnaryOp::Negative,
                    _ => UnaryOp::Not,
                };
                let value = self.parse_bp(12)?;
                Expression {
                    kind: ExprKind::Unary(op, Box::new(value)),
                    position: token.position,
                }
            }
            TokenKind::LParen => {
                let expression = self.parse_bp(0)?;
                if !matches!(self.current().kind, TokenKind::RParen) {
                    return Err(SourceError::new("expected ')'", self.current().position));
                }
                self.advance();
                expression
            }
            _ => {
                return Err(SourceError::new(
                    "expected a number, identifier, or '('",
                    token.position,
                ));
            }
        };
        while let Some((left_bp, right_bp, op)) = infix_binding(&self.current().kind) {
            if left_bp < min_bp {
                break;
            }
            let position = self.advance().position;
            let right = self.parse_bp(right_bp)?;
            left = Expression {
                kind: ExprKind::Binary(op, Box::new(left), Box::new(right)),
                position,
            };
        }
        self.depth -= 1;
        Ok(left)
    }
}

fn infix_binding(token: &TokenKind) -> Option<(u8, u8, BinaryOp)> {
    Some(match token {
        TokenKind::Or => (1, 2, BinaryOp::Or),
        TokenKind::And => (3, 4, BinaryOp::And),
        TokenKind::Eq => (5, 6, BinaryOp::Eq),
        TokenKind::Ne => (5, 6, BinaryOp::Ne),
        TokenKind::Lt => (7, 8, BinaryOp::Lt),
        TokenKind::Le => (7, 8, BinaryOp::Le),
        TokenKind::Gt => (7, 8, BinaryOp::Gt),
        TokenKind::Ge => (7, 8, BinaryOp::Ge),
        TokenKind::Plus => (9, 10, BinaryOp::Add),
        TokenKind::Minus => (9, 10, BinaryOp::Sub),
        TokenKind::Star => (11, 12, BinaryOp::Mul),
        TokenKind::Slash => (11, 12, BinaryOp::Div),
        TokenKind::Pow => (14, 13, BinaryOp::Pow),
        _ => return None,
    })
}

fn lex(source: &str) -> Result<Vec<Token>, SourceError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let (mut i, mut line, mut column) = (0, 1, 1);
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            if c == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
            i += 1;
            continue;
        }
        let position = SourcePosition { line, column };
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && matches!(chars[i], 'e' | 'E') {
                i += 1;
                if i < chars.len() && matches!(chars[i], '+' | '-') {
                    i += 1;
                }
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let text: String = chars[start..i].iter().collect();
            let value = text
                .parse()
                .map_err(|_| SourceError::new("invalid number", position))?;
            column += i - start;
            tokens.push(Token {
                kind: TokenKind::Number(value),
                position,
            });
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            column += i - start;
            tokens.push(Token {
                kind: TokenKind::Ident(name),
                position,
            });
        } else {
            let next = chars.get(i + 1).copied();
            let (kind, consumed) = match (c, next) {
                ('*', Some('*')) => (TokenKind::Pow, 2),
                ('<', Some('=')) => (TokenKind::Le, 2),
                ('>', Some('=')) => (TokenKind::Ge, 2),
                ('=', Some('=')) => (TokenKind::Eq, 2),
                ('!', Some('=')) => (TokenKind::Ne, 2),
                ('&', Some('&')) => (TokenKind::And, 2),
                ('|', Some('|')) => (TokenKind::Or, 2),
                ('+', _) => (TokenKind::Plus, 1),
                ('-', _) => (TokenKind::Minus, 1),
                ('*', _) => (TokenKind::Star, 1),
                ('/', _) => (TokenKind::Slash, 1),
                ('^', _) => (TokenKind::Pow, 1),
                ('(', _) => (TokenKind::LParen, 1),
                (')', _) => (TokenKind::RParen, 1),
                (',', _) => (TokenKind::Comma, 1),
                ('<', _) => (TokenKind::Lt, 1),
                ('>', _) => (TokenKind::Gt, 1),
                ('!', _) => (TokenKind::Not, 1),
                _ => {
                    return Err(SourceError::new(
                        format!("unsupported character '{c}'"),
                        position,
                    ));
                }
            };
            i += consumed;
            column += consumed;
            tokens.push(Token { kind, position });
        }
        if tokens.len() > MAX_TOKENS {
            return Err(SourceError::new(
                "expression exceeds the token resource limit",
                position,
            ));
        }
    }
    tokens.push(Token {
        kind: TokenKind::End,
        position: SourcePosition { line, column },
    });
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(source: &str) -> f64 {
        parse_expression(source)
            .unwrap()
            .evaluate(&BTreeMap::new())
            .unwrap()
    }

    #[test]
    fn precedence_and_right_associative_power() {
        assert_eq!(eval("2 + 3 * 4"), 14.0);
        assert_eq!(eval("2^3^2"), 512.0);
        assert_eq!(eval("-2^2"), -4.0);
    }

    #[test]
    fn conditions_short_circuit_and_only_evaluate_selected_branch() {
        assert_eq!(eval("if(2 > 1 && 3 != 4, 8, sqrt(-1))"), 8.0);
    }

    #[test]
    fn ambiguous_log_has_actionable_error_and_position() {
        let error = parse_expression("log(10)")
            .unwrap()
            .evaluate(&BTreeMap::new())
            .unwrap_err();
        assert!(error.message.contains("ln"));
        assert_eq!(error.position, SourcePosition { line: 1, column: 1 });
    }

    #[test]
    fn symbol_discovery_is_sorted_and_excludes_constants_and_functions() {
        assert_eq!(
            discover_symbols("a*exp(-x/t) + pi").unwrap(),
            ["a", "t", "x"]
        );
    }
}
