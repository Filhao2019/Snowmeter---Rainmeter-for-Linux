//! Port of Rainmeter's `MathParser` (an operator-precedence stack machine).
//!
//! The quirks of the original are kept on purpose: `^` is bitwise XOR, `**`
//! is power, all binary operators are left-associative, `&&`/`||` share the
//! comparison priority, both ternary branches are always evaluated, and a `-`
//! that follows an operator must start a numeric literal.

use crate::numparse;

const E_BRACKETS: &str = "Unmatched brackets";
const E_SYNTAX: &str = "Syntax error";
const E_INTERNAL: &str = "Internal error";
const E_EXTRA_OP: &str = "Extra operation";
const E_DIV0: &str = "Division by 0";
const E_LOGIC: &str = "Logical expression error";
const E_PARAM_COUNT: &str = "Invalid function parameter count";

const OP_STACK: usize = 96;
const NUM_STACK: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Op {
    ShiftLeft,
    ShiftRight,
    Power,
    NotEqual,
    GreaterOrEqual,
    LessOrEqual,
    LogicalAnd,
    LogicalOr,
    OpeningBracket,
    Addition,
    Subtraction,
    Multiplication,
    Division,
    Modulo,
    BitwiseXor,
    BitwiseNot,
    BitwiseAnd,
    BitwiseOr,
    Equal,
    Greater,
    Less,
    Conditional,
    ConditionalSeparator,
    ClosingBracket,
    Comma,
    SingleArgFunction,
    MultiArgFunction,
    Invalid,
}

impl Op {
    fn priority(self) -> u8 {
        use Op::*;
        match self {
            ShiftLeft | ShiftRight | Power | BitwiseXor | BitwiseNot | BitwiseAnd | BitwiseOr => 5,
            NotEqual | GreaterOrEqual | LessOrEqual | LogicalAnd | LogicalOr | Equal | Greater | Less
            | ConditionalSeparator | Comma => 2,
            OpeningBracket | ClosingBracket => 0,
            Addition | Subtraction => 3,
            Multiplication | Division | Modulo => 4,
            Conditional => 1,
            SingleArgFunction | MultiArgFunction => 6,
            Invalid => 0,
        }
    }
}

#[derive(Clone, Copy)]
struct Operation {
    ty: Op,
    func: usize,
    prev_top: i32,
}

const BR_OP: Operation = Operation { ty: Op::OpeningBracket, func: 0, prev_top: 0 };
const NEG_OP: Operation = Operation { ty: Op::SingleArgFunction, func: F_NEG, prev_top: 0 };

// Function table, same order and names as the original.
const F_ATAN2: usize = 0;
const F_NEG: usize = 20;
const F_E: usize = 24;
const F_PI: usize = 25;
const FUNC_NAMES: [&str; 26] = [
    "atan2", "atan", "cos", "sin", "tan", "abs", "exp", "ln", "log", "sqrt", "frac", "trunc", "floor", "ceil",
    "round", "asin", "acos", "rad", "deg", "sgn", "neg", "min", "max", "clamp", "e", "pi",
];
const FUNC_MAX_LEN: usize = 5;

fn is_multi_arg(f: usize) -> bool {
    matches!(FUNC_NAMES[f], "atan2" | "round" | "min" | "max" | "clamp")
}

fn single_arg(f: usize, x: f64) -> f64 {
    match FUNC_NAMES[f] {
        "atan" => x.atan(),
        "cos" => x.cos(),
        "sin" => x.sin(),
        "tan" => x.tan(),
        "abs" => x.abs(),
        "exp" => x.exp(),
        "ln" => x.ln(),
        "log" => x.log10(),
        "sqrt" => x.sqrt(),
        "frac" => x.fract(),
        "trunc" => x.trunc(),
        "floor" => x.floor(),
        "ceil" => x.ceil(),
        "asin" => x.asin(),
        "acos" => x.acos(),
        "rad" => x / 180.0 * std::f64::consts::PI,
        "deg" => x * (180.0 / std::f64::consts::PI),
        "sgn" => {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                0.0
            }
        }
        "neg" => -x,
        _ => f64::NAN,
    }
}

fn multi_arg(f: usize, args: &[f64]) -> Result<f64, &'static str> {
    let n = args.len();
    match FUNC_NAMES[f] {
        "min" if n == 2 => Ok(if args[0] < args[1] { args[0] } else { args[1] }),
        "max" if n == 2 => Ok(if args[0] > args[1] { args[0] } else { args[1] }),
        "clamp" if n == 3 => {
            let (x, a, b) = (args[0], args[1], args[2]);
            Ok(if x < a {
                a
            } else if x > b {
                b
            } else {
                x
            })
        }
        "atan2" if n == 2 => Ok(args[0].atan2(args[1])),
        "round" if n == 1 || n == 2 => {
            let mut sharp = if n == 2 { args[1] as i32 } else { 0 };
            let mut x = args[0];
            let mut coef = 10.0;
            if sharp < 0 {
                coef = 0.1;
                sharp = -sharp;
            }
            for _ in 0..sharp {
                x *= coef;
            }
            x += if x >= 0.0 { 0.5 } else { -0.5 };
            x = if x >= 0.0 { x.floor() } else { x.ceil() };
            for _ in 0..sharp {
                x /= coef;
            }
            Ok(x)
        }
        _ => Err(E_PARAM_COUNT),
    }
}

