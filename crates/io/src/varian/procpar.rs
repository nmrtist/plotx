use crate::IoError;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(super) enum Value {
    Number(f64),
    Text(String),
}

#[derive(Debug, Default)]
pub(super) struct Procpar {
    values: HashMap<String, Vec<Value>>,
}

impl Procpar {
    pub(super) fn parse(text: &str) -> Result<Self, IoError> {
        let mut lines = text.lines().enumerate().peekable();
        let mut values = HashMap::new();
        while let Some((line_no, header)) = lines.next() {
            if header.trim().is_empty() {
                continue;
            }
            let fields = tokens(header).map_err(|e| invalid(line_no, e))?;
            if fields.len() < 3 {
                return Err(invalid(
                    line_no,
                    "parameter header has fewer than three fields",
                ));
            }
            let name = fields[0].clone();
            let basic_type: i32 = fields[2]
                .parse()
                .map_err(|_| invalid(line_no, "invalid basic type"))?;
            if basic_type != 1 && basic_type != 2 {
                return Err(invalid(line_no, "unsupported basic type"));
            }
            let (value_line_no, first) = lines
                .next()
                .ok_or_else(|| invalid(line_no, "missing value record"))?;
            let mut value_tokens = tokens(first).map_err(|e| invalid(value_line_no, e))?;
            let count = parse_count(&mut value_tokens, value_line_no, "value")?;
            while value_tokens.len() < count {
                let (continuation_no, continuation) = lines
                    .next()
                    .ok_or_else(|| invalid(value_line_no, "truncated value record"))?;
                value_tokens.extend(tokens(continuation).map_err(|e| invalid(continuation_no, e))?);
            }
            if value_tokens.len() != count {
                return Err(invalid(value_line_no, "value count does not match record"));
            }
            let parsed = value_tokens
                .into_iter()
                .map(|token| {
                    if basic_type == 1 {
                        token.parse::<f64>().map(Value::Number).map_err(|_| {
                            invalid(
                                value_line_no,
                                "numeric parameter contains non-numeric value",
                            )
                        })
                    } else {
                        Ok(Value::Text(token))
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;

            let (enum_line_no, enum_first) = lines
                .next()
                .ok_or_else(|| invalid(value_line_no, "missing enumeration record"))?;
            let mut enum_tokens = tokens(enum_first).map_err(|e| invalid(enum_line_no, e))?;
            let enum_count = parse_count(&mut enum_tokens, enum_line_no, "enumeration")?;
            while enum_tokens.len() < enum_count {
                let (continuation_no, continuation) = lines
                    .next()
                    .ok_or_else(|| invalid(enum_line_no, "truncated enumeration record"))?;
                enum_tokens.extend(tokens(continuation).map_err(|e| invalid(continuation_no, e))?);
            }
            if enum_tokens.len() != enum_count {
                return Err(invalid(
                    enum_line_no,
                    "enumeration count does not match record",
                ));
            }
            values.insert(name, parsed);
        }
        Ok(Self { values })
    }

    pub(super) fn numbers(&self, name: &str) -> Option<Vec<f64>> {
        self.values
            .get(name)?
            .iter()
            .map(|v| match v {
                Value::Number(n) => Some(*n),
                Value::Text(_) => None,
            })
            .collect()
    }

    pub(super) fn number(&self, name: &str) -> Option<f64> {
        self.numbers(name)?.first().copied()
    }

    pub(super) fn strings(&self, name: &str) -> Option<Vec<&str>> {
        self.values
            .get(name)?
            .iter()
            .map(|v| match v {
                Value::Text(s) => Some(s.as_str()),
                Value::Number(_) => None,
            })
            .collect()
    }

    pub(super) fn string(&self, name: &str) -> Option<&str> {
        self.strings(name)?.first().copied()
    }
}

fn parse_count(tokens: &mut Vec<String>, line: usize, kind: &str) -> Result<usize, IoError> {
    if tokens.is_empty() {
        return Err(invalid(line, format!("missing {kind} count")));
    }
    let count = tokens
        .remove(0)
        .parse::<usize>()
        .map_err(|_| invalid(line, format!("invalid {kind} count")))?;
    Ok(count)
}

fn invalid(line: usize, message: impl Into<String>) -> IoError {
    IoError::InvalidVarian(format!("procpar line {}: {}", line + 1, message.into()))
}

fn tokens(line: &str) -> Result<Vec<String>, &'static str> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if c == '"' {
            let mut value = String::new();
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == '"' {
                    closed = true;
                    break;
                }
                if c == '\\' {
                    value.push(chars.next().ok_or("unterminated escape in quoted string")?);
                } else {
                    value.push(c);
                }
            }
            if !closed {
                return Err("unterminated quoted string");
            }
            out.push(value);
        } else {
            let mut value = String::from(c);
            while chars.peek().is_some_and(|c| !c.is_whitespace()) {
                value.push(chars.next().unwrap());
            }
            out.push(value);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numbers_strings_arrays_and_enums() {
        let p = Procpar::parse("sw 1 1 0 0 0 0 0 0 1 0\n2 1000 2000\n1 5000\ncomment 1 2 0 0 0 0 0 0 1 0\n1 \"a value with spaces\"\n2 \"yes\" \"no\"\n").unwrap();
        assert_eq!(p.numbers("sw"), Some(vec![1000.0, 2000.0]));
        assert_eq!(p.string("comment"), Some("a value with spaces"));
    }

    #[test]
    fn rejects_bad_counts_and_quotes() {
        assert!(Procpar::parse("x 1 1\n2 1\n0\n").is_err());
        assert!(Procpar::parse("x 1 2\n1 \"oops\n0\n").is_err());
    }
}