fn function_index(name: &[char]) -> Option<usize> {
    if name.len() > FUNC_MAX_LEN {
        return None;
    }
    let lower: String = name.iter().map(|c| c.to_ascii_lowercase()).collect();
    FUNC_NAMES.iter().position(|n| *n == lower)
}

struct Parser {
    ops: [Operation; OP_STACK],
    nums: [f64; NUM_STACK],
    op_top: i32,
    val_top: i32,
    obr_dist: i32,
}

impl Parser {
    fn new() -> Parser {
        let mut ops = [BR_OP; OP_STACK];
        ops[0] = BR_OP;
        Parser { ops, nums: [0.0; NUM_STACK], op_top: 0, val_top: -1, obr_dist: 2 }
    }

    fn push_op(&mut self, op: Operation) {
        self.op_top += 1;
        self.ops[self.op_top as usize] = op;
    }

    fn push_num(&mut self, v: f64) {
        self.val_top += 1;
        self.nums[self.val_top as usize] = v;
    }

    fn pop_num(&mut self) -> f64 {
        let v = self.nums[self.val_top as usize];
        self.val_top -= 1;
        v
    }

    fn calc(&mut self) -> Result<(), String> {
        if self.op_top < 0 {
            return Err(E_INTERNAL.into());
        }
        let op = self.ops[self.op_top as usize];
        self.op_top -= 1;

        if op.ty == Op::Conditional {
            return Ok(());
        }
        if op.ty == Op::MultiArgFunction {
            let count = self.val_top - op.prev_top;
            if count < 0 {
                return Err(E_INTERNAL.into());
            }
            self.val_top = op.prev_top;
            let from = (self.val_top + 1) as usize;
            let args: Vec<f64> = self.nums[from..from + count as usize].to_vec();
            let res = multi_arg(op.func, &args)?;
            self.push_num(res);
            return Ok(());
        }
        if self.val_top < 0 {
            return Err(E_EXTRA_OP.into());
        }
        let right = self.pop_num();

        let res = if op.ty == Op::BitwiseNot {
            (!(right as i64)) as f64
        } else if op.ty == Op::SingleArgFunction {
            single_arg(op.func, right)
        } else {
            if self.val_top < 0 {
                return Err(E_EXTRA_OP.into());
            }
            let left = self.pop_num();
            let (li, ri) = (left as i64, right as i64);
            match op.ty {
                Op::ShiftLeft => li.wrapping_shl(ri as u32) as f64,
                Op::ShiftRight => li.wrapping_shr(ri as u32) as f64,
                Op::Power => left.powf(right),
                Op::NotEqual => (left != right) as u8 as f64,
                Op::GreaterOrEqual => (left >= right) as u8 as f64,
                Op::LessOrEqual => (left <= right) as u8 as f64,
                Op::LogicalAnd => (left != 0.0 && right != 0.0) as u8 as f64,
                Op::LogicalOr => (left != 0.0 || right != 0.0) as u8 as f64,
                Op::Addition => left + right,
                Op::Subtraction => left - right,
                Op::Multiplication => left * right,
                Op::Division => {
                    if right == 0.0 {
                        return Err(E_DIV0.into());
                    }
                    left / right
                }
                Op::Modulo => left % right,
                Op::BitwiseXor => (li ^ ri) as f64,
                Op::BitwiseAnd => (li & ri) as f64,
                Op::BitwiseOr => (li | ri) as f64,
                Op::Equal => (left == right) as u8 as f64,
                Op::Greater => (left > right) as u8 as f64,
                Op::Less => (left < right) as u8 as f64,
                Op::ConditionalSeparator => {
                    if self.op_top < 0 {
                        return Err(E_LOGIC.into());
                    }
                    let cond_op = self.ops[self.op_top as usize];
                    self.op_top -= 1;
                    if cond_op.ty != Op::Conditional || self.val_top < 0 {
                        return Err(E_LOGIC.into());
                    }
                    let cond = self.pop_num();
                    if cond != 0.0 {
                        left
                    } else {
                        right
                    }
                }
                _ => return Err(E_INTERNAL.into()),
            }
        };
        self.push_num(res);
        Ok(())
    }

    fn calc_to_obr(&mut self) -> Result<(), String> {
        loop {
            if self.op_top < 0 {
                return Err(E_INTERNAL.into());
            }
            if self.ops[self.op_top as usize].ty == Op::OpeningBracket {
                break;
            }
            self.calc()?;
        }
        self.op_top -= 1;
        Ok(())
    }
}

// ---- lexer ----------------------------------------------------------------

const CT_UNKNOWN: u8 = 0;
const CT_LETTER: u8 = 1;
const CT_DIGIT: u8 = 2;
const CT_SEPARATOR: u8 = 4;
const CT_SYMBOL: u8 = 8;
const CT_MINUS: u8 = 0x10;
const CT_FINAL: u8 = 0x7F;

fn char_type(ch: char) -> u8 {
    match ch {
        '\0' => CT_FINAL,
        ' ' | '\t' | '\n' => CT_SEPARATOR,
        '-' => CT_MINUS,
        '+' | '/' | '*' | '~' | '(' | ')' | '<' | '>' | '%' | ',' | '?' | ':' | '=' | '&' | '|' | '^' => CT_SYMBOL,
        c if c.is_ascii_digit() => CT_DIGIT,
        c if !c.is_whitespace() && !c.is_control() => CT_LETTER,
        _ => CT_UNKNOWN,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Token {
    Error,
    None,
    Final,
    Operator,
    Number,
    Name,
}

struct Lexer<'a> {
    s: &'a [char],
    pos: usize,
    name: (usize, usize),
    token: Token,
    oper: Op,
    num: f64,
    ct: u8,
}

impl<'a> Lexer<'a> {
    fn new(s: &'a [char]) -> Lexer<'a> {
        let ct = s.first().map_or(CT_FINAL, |&c| char_type(c));
        Lexer { s, pos: 0, name: (0, 0), token: Token::None, oper: Op::Invalid, num: 0.0, ct }
    }

    fn ct_at(&self, pos: usize) -> u8 {
        self.s.get(pos).map_or(CT_FINAL, |&c| char_type(c))
    }

    fn operator_at(&self) -> Op {
        let c = self.s[self.pos];
        let next = self.s.get(self.pos + 1).copied();
        match c {
            '(' => Op::OpeningBracket,
            '+' => Op::Addition,
            '-' => Op::Subtraction,
            '*' => if next == Some('*') { Op::Power } else { Op::Multiplication },
            '/' => Op::Division,
            '%' => Op::Modulo,
            '^' => Op::BitwiseXor,
            '~' => Op::BitwiseNot,
            '&' => if next == Some('&') { Op::LogicalAnd } else { Op::BitwiseAnd },
            '|' => if next == Some('|') { Op::LogicalOr } else { Op::BitwiseOr },
            '=' => Op::Equal,
            '>' => match next {
                Some('>') => Op::ShiftRight,
                Some('=') => Op::GreaterOrEqual,
                _ => Op::Greater,
            },
            '<' => match next {
                Some('>') => Op::NotEqual,
                Some('<') => Op::ShiftLeft,
                Some('=') => Op::LessOrEqual,
                _ => Op::Less,
            },
            '?' => Op::Conditional,
            ':' => Op::ConditionalSeparator,
            ')' => Op::ClosingBracket,
            ',' => Op::Comma,
            _ => Op::Invalid,
        }
    }

    fn next_token(&mut self) -> Token {
        while self.ct == CT_SEPARATOR {
            self.pos += 1;
            self.ct = self.ct_at(self.pos);
        }
        if self.ct == CT_MINUS {
            // A '-' after an operator (other than brackets) starts a negative number.
            self.ct = CT_SYMBOL;
            if self.token == Token::Operator && self.oper != Op::OpeningBracket && self.oper != Op::ClosingBracket {
                self.ct = CT_DIGIT;
            }
        }
        match self.ct {
            CT_FINAL => self.token = Token::Final,
            CT_LETTER => {
                self.token = Token::Name;
                let start = self.pos;
                loop {
                    self.pos += 1;
                    self.ct = self.ct_at(self.pos);
                    if self.ct > CT_DIGIT {
                        break;
                    }
                }
                self.name = (start, self.pos);
            }
            CT_DIGIT => self.lex_number(),
            CT_SYMBOL => {
                let oper = self.operator_at();
                if oper != Op::Invalid {
                    self.token = Token::Operator;
                    self.oper = oper;
                    self.pos += if (oper as usize) <= (Op::LogicalOr as usize) { 2 } else { 1 };
                    self.ct = self.ct_at(self.pos);
                }
            }
            _ => self.token = Token::Error,
        }
        self.token
    }

    /// Numbers: `0x..`, `0o..`, `0b..`, otherwise `strtod`. If nothing could be
    /// scanned (e.g. `-x`) the previous token is returned unchanged, exactly as
    /// the original does.
    fn lex_number(&mut self) {
        let has_next = self.pos + 1 < self.s.len();
        if self.s[self.pos] == '0' && has_next {
            let (value, end, valid) = match self.s[self.pos + 1] {
                'x' => {
                    let (v, e, _) = numparse::strtol(self.s, self.pos, 16);
                    (v, e, true)
                }
                'o' => {
                    let (v, e, _) = numparse::strtol(self.s, self.pos + 2, 8);
                    (v, if e == self.pos + 2 { self.pos + 2 } else { e }, true)
                }
                'b' => {
                    let (v, e, _) = numparse::strtol(self.s, self.pos + 2, 2);
                    (v, if e == self.pos + 2 { self.pos + 2 } else { e }, true)
                }
                _ => (0, 0, false),
            };
            if valid {
                // `0x` with no hex digit consumes just the leading "0".
                let end = if end == self.pos && self.s[self.pos + 1] == 'x' { self.pos + 1 } else { end };
                if end != self.pos {
                    self.token = Token::Number;
                    self.num = value as f64;
                    self.pos = end;
                    self.ct = self.ct_at(self.pos);
                }
                return;
            }
        }
        let (v, end) = numparse::strtod(self.s, self.pos);
        if end != self.pos {
            self.token = Token::Number;
            self.num = v;
            self.pos = end;
            self.ct = self.ct_at(self.pos);
        }
    }
}

// ---- driver -----------------------------------------------------------------

/// Evaluate a formula. `lookup` resolves names that are not built-in
/// functions or constants (measure values for skin options). Brackets are
/// balance-checked first, like `MathParser::CheckedParse`.
pub fn evaluate(expr: &str, lookup: &dyn Fn(&str) -> Option<f64>) -> Result<f64, String> {
    let chars: Vec<char> = expr.chars().collect();
    let balance: i32 = chars.iter().map(|&c| (c == '(') as i32 - (c == ')') as i32).sum();
    if balance != 0 {
        return Err(E_BRACKETS.into());
    }
    parse(&chars, lookup)
}

fn parse(chars: &[char], lookup: &dyn Fn(&str) -> Option<f64>) -> Result<f64, String> {
    if chars.is_empty() {
        return Ok(0.0);
    }
    let mut p = Parser::new();
    let mut lx = Lexer::new(chars);

    loop {
        if p.op_top == OP_STACK as i32 - 2 || p.val_top == NUM_STACK as i32 - 2 {
            return Err(E_INTERNAL.into());
        }
        let token = lx.next_token();
        p.obr_dist -= 1;
        match token {
            Token::Error | Token::None => return Err(E_SYNTAX.into()),
            Token::Final => {
                p.calc_to_obr()?;
                if p.op_top != -1 || p.val_top != 0 {
                    return Err(E_INTERNAL.into());
                }
                return Ok(p.nums[0]);
            }
            Token::Number => p.push_num(lx.num),
            Token::Operator => match lx.oper {
                Op::OpeningBracket => {
                    p.push_op(BR_OP);
                    p.obr_dist = 2;
                }
                Op::ClosingBracket => p.calc_to_obr()?,
                Op::Comma => {
                    p.calc_to_obr()?;
                    if p.op_top >= 0 && p.ops[p.op_top as usize].ty == Op::MultiArgFunction {
                        p.push_op(BR_OP);
                        p.obr_dist = 2;
                    } else {
                        return Err(E_SYNTAX.into());
                    }
                }
                oper => {
                    match oper {
                        Op::Addition if p.obr_dist >= 1 => continue,
                        Op::Subtraction if p.obr_dist >= 1 => {
                            p.push_op(NEG_OP);
                            continue;
                        }
                        Op::Conditional | Op::ConditionalSeparator => p.obr_dist = 2,
                        _ => {}
                    }
                    while p.op_top >= 0 && oper.priority() <= p.ops[p.op_top as usize].ty.priority() {
                        p.calc()?;
                    }
                    p.push_op(Operation { ty: oper, func: 0, prev_top: 0 });
                }
            },
            Token::Name => {
                let (a, b) = lx.name;
                let name = &chars[a..b];
                if let Some(f) = function_index(name) {
                    if f == F_E {
                        p.push_num(std::f64::consts::E);
                    } else if f == F_PI {
                        p.push_num(std::f64::consts::PI);
                    } else if is_multi_arg(f) || f == F_ATAN2 {
                        p.push_op(Operation { ty: Op::MultiArgFunction, func: f, prev_top: p.val_top });
                    } else {
                        p.push_op(Operation { ty: Op::SingleArgFunction, func: f, prev_top: 0 });
                    }
                } else {
                    let n: String = name.iter().collect();
                    match lookup(&n) {
                        Some(v) => p.push_num(v),
                        None => return Err(format!("\"{n}\" is unknown")),
                    }
                }
            }
        }
    }
}
